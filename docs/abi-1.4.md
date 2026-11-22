# Host channel 1.4: own channel key K and unsolicited app messages

Stub and loader changes for hosts that cannot bind or poll: a phone app next to Nothing X on the system's shared
BLE link. Proof in the emulator:
`tests/test_chan_key_e2e.py`. Not yet on hardware. Apps see no change; `Api` is unchanged.

## Channel key K

- `K` = the pairing secret's 16 bytes: the 32 hex chars `AT GETSECRET` returns, which the stub reads from NVRAM
  `APP_SECRET` (`docs/recon.md`, "Project 2"). Host: `link.channel_key`, `Session.open_channel`,
  `Channel(channel_key=True)`.
- Frames `0x90A0..0x90A7` (install, `MSG`, `PING`) may be sealed with `K` instead of the stock session key, in the
  same format (`plain ‖ crc32le`, PKCS7, AES-128-CBC, IV `5051…665A`). The stub tries the stock key first, then `K`.
  Replies and `EVENT`s go out under the key of the last frame.
- No bind: no "Paired" popup, the stock session (Nothing X) is untouched, `K` survives reboots.
- App messages to a `K` host need a stock session word that is not zero (a bind since the last factory reset): the
  loader checks `session_word() == host_key`. Any watch paired with Nothing X has one.
- Security: as before. Any BLE client reads the secret; the owner accepted that (`docs/recon.md`, threat model).

## Shared link

Every host on the link sees every notification. A host skips frames and `EVENT`s its key cannot open
(`link.Session.request`, `channel.recv`). Switching between a stock host and a `K` host drops the app messages
queued for the other one (as a new host key always did).

## Writing a host (a phone companion)

What a host next to Nothing X does, in order. Implementations: the Python host (`link.Session.open_channel`,
`Channel(channel_key=True)`) and WireKit in the workout iPhone app (`KSession`), which is iOS only.

1. **Use the link the phone already has.** The watch takes one central, and on the phone it is the system's link
   that Nothing X holds. iOS: `retrieveConnectedPeripherals(withServices: [FFF0])`, else connect.
   Android: `connectGatt` to the bonded watch is expected to add a second GATT client on the same link
   (untested). A second device (a Mac while the phone is connected) does not work: that is another central.
2. **Characteristics.** Commands: write `0000FFF2-0000-1000-8000-00805F9B34FB`, notify
   `0000FFF1-0000-1000-8000-00805F9B34FB`. Shell: write `77D4FF01-2FE2-2334-0D35-9CCD078F529C`, notify
   `77D4FF02-2FE2-2334-0D35-9CCD078F529C`. Subscribe to both notifies.
3. **K.** Write `AT GETSECRET\r\n` to the shell; the reply is `GETSECRET:<32 hex>,OK`, possibly over several
   notifications. K is those 16 bytes. Cache it like a password: it is personal data (AGENTS.md, rule 2) and only
   changes with a factory reset.
4. **Frames.** `F5 ‖ len:u16be ‖ c1:u16be ‖ 00 01 ‖ 00 01 ‖ c2:u16be ‖ body`, `c1 = FFFF`, `len` = body length,
   `body = AES-128-CBC(K, IV, PKCS7(plain ‖ crc32le(plain)))`, IV `50 51 52 53 54 55 56 57 60 61 62 63 64 65 66 5A`.
   Each frame is one ATT write (write with response when over MTU−3). Opcodes and payloads: `docs/abi-1.1.md`,
   "Data channel"; the reply to `c2` is `c2 | 0xA000`.
5. **Reassemble and filter notifications.** Frames span notifications: find `F5`, wait for `11 + len` bytes. Drop
   every frame that does not open with K (bad padding or CRC): those are Nothing X's.
6. **Register with one `PING`** (`0x90A7`, payload `00`). From then on the stub sends the app's messages as `EVENT`
   (`0xB0A6`, plain `seq ‖ slot ‖ data`) without polling, if the loader is from 2026-11-13 or later; with an older
   loader, poll with `PING`.
7. **Detect the firmware.** No reply to a `K` frame: stock firmware or a stub without `K` (anything before the
   `fw-wp2.bin` of 2026-11-13); the user needs the OTA. `INFO` with slot `0xFE` tells stub versions apart
   ("Stub version" below).

The app on the watch side needs nothing new: `send`/`recv` as in `docs/app-guide.md`, section 4.

