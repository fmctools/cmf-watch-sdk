# App widgets: templates and the app's own icons

2026-11-21. Repos: `cmf-watch-sdk` (stub, loader, SDK, `wp2apps`), then `fmctools/apps` (Weather adopts it) and
`fmctools/companion` (the phone pushes Weather's card).

## Intent

An app's widget card (`0x40/<slot + 1>`, `fw/wp2-stub/src/card.rs`) is two lines of text today. The stock cards
can do more: widget 97 shows the weather by the hour (`Now 23:00 00:00 01:00`, an icon and a temperature under each),
98 the same by day (frames: `runs/weather-widgets/w97.png`, `w98.png`, not committed). The owner wants **any app**
to be able to build such a card, not a weather-only feature. Weather is the first user: its card shows now and the
next hours, like 97, with its own data and a tap into the app.

## Goals

- A record format with a template id. Two templates: `text` (today's card, unchanged) and `columns` (one to four
  columns, each a caption, an icon and a value, in the layout of stock 97/98).
- Icons come from the app's own package: a small set the app ships next to its menu icon, referenced by index.
- The phone can still set the card while the app does not run (`PUSH`, `docs/widget-push.md`), with the new record.
- Old records, old apps and old loaders keep working; an app can tell when the stub cannot draw a template and fall
  back to `text`.

## Non-goals

- Stock firmware icons (weather, steps, heart…) by id. Possible later through the same icon byte (a high bit), after
  the resources are mapped; not now.
- Live cards: the card is read at layout, as today (`update` stays a no-op). No timers, no animation.
- Other sizes than wide 344×166, more templates than these two, colours per column.
- Feeding the stock weather widgets (97/98) from the phone.
- Phone pushes of `columns` cards for apps other than Weather.

## Anti-goals

- A card that can take the watch face down. The stub validates every record before it draws; anything malformed
  draws the package name, as a missing record does. The face must never fault because of an app.
- A card that does not look stock: same tile, grey, fonts and spacing as 97.
- Unbounded NAND or heap use per card.

## Constraints found

- Stub flash: link region `0x5000`, `wp2apps firmware` budget `STUB_MAX = 0x6000` (can grow; the NOR partition
  has ~964 KiB free). No writable statics in the stub (`link.ld` asserts it); per-card state lives in the card's
  `State` (allocated by the stock card view, size in the `wgt_reg` entry).
- The card is created whenever the face shows (`docs/recon.md`, "Card lifecycle"); create must be cheap: one record
  read plus the icons it names.
- Images: the stub shows pixels through the vendor image object with a `RES` block in the stock resource-cache layout
  (`applist.rs icon_block`, `docs/app-icons.md` "Rendering"): CF 5 (RGB565 + A8, 3 B/px), 64-byte head, PSRAM sync.
- Fonts: the stub opens stock bitmap fonts by path (`/NAND:A/font*.font`, `docs/recon-lvgl.md` "Fonts"). The stock
  LVGL fonts the apps use have no `°` (found in Weather), but stock 97 draws `0°`, so some stock font has it.
- The app writes its record through the loader (`lv_submit` op `0x14`, today exactly 64 B) into
  `/NAND:/wp2apps/<d>.wgt`; `PUSH` kind `00` takes exactly 64 B. Both cap the write rate (NAND wear).
- 16 slots, stub 8 (`docs/abi-1.4.md`). The loader reads the stub's release from `StubRam`.

## Approaches

1. **Templates drawn by the stub** (chosen). The record names a template and carries its fields; the stub lays out
   stock objects. Small stub code, always the stock look, small records the phone can push. Limited to what the
   templates offer; a new template is a new stub (OTA).
2. LVGL command list replayed by the stub. Nearly free-form, but the stub would need the loader's validator and
   interpreter (several KB), records of 0.5–2 KB, a bigger `PUSH`, and a bad batch risks the watch face itself.
3. A bitmap the app renders. Simplest stub, but 28–114 KB per update (NAND wear, slow over BLE), and LVGL apps
   such as Weather cannot render pixels.

Icons: the owner chose **the app's own icons in the package** over the stock set (needs firmware resource recon,
limited to the vendor's drawings) and over both at once (later, through the icon byte).

## Design

### Record (`.wgt`)

- **v1, `text`**: the 64-byte record of today (title 24 B, value 40 B). Unchanged, and still what a record whose
  first byte is not `FF` means.
- **v2**: `FF ‖ template:u8 ‖ fields`, at most **128 bytes**. `FF` never starts UTF-8, so v1 and v2 cannot be
  confused.
  - template `00` `text`: `title[24] ‖ value[40]` as v1 (so the phone can always send v2).
  - template `01` `columns`: `n:u8` (1..=4) then `n × { caption[10] ‖ icon:u8 ‖ value[10] }`, text fields UTF-8,
    NUL-padded, cut at a character boundary. `icon` is an index into the package's widget icons, `FF` none.
    Size: 3 + 4 × 21 = 87 B.
- The stub draws v2 only when it is complete and valid (length, template, `n`, NULs inside each field, icon index
  within the package's set); otherwise it shows the package name. Unknown templates likewise (a newer app on an older
  v2 stub).

### Widget icons in the package

- **Outside the loaded image** (builder check, our call): the loader reads the whole image into `RES` at every start
  and an image is at most `APP_SIZE` = 128 KiB (`layout.rs`); 8 icons of 48² are 55 KB the app would carry in RAM for
  nothing. So the block is a trailer after the relocations: `"WP2W" ‖ version:u8 = 1 ‖ count:u8 (≤ 16) ‖ size:u8 ‖
  0 ‖ count × pixels`, CF 5, square `size × size` (the size of stock 97's icons; the builder measures it in
  `w97.png`, 40 or 48). The package length becomes `HEADER_SIZE + image_size + 4·reloc_count + trailer`; `body_crc`
  covers the trailer; the loader never loads it; an older stub or loader that checks the exact length must be
  checked by the builder (if one refuses a trailer, the trailer needs a header flag the old ones ignore). Icon `i`
  sits at a fixed offset from the package's end of relocations, so the stub reads one icon with one seek.
- `wp2apps pack --widget-icons <dir>` appends it; `pack` refuses a broken block, `info` prints the count.
- Built like the menu icon: `widget-icons/NN-name.svg` next to `Cargo.toml`, rendered by `scripts/build-app` and
  passed to `pack`, indices in file name order. Independent of the menu icon and of `app_features!`.
- The stub reads only the icons the record names (at most 4) into `RES` blocks at create and frees them at delete,
  as `icon_block` does. Missing block, short read or a failed allocation: that column has no icon. No total budget
  across cards (our call): a face holds at most 8 cards × 2 wide tiles; the builder measures free `RES` with the face
  and 16 `columns` cards in the emulator, and if that runs short, the icon size goes down to 40 before anything else.

### The `columns` card

- Same tile as today (344×166, `CARD_BG`). Columns evenly spaced across the tile; per column, top to bottom: caption
  (grey, small stock font), icon, value (white, a stock font that has `°`). Positions and fonts copied from stock 97
  (builder measures from the frame and, where useful, from the 97 widget's create code, `wgt_reg` entry 97).
- Tap: as today, starts the app.

### Who writes it

- **App**: `Api::widget_columns(&[Column])` (and `Api::widget_v2(&[u8])` underneath) writes a v2 record through the
  loader. The loader accepts op `0x14` with 64..=128 B; it returns `-22` for a v2 record whose template the running
  stub does not draw, from a table template → first stub (`00`, `01` → stub 9), so the app falls back to
  `widget(title, value)`, and later templates get the same check. `widget_record` stays.
- **Phone**: `PUSH` kind `00` takes 64..=128 B from stub 9. A host reads the stub version first (it already does for
  16 slots) and sends v1 to an older stub.
- `wp2apps widget set <slot> --columns 'Now:3:-3°' …` for testing from a Mac.

### Versions

- Stub 9 (OTA), loader of the same date. Docs: `abi-1.4.md` (stub 9 row, record, `PUSH`), `widget-push.md`,
  `app-guide.md` "Widget", `app-icons.md` (the `WP2W` block).

### Weather (apps repo, after the SDK pin moves to stub 9)

- `columns` with Now and the next three hours: caption `Now` / `HH:00` (as stock 97), the condition icon, the
  temperature with `°`. Ten widget icons (the owner added the two night ones on 2026-11-22): the eight conditions
  (`model::Icon`), clear at night and partly cloudy at night, drawn in the stock weather style, our own drawings.
  Now takes the night bit of the hourly entry that contains now.
  Fallback to today's text card on an older stub. The pin bump is its own PR (`sdk-update` skill).
- The icon order is a contract between the Weather package and the phone: indices 0..7 = `model::Icon` order (clear,
  partly, cloud, fog, drizzle, rain, snow, storm, as the WMO groups of the weather spec), 8 = clear at night,
  9 = partly cloudy at night; written in the app's README and in the companion's docs.

### The phone keeps the Weather card fresh (companion repo, iOS and Android)

The owner's call: the card must not show hours that have passed, so FMC Companion pushes it.
- It builds the same `columns` record (Now + the next three hours, same captions, icons by the contract above, `°`)
  from the forecast it already fetches for the `weather` RPC (WeatherKit / Open-Meteo, cached).
- It finds Weather's slot by package name in the slot list it already reads (Apps tab), and sends `PUSH` kind `00`
  only to stub 9 or later (it reads the stub version already for 16 slots), never a v1 fallback over the app's own.
- When: on connect and whenever the first column's hour changes, at most once a minute per the stub's limit; the same
  bytes again are free (`00`, no write). iOS runs in its background wakes (~10 s); a missed hour is caught at the next
  wake or connect.
- Watch and phone both write the same file; the last write wins. Both build it from the same kind of forecast, so
  either is right; no arbitration.
- Only for the Weather package for now, behind the Apps tab's existing weather permission for that app.

## As built (2026-11-21, deviations)

- **Trailer and `body_crc`.** `body_crc` does *not* cover the widget icon trailer: an old loader checks `body_crc`
  over image + relocations and would refuse the app, and a new one would read every icon at each start. The
  trailer carries its own crc32: `"WP2W" ‖ 1 ‖ count ‖ 40 ‖ 0 ‖ crc32:u32 ‖ pixels` (12-byte head). Stub 9 checks
  it at `END`; a bad one has its magic zeroed and the app still installs and runs, without icons. The card checks
  the head before reading an icon.
- **Old stubs.** A stub before 9 refuses any package longer than its header says (`Invalid` at `END`); a header flag
  cannot help (reserved bytes must be 0, the size check stays). So hosts strip the trailer for stub < 9
  (`package.strip_trailer`, `wp2apps install`; the companion later): the result is byte for byte the package without
  icons. Old loaders need nothing.
- **Record length.** `columns` with one or two columns is 24 or 45 bytes, so op `0x14` and `PUSH` kind `00` take any
  length up to 128 and the record's own checks decide (v1 stays exactly 64).
- **Look.** Captions and values are both white, as in 97 (the stock code sets both to white; the frame reads it
  grey). `°` is drawn as `º` (U+00BA): the stock fonts have only that one, and 97 uses it.
- **Free RES.** Measured with 16 tiles: no shortage, the icons stay 40 px.

## Left to the builder

- The exact geometry and fonts of 97 (from the frame and the stock code), the icon size (40 or 48), which stock font
  has `°`.
- How the stub reads one icon from a package by index without reading the whole `.wp2info` (offsets from the header).
- Tests: record validation on the host (`wp2-core`), the `WP2W` block in `wp2apps` tests, an e2e test that puts a
  `columns` card on the face in the emulator and checks the frame, plus every malformed record showing the name and no
  fault.