## Unsolicited app messages

- At boot the stub stores `StubRam.kick` (its `wp2_kick`) and `kick_magic = "KICK"`. The loader calls `kick` on the
  app thread right after the app's `send` queued a message, if the magic is there (an older stub leaves it unset;
  `api` stays 1 because loader 1.3 needs `api == 1`).
- With a `K` host listening, `wp2_kick` sends all queued messages at once (software AES, the stock crypto buffers are
  not used). The stock itself writes the stream from threads other than `wewear_read` (the bind popup's `0x0049`
  reply goes out from the UI thread), and the stream write has its own lock.
- `StubRam.busy` serializes the stub's frame path: `wewear_read` waits for it (`os_sleep(1)`), the kick skips when it
  is taken (`wewear_read` drains after its reply anyway).
- A stock host still polls with `PING`. A `K` host may poll too, but does not have to: 40 messages in a burst
  arrived without a host frame, none lost, `seq` contiguous (emulator).

## App order in the menu (loader of 2026-11-19, no ABI bump)

The phone sets the order of the Apps menu by installing a loader package with a `WP2ORDER` table: no new command,
any stub. Contract: `docs/app-order.md`.

## Install order

New stub (OTA, `docs/ota.md`) and new loader (`wp2apps install target/loader.wp2app`). Each works with the other's
old version: an old loader never calls `kick` (messages wait for the host's next frame), an old stub has no magic.

## Cards (loader of 2026-11-13, no ABI bump)

`FOCUS` with `icon = 0xFFFF` (`lv::FOCUS_CARDS`) gives the stock Exercise list instead of the app list: every
child is a card (`stock::focus_card`) that narrows with its distance from the centre line, 434 px in the centre and
`434 − 2·shift` further out (`lvcmd::card_width`, 370 px one row away, as in the stock list); text fades as before.
The container lays the cards out at a fixed x (`stock::focus_page` sets `PadLeft` 16 and a start-aligned flex) and
the loader centres each one with `TRANSLATE_X = shift`: a width set from `LAYOUT_CHANGED` is not laid out again, so
a flex centre lags one scroll behind. The loader owns the cards' `Width`, `TranslateX` and `TextOpa`. `ABI_MINOR`
stays 3: the stub on the watch refuses packages above its `LOADER_ABI_MINOR` (3), and raising it takes an OTA. An
older loader answers `-22` to this FOCUS, so install the loader before an app that uses it.

## Heart rate (loader of 2026-11-14, no ABI bump)

Live heart rate for an app, as the stock Strength training shows it under the clock. Stock side:
`docs/recon.md`, "Heart rate". Emulator proof: `tests/test_hr_e2e.py` (app `sdk/examples/hr-probe`, a canvas app).
Not yet on hardware, and the emulator has no PPG model: the test writes a synthetic reading into the stock cell.

### Why not a new `Api` field

The stub on the watch refuses packages above its `LOADER_ABI_MINOR` (3), and raising it takes an OTA. Options that
keep `abi_minor` 3:

1. A record on `lv_submit` (taken). The table has the field since 1.2, an older loader answers `-22` to an unknown
   op, as with `FOCUS_CARDS`.
2. Extra values on `screen_on(bool)` or `own_back(bool)`: a `bool` above 1 is undefined in Rust and mixes meanings.
3. A package flag ("uses heart rate") that the loader reads at launch: no off switch while the app runs, and an
   older loader ignores it silently.
4. The host channel (`send`): goes to the phone, not to the loader.

### The call

SDK: `Api::heart_rate(&self, on: bool) -> i32`, event kind `HEART` = 9.

- `on = true`: the loader starts the stock measurement (ux2sys property 2 = 1, as the stock heart view does; the
  stock reading is cleared first) and queues `Event { kind: HEART, x: worn, delta: bpm }` once per 1000 ms
  (`wp2_core::heart::EMIT_MS`) while the app runs in the foreground. `delta` 0 = no reading yet, or off the wrist
  (`x` 0). Queued HEART events coalesce: the app sees the latest.
- `on = false` stops it. Returns 0, or `-22` on an older loader (or `abi_minor < 2`): no heart rate there.
- The loader stops the measurement on exit, crash (`exit(-1)` from the panic handler), force stop, view delete,
  and `PAUSE`; it starts it again on `RESUME`, with the reading cleared. It keeps measuring while the panel is dark
  (AOD included): the readings keep coming, so a workout has heart rate through its rests; the watch stays out of deep
  sleep meanwhile. AOD sends no `PAUSE` since the loader of 2026-11-16: the launcher's AOD enter (`0x100677C0`)
  opens the AOD overlay as stock msgbox `0x113` (`msgbox_cache_popup`, the function the charge popup goes through),
  which sets the popup flags `0x3800FAF4`; the loader of 2026-11-14 paused on any popup flag, so every dark panel
  with AOD on paused the app, stopped the measurement and restarted it on wake (the hardware symptom: `PAUSE` and
  `RESUME` a few times a minute, no reading). Now msgbox `0x113` does not count (`events::popup_covers`), nor does
  a `DEFOCUS` after the view's own early-suspend `0x82`; a call or notification box over AOD still pauses. HEART
  events go from `next_event` straight to the app (`hr::due`), not through the event queue, whose only producer is
  the UI thread. Emulator proof: `tests/test_hr_e2e.py` `test_aod_overlay_no_pause` (the AOD msgbox state written by
  the test: the emulator cannot run AOD) and `test_long_aod` (6 min dark: no pause, one start, HEART every second,
  the HR watchdog count below 5, no return to the face). A stop while
  the app runs queues one HEART with `delta` 0: no reading while it is off. A new launch starts with it off.
- The panel keeps its own auto-sleep: the loader does not take the heart view's 25 s hold (`docs/recon.md`, "Heart
  view: screen"). A reading needs the panel lit for a few seconds; an app that wants one holds the screen itself
  (`screen_on`). The app's request is a wish
  (`fw/wp2-loader/src/hr.rs`); the UI tick does the stock calls, the rule is `wp2_core::heart::Heart::tick`.
- Exactly one start per hold, one stop at its end: the stock counts requests (`docs/recon.md`), so a repeated
  start would keep the sensor on after the app left. The background measurement uses the same count.
- A stock owner already measuring (owner byte `0x3804B30F` non-zero, e.g. a stock workout left running): the loader
  only reads and never starts or stops it.

### Wire format

A batch holding one SENSOR record, `[0x13, 2, 0, 0, sensor, on]` (sensor 1 = heart rate, on 0 or 1;
`wp2_core::lvcmd::sensor`). The loader takes it in `lv::submit` on the app thread, before LVGL: no root is made,
so a canvas app may send it. In a longer batch, and on an older loader, op `0x13` is `Malformed` (`-22`). The SDK
keeps the two records as statics (`cmf_wp2_app::HEART_ON`, `HEART_OFF`): `lv_submit` takes buffers in the app
block only.

### Privacy and battery

Heart-rate values are personal data: apps and tests never log them; the e2e test uses synthetic values. The PPG
LED runs while an app holds the measurement: ask for it only while the value is shown or recorded.

### Screen-off

The measurement keeps the watch out of deep sleep, so with the panel dark the stock "return to watch face" (30 s,
2 min or 30 min, ux2sys `0x9C`) is reached and late-resume would close the app view. The loader of 2026-11-14 holds
that return off while an app runs, as the stock does for its own workout views (`docs/recon.md`, "Return to watch
face"). No ABI change. Since the stop on a dark panel the measurement no longer holds the watch out of deep sleep;
the return stays held anyway.

## GPS (loader of 2026-11-19, no ABI bump)

The watch's own GPS for an app, as the stock outdoor workouts use it. Stock side: `docs/recon.md`, "GPS". Emulator
proof: `tests/test_gps_e2e.py` (app `sdk/examples/gps-probe`, a canvas app) with the emulator's NMEA model and a
synthetic position. Not yet on hardware.

**Loader only.** Same path as heart rate: a SENSOR record and a new record on `lv_submit`, which the table has
since 1.2. `ABI_MINOR` stays 3, the stub does not change (no stub 9, no OTA): install the new loader, then the app.
An older loader answers `-22` (op `0x18` and SENSOR value 2 are unknown there). No package flag: the loader needs
nothing at launch, the app asks at run time and the loader turns it off on every way out.

### The calls

SDK: `Api::gps(&self, on: bool) -> i32`, `Api::gps_fix(&self) -> Option<GpsFix>`, event kind `GPS` = 11.

- `gps(true)`: the loader sets ux2sys `0x63` to **1** (the stock workouts' mode) from its UI tick and queues
  `Event { kind: GPS, x: fix quality, y: satellites }` once per 1000 ms (`wp2_core::gps::EMIT_MS`) while the app runs
  in the foreground, the panel dark included (handed out from `next_event`, as HEART). `x` 0 = no fix yet.
- `gps_fix()`: the latest fix, read from the stock parser's block when called:

  | Field | Unit | Source |
  |---|---|---|
  | `lat_e7`, `lon_e7` | 1e-7 degrees, rounded | GGA, f64 in the stock |
  | `alt_dm` | decimetres above mean sea level | GGA altitude (f32 m) |
  | `ms` | the loader's `millis` at the read | loader |
  | `fix` | GGA fix quality: 0 none, 1 GPS, 2 DGPS | GGA |
  | `sats` | satellites in use | GGA |
  | `ttff_s` | seconds from the start to the first fix, 0 before it | stock |

  **No speed, course or accuracy**: the stock parses only GGA (and GSV SNRs); RMC is dropped, HDOP is read and not
  kept. An app derives speed and course from successive fixes (`gps-probe` does speed). `None` while the app has not
  asked for the GPS, and on an older loader.
- `gps(false)` turns it off and queues one GPS event with `x` 0. Returns 0, or `-22` on an older loader.
- The loader turns the GPS off (mode 0) on exit, crash, force stop, view delete and `PAUSE`, and on again on
  `RESUME`. A new launch starts with it off. One start per hold, one stop at its end (`wp2_core::gps::Hold`, the
  heart rate's rule).
- **A GPS the stock already runs is only read.** The stock keeps no request count for the GPS: one mode, and any
  `0` powers the receiver down (`docs/recon.md`, "GPS", Ownership). If the mode in force (`0x380443F5`) is non-zero
  when the app asks, the loader never writes `0x63` for that hold, so an app cannot cut a stock workout's GPS. In
  the other direction a stock workout cannot start while the app is in the foreground; on `PAUSE` the loader has
  already turned its own GPS off.

### Wire format

- SENSOR record `[0x13, 2, 0, 0, 2, on]` (sensor 2 = GPS; `cmf_wp2_app::GPS_ON`, `GPS_OFF`).
- GPS_FIX record `[0x18, 20, 0, 0, <20 bytes>]`, alone in a batch, in the app block (a `static mut`): the loader
  overwrites the 20 payload bytes with `lat_e7 i32, lon_e7 i32, alt_dm i32, ms u32, fix u8, sats u8, ttff_s u16`
  (LE) and returns 0, or `-1` while the app holds no GPS. The stock block is cleared and refilled at every GGA on
  `gps_service`; the loader copies it until two copies agree (`fw/wp2-loader/src/gps.rs`).

### Privacy and battery

- Positions are personal data: apps and tests never log or send them unasked; the loader never logs them; the e2e
  test uses a synthetic position. `gps-probe` shows them rounded to 0.001° (about 100 m).
- **The stock stores a track point every ~5 s while the GPS has a fix, workout or not** (`record_gps_data`, the
  stock GPS store that `sport_gps_data_sync` hands to the phone). An app's GPS session leaves points there, as a
  stock workout's does. Whether Nothing X shows or uploads points outside a workout is not known: the real-watch
  check below looks for it.
- The receiver is powered while an app holds it: ask only while the position is shown or recorded. The draw was not
  measured; the plan below measures it.

### First real-watch check (plan, not run)

The owner decides when, and runs it. Order: install the loader of 2026-11-19 and `gps-probe` (phone or
`wp2apps install`), nothing else changes on the watch.

1. **Baseline, no app.** Outdoors, open sky. Stock Outdoor run: note the seconds from opening the sport list until
   the GO screen shows a GPS fix. End the workout. Charge to 100 %.
2. **Time to first fix.** Leave the watch indoors ≥ 2 h (cold-ish start), go outdoors, open `gps-probe`, wait for
   `FIX n SAT TTFF s`; note `s` (`ttff_s`). Repeat warm (exit,
   reopen within 1 min) and after 30 min off. Do it once with the phone linked (aiding over BLE,
   `auxiliary_gps_request`) and once with Bluetooth off on the phone.
3. **Fix rate and quality.** Walk 10 min: the event rate (one per second expected), satellites, that the rounded
   position follows the walk, that the speed reads walking pace (4–6 km/h). Coordinates stay on the watch: report
   counts and rates, not positions.
4. **Battery per hour.** From 100 %, `gps-probe` open outdoors with a fix, panel on its own timeout, for 60 min;
   note the percent drop (`wp2apps info` or the watch) and compare with 60 min of the stock Outdoor run (step 1 day,
   same conditions) and with 60 min idle. Expect the app ≈ the stock workout minus its heart rate.
5. **Turned off afterwards.** Exit `gps-probe` with the button, then 30 min idle
   and compare the drop with idle (a receiver left on shows as several percent). Optional read-only check: `AT
   GETGPSSNR` after exit should report 0 satellites.
6. **Stock still gets GPS.** Right after the app's exit: stock Outdoor run, check the GO screen gets a fix and the
   workout records a track (Nothing X after sync). Also the other order: start the stock Outdoor run, go back to the
   face with the workout running, open `gps-probe` (it reads the workout's fix), exit it, and check the workout's track
   has no gap after that moment.
7. **The stock track store.** After step 3, sync with Nothing X and look for a stray track or points at the time of
   the app session outside any workout. If they show up, the API needs a note (or a different mode) before apps use
   it.

Abort criteria: the watch reboots, the GPS stays on after exit (step 5), or the stock workout loses its fix (step 6).

## Pager (loader of 2026-11-14, no ABI bump)

A vertical pager of full-screen pages, the stock `lv_tileview` (entries: `docs/recon-lvgl.md`, `lv_tileview`).
SDK: `stock::pager` (`docs/app-guide.md`, "Pages"). Emulator proof: `tests/test_pager_e2e.py` (ui-gallery's
"Pages"), frames `runs/pager/`.

- `CREATE` class **8 Pager**: `lv_tileview_create(parent)`. Children: class **9 Page** only, and a page only in a
  pager. A page is `lv_tileview_add_tile(pager, 0, row, LV_DIR_TOP | LV_DIR_BOTTOM)` with `row` = the pager's child
  count, so pages stack in creation order. `DEL` of a page is `BadClass` (the rows after it would leave a gap); it
  goes with its pager.
- `VALUE` on a pager: `lv_obj_set_tile_id(pager, 0, v, anim)` (`v` 0..=192, past the last page: the last).
- At `CREATE` the loader registers the crown filter (`0x2D`) on the pager, which makes it the knob object, and its
  own `VALUE_CHANGED` callback. `KNOB(pager)` takes the crown back. A crown event scrolls the current page by its
  step (`lv_obj_scroll_to_y`, no animation) while it has room in that direction (`lv_obj_get_scroll_bottom`,
  `lv_obj_get_scroll_y`); at the end it glides to the next or previous page. A drag does the same through LVGL's
  scroll chain (the tile is scrollable, the tileview snaps with `SCROLL_ONE`).
- Event: when the tileview settles (`VALUE_CHANGED`, sent by its own `SCROLL_END` handler) the app gets
  `Event { kind: LV, x: pager, y: 28, delta: index of lv_tileview_get_tile_act }`, coalesced, with no `EVENTS`
  needed. `EVENTS` `VALUE_CHANGED` on a pager adds nothing.
- The tileview scrolls vertically only, so the back swipe (`0x71`, a right swipe) reaches the loader as before.
- `ABI_MINOR` stays 3 (an OTA would be needed otherwise). An older loader answers `-22` to class 8 and 9: install the
  loader first. The golden fixture is unchanged; `lvcmd::tests::pager_holds_pages` pins the rules.

## Horizontal pager (loader of 2026-11-16, no ABI bump)

The stock workout screen (`sport1_view` `0x64`, built in `0x10079148`) is an `lv_tileview` with three tiles side by
side, `add_tile(tv, 0..2, 0, dir)` with dir 2, 3, 1. SDK: `stock::hpager` (`docs/app-guide.md`, "Horizontal
pager"). Emulator proof: `tests/test_pager_e2e.py` `test_hpager` (ui-gallery `!hpager`).

- `CREATE` class **10 PagerH**: `lv_tileview_create(parent)` and the loader's `VALUE_CHANGED` callback, no crown
  filter (the crown stays with the app: CROWN events as on the root). Its children are class 9 pages only; a page in
  a PagerH is `lv_tileview_add_tile(pager, col, 0, dir)` with `col` = the child count and the stock directions:
  the first `LV_DIR_RIGHT`, the newest `LV_DIR_LEFT`, the one before it gains `LV_DIR_RIGHT` (byte `tile + 0x2C`,
  set by the tile constructor `0x10138078`; the constructor sets the tileview's scroll direction from tile (0, 0)
  only), so three pages get 2, 3, 1 as in the stock (`lvcmd::hpage_dirs`).
- `VALUE`: `lv_obj_set_tile_id(pager, v, 0, anim)`. The page event as for the vertical pager.
- Back swipe: while any PagerH lives the loader marks key `0x71` (the right swipe) handled and queues no `BUTTON`,
  as the stock launcher skips back for the sport views 100..105 (`docs/recon.md`). The crown press stays `BUTTON`
  and keeps the escape. On the first page the right swipe does nothing.
- An older loader answers `-22` to class 10. `lvcmd::tests::hpager` pins the rules.

## Music (loader of 2026-11-16, no ABI bump)

The phone's music, as the stock music view and workout screen control it (ux2sys, getter `0x100E082C(prop, out)`,
setter `0x100E07B8`): `0x31` phone link (u32), `0x4C` media available (u32), `0x4D` playing (u32), `0x4E` command
(set, u32: 1 play, 2 pause, 3 previous, 4 next; the stock sends it only when `0x31` and `0x4C` are non-zero),
`0x4F` title `[0..0x40]` and artist `[0x41..0x81]` (NUL-terminated). Reading the volume (`0x50`) is not offered; volume up and down are (MEDIA_KEY 5, 6).

- **MEDIA_KEY** record, alone in a batch: `[0x16, 1, 0, 0, cmd]`, cmd 1..=4, and since the loader after 2026-11-19 5 volume up
  and 6 volume down (the volume popup's own commands, one phone volume step each). The UI tick checks the link and
  media and sets `0x4E`; the app thread waits for it as for an LVGL batch (with the panel dark it returns once it is
  lit). `0`, `-19` without a phone link or media, `-22` for another cmd and on an older loader (the loaders of
  2026-11-16 to 2026-11-19: `-22` for 5 and 6).
- **MEDIA_BIND** record, alone in a batch: `[0x17, 5, 0, 0, on, title u16, artist u16]`. `on = 1`: from the next UI
  tick, once a second while the app runs in the foreground, the loader reads `0x4D`, `0x31`/`0x4C` and `0x4F`; it
  queues `Event { kind: MEDIA (10), x: playing, y: available }` once at the bind and on every change, and writes
  title and artist into the two labels when the text changed (handle 0 = not shown; a label deleted since is
  skipped). `on = 0` stops it; exit and force stop do too. `-22` for a handle that is not a live label, before
  the app's first LVGL batch, and on an older loader.
- Wire format and the label text cut: `lvcmd::media_key`, `media_bind`, `media_info` and their tests. SDK:
  `Api::media_key`, `Api::media_bind`, `Api::media_unbind`, `MEDIA`, `MEDIA_PLAY`…`MEDIA_NEXT`.
- Emulator: no phone media there, so `test_hpager` checks the bind (one `MEDIA`, 0/0) and `-19`; the label writes
  and a real command need the phone (not proven).

## Music tile (loader of 2026-11-16, no ABI bump)

The stock workout screen's music page (`sport1_view`, id `0x64`, its third tile) as one object: `CREATE` with class
**`Music` (11)**. The loader builds it with the stock tile's own calls, images and fonts (`fw/wp2-loader/src/music.rs`,
`docs/recon.md` "Music tile"), so it is the stock tile pixel for pixel (`tests/test_music_e2e.py`). The page dots
under it belong to the stock screen, not to the tile.

- `CREATE h Music parent`: a 466 × 466 container in `parent` (any object but a pager; a page of `stock::hpager` in
  practice) with "No music" or the track title (sans32) and artist (grey), previous / play-pause / next and volume.
  One at a time (`-22` for a second). `-5` when the scene or its images do not load. `DEL` (of it or an ancestor)
  and exit free its images, text and fonts.
- The loader refreshes it once a second while the app runs in the foreground, as the stock paint (`0x101AF03C`):
  with phone media title and artist, else "No music"; the play button grey without media, else pause while
  playing and play while paused.
- Taps: previous / play-pause / next send `0x4E` (3, 1 or 2, 4) when the phone link and media are up; play-pause
  shows the other icon at once. Every tap also reaches the app: `Event { kind: LV, x: h, y: CLICKED (7), delta }`,
  delta 0 play-pause, 1 previous, 2 next, 3 volume. With phone media volume also opens the stock volume popup
  (`0x100786DC`) over the app (on its root, so a pager does not scroll under it): the volume pill, a slider with the
  phone's volume (`0x50`, refreshed once a second), + and − send `0x4E` 5 / 6, six crown steps one way send one
  (with a buzz while the volume can move), a right swipe closes it. While it is open the right swipe is no BUTTON
  and the crown goes to it; the app gets no event from it. The stock "connect the phone" popup is left out.
- `VALUE h v`: v 1 shows pause, 0 play, until the phone's state next changes. Other values `-22`.
- MEDIA events need `MEDIA_BIND` as before (it can bind no labels: handles 0).
- SDK: `stock::music(&mut b, parent) -> Music { obj }`, `Music::event` (the button), `Music::playing(b, on)`,
  `MUSIC_PLAY`…`MUSIC_VOLUME`. An older loader answers `-22` (class 11 is `BadValue`).
- Emulator: `tests/test_music_e2e.py` compares the stock tile and ui-gallery's `!music` above the page dots, without
  phone media and with a synthetic track (the four ux2sys getters pointed at RAM stubs): 0 pixels differ. The
  volume popup over the whole screen: 0 pixels differ (volume 8 of 16 from a stub getter; the slider's alpha images
  stop the emulator, stock and loader alike, so the test draws them opaque), and +, −, the crown and the closing
  swipe are checked with a recorder on the `0x4E` setter. Real phone media and the keys reaching the phone are not
  proven.

## Escape with own_back (loader of 2026-11-16, no ABI bump)

ABI 1.3 stopped an app that owns back when a `BUTTON` stayed unread for 1000 ms. A live app that is busy longer (a
view rebuild after the first swipe, a 4 KB save) was closed by the next swipe. Now (`events::escape_due`):
an app that owns back is stopped only when it has neither read the `BUTTON` nor called `next_event` for
**5000 ms** (`HANG_MS`); other apps keep the 1000 ms. The escape is armed only when the `BUTTON` was queued (a full
queue drops it; before, that armed a timer for an event the app never got).

## Stub version (stub 2, no ABI bump)

The watch reports `1.0.0.73` (`AT GETVERSION`, `0x8006`) whatever `app.bin` it runs, and the OTA `version_code`
can't be read back. So the stub answers its own release:

- Request: `INFO` (`0x90A0`) with slot `STUB_SLOT` = `0xFE`, under the stock session or `K` like every stub frame.
- Reply (`0xB0A0`): `status` (0) ‖ `STUB_VERSION` (u32 LE) ‖ `LOADER_ABI_MINOR` (u8), 6 bytes. `STUB_VERSION` lives
  in `fw/wp2-core/src/layout.rs`; bump it with every stub change that goes out by OTA. It is not the stub header's
  `version` (the header format, which `wp2apps firmware` requires to be 1).
- An older stub parses `0xFE` as a bad slot and answers `BadRequest` (1): version 1. Stock firmware has no stub and
  doesn't answer at all. One request tells the three apart: no reply, `01`, `00 …`.
- Host: `install.stub` (`wp2apps list` prints it as `stub`).
- Stub 6: `BEGIN` into the loader slot or an app page's slot is no longer `BUSY` while the page is only behind the
  face; the page reloads on its next FOCUS (`docs/app-guide.md`, install notes). Same frames, same statuses.
- Stub 7: `DELETE` of an app page's slot is no longer `BUSY` while the page is only behind the face; the stub drops
  the app's page and widgets from the cards (the stock lays the face out again, which stops the app), then unlinks.
  Every `DELETE` now takes the slot's widgets and page off the cards. `DELETE` of the loader slot stays `BUSY` while
  it is held. Same frames, same statuses (`docs/app-guide.md`, install notes).
- Stub 8: 16 app slots (next section) and `PUSH` (`0x90A8`): the phone sets an app's widget record or start blob
  while it does not run (`docs/widget-push.md`).
- Stub 10 (with the first loader released after `loader-2026-11-22-884e9a6`, no ABI bump): widget templates. v2 `.wgt` records (`text`, `columns`), `PUSH` kind
  `00` takes them (up to 128 B), the card draws `columns` with icons from the package's widget icon trailer, and `END`
  accepts a package with that trailer (`docs/widget-push.md`, `docs/app-icons.md`). The loader's op `0x14` takes
  records up to 128 B and answers `-22` for a v2 record whose template the running stub does not draw
  (`wp2_core::widget::first_stub`: `00`, `01` → stub 10) or that is malformed; `Api::widget_columns` /
  `Api::widget_v2`; the loaders up to `loader-2026-11-22-884e9a6` answer `-22` to every v2 record. Hosts strip the
  trailer for a stub before 10. An icon index below 16 that the package lacks leaves that column without an icon and
  does not cost the card (so a package stored stripped keeps its card over the OTA); 16..254 is malformed. Stub 10 is
  `0x3D38` bytes.

## 16 app slots (stub 8, loader of 2026-11-19, no ABI bump)

- **Which stub takes which slots.** `INFO` (`0x90A0`), `BEGIN` (`0x90A1`), `DELETE` (`0x90A4`) and `MSG` (`0x90A5`)
  accept app slots `0..15` from **stub 8** on. Stub 7 and older take `0..7` and answer `BadRequest` (1) for
  `8..15`. The host reads the stub version first (`INFO` slot `0xFE`, section above): `>= 8` → 16 slots, else 8.
  Listing = `INFO` for `0..count-1` and `0xFF`; a new app goes to the slot of the same name, else the first free one
  below `count` (`install.slots`, `install.install`). Slot numbers stay bytes; `0xFE`/`0xFF` keep their meaning.
- **Files.** `/NAND:/wp2apps/<d>.pkg|.dat|.new|.wgt|.tmp` with `<d>` the slot as one hex digit, upper case:
  `0`..`9`, `A`..`F`. Slots `0..7` keep the names of older stubs, so nothing moves on the OTA.
- **Pages.** View `0xF8 + slot`: `0xF8..0xFF` for slots 0..7 as before, `0x100..0x107` for slots 8..15 (the stock
  view ids end at 237; `wp2apps firmware` adds 17 table entries). Stub 8 with `PUSH` is `0x2F31` bytes
  and stub + table exactly `0x4000`: the stub's flash region went from `0x3000` to `0x5000` and `STUB_MAX` from
  `0x4000` to `0x6000` so the next change has room. Both are budgets of our own (the NOR app partition has ≈ 964 KiB
  free after `app.bin`, `docs/recon.md`).
  The phone's `0x906D` view card carries the id as `u16` LE: `02 00 07 01` is slot 15's page.
- **Widgets.** `0x40` with `param_id = slot + 1`, so `1..16`.
- **Loader.** Reads the stub's release from `StubRam.stub_version` behind `version_magic = "WPSV"` (stub 8 sets both
  at boot; older stubs leave the bytes unset) and lists slots `0..15` on stub 8, `0..7` on stub 7 and older. A loader
  from before 2026-11-19 lists `0..7` on any stub: install the new loader with the OTA, or apps in `8..15` stay
  out of the menu (they still answer `INFO` and can be removed).
- **NAND space.** A package is at most `64 + 2 × 128 KiB` (`BEGIN` limit), in practice image + relocations ≈ the
  image: 16 × 128 KiB = 2 MiB, plus per slot `.dat` 4 KiB (+ `.new` 4 KiB during a save) and a 64 B `.wgt`, each
  at least one 2 KiB cluster, plus the loader (64 KiB) and one `.tmp` during an install: ≈ 2.4 MiB with full-size
  apps, ≈ 4.4 MiB in the theoretical worst case. The emulator's `/NAND:` (Zephyr `statvfs` through the mount API
  `+0x44`, `tests/test_slots16_e2e.py`) is a 9.75 MiB FAT, 2 KiB clusters: 9.69 MiB free with the loader, 9.19 MiB
  after 15 × 33 KiB + 2 KiB apps. That is the emulator's NAND fixture; the real watch was not measured (read-only
  AT has no free-space query known to us), so check `statvfs` there before relying on it.
- **RES heap.** The menu with 15 icons (27.7 KiB each) open: 4.6 MiB of `ResFbHeap` free, lowest ever 1.5 MiB
  (emulator). An icon that does not fit falls back to the letter (`heap::alloc` checks first).
- **Order of updates.** OTA first (stub 8), then the loader; until the OTA the phone sees 8 slots and installs only
  there. Downgrading the stub leaves `8.pkg`..`F.pkg` on NAND unseen: remove them before.
