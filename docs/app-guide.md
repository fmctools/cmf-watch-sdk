# Writing apps for the watch

How to build an app for the cmf-watch-sdk loader (ABI 1 / 1.1 / 1.2) on the CMF Watch Pro 2, JX402 `1.0.0.73`. Read this
before anything else. You should not need `docs/recon*.md`, the plans or agent reports to write an app; file paths
below point at the code that backs each fact.

An app is a `no_std` Rust binary linked at address 0, packed into a `.wp2app` and installed over BLE into one of 8
slots. The loader copies it into PSRAM, relocates it and runs `main(api)` in its own thread. The app draws into a
466×466 RGB565 framebuffer, waits for events and talks to the hardware only through the `Api` table
(`sdk/cmf-wp2-app/src/lib.rs`). The loader, `vibration-lab`, `agent-glow`, `api-demo` and `herdr-agents` have all run
on a real watch. Every install onto a physical watch needs the owner's approval (AGENTS.md, hard rule 1).

## 1. Quick start

To publish an app to the FMC store, write it in [cmf-tech-community/apps](https://github.com/cmf-tech-community/apps)
instead: it pins this SDK and publishes what's merged there. The steps below build an app inside this repository, as
one of the SDK's examples.

### Create the crate

1. Copy `sdk/examples/vibration-lab` to `sdk/examples/<name>` (it has the host-test layout and an `icon.svg`).
   Replace `src/main.rs` (start from the app below) and delete `src/lab.rs`.
2. In `sdk/examples/<name>/Cargo.toml`, replace `vibration-lab` with `<name>` (package and `[[bin]]`). Leave out
   `test = false`, or `cargo test` skips the crate.
3. Add `"examples/<name>"` to `members` in `sdk/Cargo.toml`.
4. Edit `icon.svg` (96×96 viewBox). Style of the bundled icons: a gradient circle filling the 96 px, a white glyph
   about 40 px tall (`docs/app-icons.md`). No icon: delete `icon.svg` and the `app_icon!()` line; the menu then
   shows the app's initial in a grey circle. `app_icon!()` without `icon.svg` fails to compile
   (`WP2_ICON` not defined).

### A minimal app

`blink`: a coloured square, the crown changes its colour, a tap counts and ticks the motor, the button exits,
a stock popup pauses it. This exact code was built with `scripts/build-app`, its tests pass, and it was run in the
emulator (colour, crown, a 30 ms motor tick, pause/resume under the charger popup, exit).

`src/main.rs`:

```rust
// wp2_sim: the same device code runs in the host simulator (section 6)
#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod logic;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod device {
    use crate::logic::{label, turn, COLOURS};
    use cmf_wp2_app::{Api, BUTTON, CROWN, PAUSE, RESUME, TOUCH_DOWN};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!(); // needs icon.svg next to Cargo.toml

    const HZ: u16 = 31; // the stock motor PWM frequency
    const TICK_MS: u32 = 25;

    fn draw(api: &Api, colour: usize, taps: u32) {
        api.rect(0, 0, api.width, api.height, 0x0000);
        api.rect(133, 113, 200, 200, COLOURS[colour]);
        let mut buf = [0u8; 8];
        let text = label(taps, &mut buf);
        api.text(233 - 8 * text.len() as i16, 340, text, 0xFFFF);
        api.show();
    }

    fn main(api: &'static Api) -> i32 {
        let (mut colour, mut taps) = (0, 0);
        let mut tick_off: Option<u32> = None; // when the tap tick must stop
        draw(api, colour, taps);
        loop {
            let wait = tick_off.map_or(u32::MAX, |t| (t.wrapping_sub(api.now()) as i32).max(0) as u32);
            let Some(e) = api.event(wait) else {
                api.vibrate(0, HZ); // timed out: the tick is over
                tick_off = None;
                continue;
            };
            match e.kind {
                BUTTON => break,
                PAUSE => {
                    api.vibrate(0, HZ);
                    tick_off = None;
                    loop {
                        match api.event(u32::MAX).map(|e| e.kind) {
                            Some(RESUME) => break,
                            Some(BUTTON) => return 0,
                            _ => {}
                        }
                    }
                }
                CROWN => colour = turn(colour, e.delta),
                TOUCH_DOWN => {
                    taps += 1;
                    if api.vibrate(400, HZ) == 0 {
                        tick_off = Some(api.now().wrapping_add(TICK_MS));
                    } else {
                        api.vibrate(0, HZ); // -4: clear the lock, skip this tick
                    }
                }
                _ => continue,
            }
            draw(api, colour, taps);
        }
        api.vibrate(0, HZ);
        0
    }
}
```

`src/logic.rs`, the pure part that `cargo test` runs on the Mac:

```rust
// Pure logic: no Api, so `cargo test` runs it on the Mac.

/// Orange, cyan, magenta, green (RGB565).
pub const COLOURS: [u16; 4] = [0xFD20, 0x07FF, 0xF81F, 0x07E0];

/// Colour index after one crown detent (`delta` is +1 or -1).
pub fn turn(i: usize, delta: i16) -> usize {
    (i as i16 + delta).rem_euclid(COLOURS.len() as i16) as usize
}

/// "taps N", N capped at 999, without a heap.
pub fn label(taps: u32, buf: &mut [u8; 8]) -> &str {
    buf[..5].copy_from_slice(b"taps ");
    let n = taps.min(999);
    let digits = if n >= 100 { 3 } else if n >= 10 { 2 } else { 1 };
    for i in 0..digits {
        buf[4 + digits - i] = b'0' + (n / 10u32.pow(i as u32) % 10) as u8;
    }
    core::str::from_utf8(&buf[..5 + digits]).unwrap_or("?")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crown_wraps_both_ways() {
        assert_eq!(turn(0, -1), 3);
        assert_eq!(turn(3, 1), 0);
    }

    #[test]
    fn label_digits() {
        let mut b = [0; 8];
        assert_eq!(label(0, &mut b), "taps 0");
        assert_eq!(label(42, &mut b), "taps 42");
        assert_eq!(label(5000, &mut b), "taps 999");
    }
}
```

The two-module layout is how `vibration-lab`, `agent-glow` and `herdr-agents` are built: everything that needs
`Api` lives in `mod device` (compiled only for `target_os = "none"`), and the host build gets an empty `main` so
`cargo test` can link. A crate that needs no host tests can be a plain `#![no_std] #![no_main]` file with
`test = false` in `Cargo.toml` (`sdk/examples/hello`, and `line` in section 4).

### Build and test

```bash
(cd sdk && cargo test -p <name> --target aarch64-apple-darwin)   # host unit tests
scripts/sim <name>                                               # run it in a window on the Mac (section 6)
nix-shell -p llvm --run 'scripts/build-app <name> <version>'     # -> target/<name>.wp2app
uv run --locked wp2apps info target/<name>.wp2app                # name, version, abi_minor, image_size, icon
```

`scripts/build-app` renders `icon.svg` with `resvg` (via `nix-shell -p resvg` if missing), builds the release ELF
for `thumbv7em-none-eabi` (`sdk/.cargo/config.toml` sets the target, `app.ld` and `--emit-relocs`), converts it with
`llvm-objcopy` and packs it with `wp2apps pack`. A bare `cargo build` of an app with `app_icon!()` fails without
`WP2_ICON`; always build through the script. The `+no-movt` "unstable feature" warning is expected.
`<version>` is a `u32` shown in the loader menu as `v<version>`; nothing checks it, bump it so you can tell
installs apart.

### Install and run

Emulator (boot it as in README "Running in the emulator": `scripts/build-fw`, `wp2apps nor`, `scripts/emu` with
`--hci-port P`, `netsimd` on the same port):

```bash
uv run --locked wp2apps install target/loader.wp2app --hci-port P        # once per loader build
uv run --locked wp2apps install target/<name>.wp2app --hci-port P        # -> {"installed": "0"}
uv run --locked wp2apps list --hci-port P
```

Real watch, only after the owner says yes:

```bash
uv run --locked wp2apps scan --show-address          # macOS gives a CoreBluetooth UUID, not a MAC
uv run --locked wp2apps install target/<name>.wp2app --address <UUID>
```

- `install` without `--slot` reuses the slot of a package with the same name (an update, keeps its stored data),
  else the first free one. Installing into the slot of the running app, or the loader while its menu is open, is
  refused with `BUSY` (`fw/wp2-stub/src/ble.rs`, `busy`). Close the app first.
- An app page behind the face (section "Page") holds the loader too, but from stub 6 on that is no `BUSY`: the loader
  and the page's app take the install while the page is not in view, keep running the old code, and the page loads
  the new loader and starts the new app the next time it comes into view (`behind_face`, `fw/wp2-stub/src/view.rs`,
  `tests/test_page_install_e2e.py`). Nothing to remove first, no host change. A page in view is a foreground app:
  `BUSY` as before. Stub 5 answers `BUSY` to both.
- From stub 7 on `remove` of a page's app behind the face is no `BUSY` either: the stub takes the app's page and its
  widgets (`0x40` / slot + 1) off the cards, as "Remove page" / "Remove widget" on the watch, then deletes the package
  and its data. Storing the cards lays the face out again at once (as the phone's `0x906D` set does), which deletes
  the page view and stops its app. A widget goes with every `remove`, running app or not (`forget`,
  `fw/wp2-stub/src/ble.rs`, `tests/test_page_delete_e2e.py`). A page in view, the menu or an app in the foreground
  stay `BUSY`, and so does `remove` of the loader while it is held, a page behind the face included (the page would
  stay on the cards without a loader). Stub 6 still answers `BUSY` to a page's app.
- `install`, `list` and `remove` do a full bind, and the stock then shows a "Paired" popup. Close it with the
  button. If an app was running, closing that popup ends it (section 4).
- Open an app: stock app list (list style, not waterfall) → "Custom Apps", the first row → tap the app's row (the
  crown scrolls the list). The row shows the name and `vN`. Rows come in the order the phone set, then the other
  apps in slot order (`docs/app-order.md`). 16 slots from stub 8, 8 before it (`docs/abi-1.4.md`, "16 app
  slots"). If `main` returns a non-zero code, the row shows `Error <code>` for 2 s (`fw/wp2-loader/src/main.rs`, `fail`).
- Remove on the watch: a long press on the app's row asks "Delete" / "Cancel" with its name; Delete removes the
  package and its data (as `wp2apps remove`) and the list is built again (loader of 2026-11-14,
  `tests/test_menu_delete_e2e.py`).
- Leave, as in stock apps: in an app, the crown press or a right swipe sends `BUTTON` and returns to the "Custom
  Apps" menu (the app gets 1 s, then it is stopped). In the menu a right swipe goes back to the stock app list at
  the same scroll position and the crown press goes to the clock (`docs/recon.md`, "Back navigation").

## 2. Platform facts

### Screen

| | |
|---|---|
| Panel | 466×466 round AMOLED. `api.width`/`api.height` are 466. Origin top left, centre (233, 233), visible disc radius 233 |
| Pixels | RGB565, `0xRRRR_RGGG_GGGB_BBBB` as `u16` (`LV_COLOR_16_SWAP 0`, `fw/wp2-loader/src/ui.rs`) |
| Black | `0x0000` is a pixel that is off: use black backgrounds and light only what matters (AMOLED power) |
| Framebuffer | one buffer, 434 312 B in PSRAM, owned by the loader; it starts black at each launch |
| Font | built-in 8×8 at 2×: every char is a 16×16 cell, no other sizes |

**Safe area.** Anything outside the circle is cut. For a band from `y0` to `y1` the visible half-width is
`isqrt(233² − d²)` with `d` the larger of `|y0 − 233|` and `|y1 − 233|`; keep 6–8 px of margin inside it.
`row_geometry` in `sdk/examples/herdr-agents/src/model.rs` does this (with a test that every row fits).
Centred 16 px text lines, 6 px margin:

| line top `y` | 40 | 60 | 80 | 100 | 120 | 160 | 200–217 |
|---|---|---|---|---|---|---|---|
| chars that fit | 15 | 18 | 21 | 23 | 24 | 26 | 28 |

The same table holds mirrored below the centre (line bottom at `466 − y`). A centred line of `n` ASCII/Cyrillic
chars starts at `x = 233 − 8·n`. Touch rows in the examples are 50–58 px tall.

**Colours that read well** (from the examples): white `0xFFFF` text; dim grey `0x8410` or `0x8C71` for secondary
text; `0x39E7` or `0x2124` for a selected-row background; orange accent `0xFD20` (vibration-lab); cyan `0x07FF`
(api-demo); status colours red (255, 80, 40), green (60, 220, 110), blue (60, 140, 255), grey (110, 110, 110)
(herdr-agents); palette `#22D3EE`, `#3B82F6`, `#8B5CF6`, `#E84CD8` on black (agent-glow). To convert:

```rust
fn rgb(r: u32, g: u32, b: u32) -> u16 {
    ((r >> 3) << 11 | (g >> 2) << 5 | b >> 3) as u16
}
```

### Limits

| | Value | Source |
|---|---|---|
| Image | ≤ 128 KiB (`APP_SIZE = 0x20000`): icon + code + rodata + data **+ bss** | `fw/wp2-core/src/layout.rs`, `sdk/cmf-wp2-app/app.ld`, `wp2apps pack` refuses more |
| Icon | 27 664 B of the image (16 B descriptor + 96·96·3) | `docs/app-icons.md` |
| Statics | `.bss` is written into the image as zeros (`llvm-objcopy --set-section-flags .data=alloc,load,contents`), so a `static mut [u8; 8192]` adds 8 KiB to the package and the install. They start fresh on every launch: the image is reloaded from NAND | `scripts/build-app`, `fw/wp2-core/src/load.rs` |
| Bitmaps | pixels of `bitmaps/*.png` live in the image like any static: keep **all bitmap pixels ≤ 48 KiB** and every side ≤ 466 px so code + data + the 27 664 B menu icon still fit the 131 072 B package; a full-screen image (434 312 B) can never be baked | `docs/bitmap-assets.md`, `wp2apps.bitmaps` |
| Stack | 8 KiB (`0x2000`), no guard: an overflow corrupts the thread struct. Put buffers over ~1 KiB in a `static mut` (api-demo `SCRATCH`, herdr-agents `STORE`) | `fw/wp2-loader/src/app.rs`, `ThreadBuf` |
| Heap | none: no allocator, no `alloc`. Fixed arrays only | |
| Floats | compile to soft-float library calls (target is `eabi`, not `eabihf`): an `f32` multiply linked `__aeabi_fmul` and grew `.text` by ~600 B. No shipped app uses floats on the device; use integers and fixed point (`sin`/`cos` in Q14 in `agent-glow/src/glow.rs`) | build check in this guide's scratch |
| Formatting | `core::fmt::Write` into a fixed buffer works (`Line` in `vibration-lab/src/main.rs`, `Buf` in `api-demo/src/main.rs`); it costs code size. herdr-agents formats digits by hand | |
| Thread | one Zephyr thread at priority 14, the lowest preemptible (idle is 15): stock BLE, UI and sensors preempt the app, so a busy app does not freeze the watch, but it burns battery and starves `standby` | `fw/wp2-loader/src/app.rs`, `APP_PRIO` |
| Panic | the SDK handler turns the motor off and exits with `-1` (menu: `Error -1`). A CPU fault is not caught | `sdk/cmf-wp2-app/src/lib.rs` |
| ABI | an app built with this SDK needs a 1.1 loader; on a 1.0 loader `main` never runs and the menu shows `Error -2` | `__start` in `lib.rs` |

### What is slow

- `rect` and `text` write straight into the PSRAM framebuffer; `show` copies nothing. It publishes the bounding box
  of every `rect`/`text` since the last `show`. Every 10 ms the loader's UI tick writes back **whole rows** `y0..=y1`
  of that box from the PSRAM cache (full 466 px width) and has LVGL redraw the box to the panel
  (`present` in `fw/wp2-loader/src/app.rs`, `flush` in `fw/wp2-loader/src/ui.rs`). Several `show` calls between
  two ticks merge into one box.
- So the cost scales with the rows the union box spans. A full-screen clear is a 434 KB write-back and a full
  LVGL redraw. Two small changes at the top and bottom of the screen cost about as much as a full redraw. Redraw
  only what changed and keep animated parts compact (agent-glow repaints rings in place; herdr-agents repaints only
  the pulsing dots between full redraws).
- Measured in the emulator (not hardware; `tests/test_demo_e2e.py` prints it): agent-glow with dirty redraws
  flushes ~255 KB per frame (58 % of full) and draws a frame in 16–25 ms of its 33 ms. On the watch it looks nearly
  smooth. There are no hardware frame timings yet.
- There is no double buffering. LVGL can read the buffer while the app draws, so draw a frame quickly and `show`
  right after.

## 3. API reference

`fn main(api: &'static Api) -> i32` is registered with `cmf_wp2_app::entry!(main)`. Returning ends the app;
`0` is a normal exit, anything else shows `Error <code>` in the menu. `(api.exit)(code)` does the same from deep
inside the app (there is no wrapper). `cmf_wp2_app::api()` returns `Some(&Api)` from anywhere once the app started.

### Drawing

| Call | Notes |
|---|---|
| `rect(x: i16, y: i16, w: u16, h: u16, rgb565: u16)` | Fills a rectangle, clipped to the screen. The only drawing primitive: discs, rings and AA edges are built from 1 px rows (see section 5) |
| `text(x: i16, y: i16, text: &str, rgb565: u16)` | Top-left of the first 16×16 cell at (x, y). Draws only the glyph pixels (transparent background): clear the area first with `rect`. ASCII and Cyrillic `А–я`, `Ё`, `ё` have glyphs; any other char and every invalid UTF-8 byte draws one `?` cell (`fw/wp2-core/src/gfx.rs`). **At most 64 bytes per call** (`TEXT_MAX`, `fw/wp2-loader/src/app.rs`): longer text is cut silently, which is 32 Cyrillic chars. No wrapping, no width query: width = 16 × chars |
| `pixels(px: &[u16], x: i16, y: i16, w: u16, h: u16)` | Draws `w * h` row-major RGB565 pixels at (x, y), clipped like `rect`; `0`, or `-1` for a side outside 1..=466 or a short `px` (docs/bitmap-assets.md). One copy on a loader with bitmaps; on an older loader or the simulator the same picture falls back to spans of `rect` |
| `show()` | Publishes what was drawn since the last `show` (see "What is slow"). Without it nothing is guaranteed to reach the panel. While paused the box stays pending and the loader repaints everything on resume |

### Bitmap assets

Two ways to show an image, both documented with formats, limits and the wire protocol in
[docs/bitmap-assets.md](bitmap-assets.md):

- **Baked into the package** — put PNGs in `bitmaps/` next to `Cargo.toml`, call
  `cmf_wp2_app::bitmaps!()`, then draw `bitmaps::LOGO` with `api.pixels(&LOGO.px, x, y, LOGO.w,
  LOGO.h)` (canvas apps) or `Batch::image` + `Batch::set_image` (LVGL apps). `scripts/build-app`
  regenerates `src/bitmaps.gen.rs` (also run it by hand: `python -m wp2apps.bitmaps <crate>`).
  Budget: all asset pixels together ≤ 48 KiB, every side 1..=466 — see the Limits table above.
- **Streamed from a phone** — a host sends `START/DATA/DONE` as ordinary host messages; the app
  turns each chunk into `pixels()` spans as it arrives (receiver pattern:
  `sdk/examples/bitmap-stream`; senders: `python -m wp2apps.stream` and the Android
  `photo-stream-companion`). Limits and the temporary-protocol caveats: bitmap-assets.md,
  "Streaming images from the phone".

Both draw from memory the app owns: pixels stay in the relocated package image, are never copied
into loader storage, and the loader range-checks every pointer against the app's own image before
drawing.

### Waiting and events

`event(timeout_ms: u32) -> Option<Event>` is the only way to wait or sleep. `u32::MAX` waits forever, `0` polls,
`None` means the timeout ran out. It checks the queue, then the host inbox, then sleeps 5 ms, so timeouts and input
have ~5 ms granularity (`next_event` in `fw/wp2-loader/src/app.rs`). Never spin without calling it.

`Event { kind: u8, x: i16, y: i16, delta: i16 }`:

| `kind` | When | Fields, notes |
|---|---|---|
| `TOUCH_DOWN` (1) | finger down | `x`, `y` in screen pixels. Act on this one |
| `TOUCH_UP` (2) | finger up | `x`, `y` are not reliable: in the emulator a tap at y 199 reported a row-0 `y` (herdr-agents now uses DOWN). There are no move events, so no drags or swipes inside the app |
| `CROWN` (3) | one crown detent | `delta` is `+1` or `-1`, never a magnitude; a fast turn is several events |
| `BUTTON` (4) | crown press, or a right swipe (the stock "back") | The loader force-stops the app **1000 ms** later if it is still running (`ESCAPE_MS`, `wp2_core::events::escape_due`). Turn the motor off, save if needed, return. ABI 1.3: after `own_back(true)` it is an ordinary event and the app exits by returning; a hung app (no `BUTTON` read and no `next_event` call for 5000 ms, loader of 2026-11-16) still is force-stopped (`docs/abi-1.3.md`, `docs/abi-1.4.md`). While a horizontal pager lives the right swipe is no `BUTTON` |
| `PAUSE` (5) | a stock popup (charger, incoming call, "Paired"; notifications use the call's popup type, untested) or another view covers the app | The queue is cleared, input is dropped, `vibrate` returns `-3` and the motor is cut, `show` does not reach the panel, host messages are refused with `PAUSED`. Stop the motor and animations, then block in `event(u32::MAX)` until `RESUME` (handle `BUTTON` there too, as `paused()` in agent-glow and herdr-agents does) |
| `RESUME` (6) | the popup closed | The loader repaints the whole framebuffer itself. Redraw time-dependent parts; ask the host for a fresh snapshot |
| `MESSAGE` (7) | ABI 1.1: a host message is in the inbox | `x` = its length. A hint: drain with `recv` until `None` |
| `HEART` (9) | about once a second after `heart_rate(true)` (loader of 2026-11-14) | `delta` = bpm, 0 = no reading yet or off the wrist; `x` = 1 on the wrist. Personal data: never log it |
| `GPS` (11) | about once a second after `gps(true)` (loader of 2026-11-19) | `x` = fix quality (0 = no fix yet), `y` = satellites; the position comes from `gps_fix()`. Personal data: never log it |

- The queue holds 15 events; more are dropped (`fw/wp2-core/src/events.rs`).
- **An app can end without any event.** Closing the "Paired" popup jumps to the clock and deletes the view. All of
  these abort the thread: no `BUTTON`, no return. Save state when it changes, not only at exit.
- **Screen-off does not end the app** (`docs/recon.md`, "Screen-off and the app view"): the stock keeps the view
  stack across auto-sleep, drops the key press that wakes the panel, and the app is shown again; an AOD overlay
  pauses it (`PAUSE`/`RESUME`). Proven in the emulator for light sleep (S1, `tests/test_back_e2e.py`
  `ScreenOffTest`); deep sleep (S2/S3BT) and AOD are not modelled there and are unverified on hardware.
- **The press that ends AOD** reaches the view after the launcher's late-resume (`0x83`): the loader drops a key that
  comes while asleep (`0x82`) or within 1.5 s of `0x83` (500 ms let it through on hardware). An app on an older loader drops a `BUTTON` within 500 ms of
  `RESUME` itself. The emulator does not model AOD: seen on hardware only.

### Time

| Call | Returns | Notes |
|---|---|---|
| `now() -> u32` | ms since boot | Wraps after ~49.7 days: compare with `(a.wrapping_sub(b) as i32) >= 0`, as every example does |
| `time() -> Option<Time>` (1.1) | local wall-clock time as the watch shows it | `None` if the RTC is unavailable. `Time { year, month 1–12, day, hour, minute, second, weekday (0 = Sunday), millis, utc_offset_min }`; `utc_offset_min` is `i16::MIN` when the phone never synced a zone |

### Motor

`vibrate(duty_permille: u16, freq_hz: u16) -> i32`, `duty_permille` `0..=1000` (0 = off), `freq_hz` `1..=20000`.
31 Hz is the stock PWM frequency and what the examples use; `vibrate(0, 0)` is a valid "off".

| Return | Meaning |
|---|---|
| `0` | applied |
| `-1` | out of range |
| `-3` | paused (between `PAUSE` and `RESUME`); the motor stays off |
| `-4` | locked: the loader cut a run longer than 2000 ms; every `vibrate(duty > 0)` returns `-4` until `vibrate(0, _)` |
| other negative | PWM driver error |

- The 2000 ms cap counts from the off→on switch; calling `vibrate` again while on does not restart it
  (`fw/wp2-core/src/pwm.rs`). Longer feedback = pulses with gaps.
- The loader turns the motor off on exit, force stop, crash and view delete, and cuts it on pause. Your app still
  turns it off on `BUTTON` and `PAUSE`, because its own pattern state must stop too.
- A tick: start, then stop from the event loop when its deadline passes (`blink` above, `agent-glow` main.rs). Do
  not `event(20)` to wait for it inside a handler: that call can swallow a `BUTTON`.
- Patterns used today: tick 250‰ for 20 ms (agent-glow); 400‰ for 25 ms (blink); blocked = 3 × 110 ms at 1000‰ with
  90 ms gaps, done = 220 ms at 650‰ (herdr-agents `BLOCKED_PATTERN`, `DONE_PATTERN`). The emulator only logs duty;
  how it feels is checked on the watch.

### Storage (1.1)

One blob of at most `STORE_MAX = 4096` bytes per slot, in `/NAND:/wp2apps/<slot>.dat`.

| Call | Returns |
|---|---|
| `load(buf: &mut [u8]) -> i32` | stored length (`0` = nothing stored), `-1` `buf` too short, `-5` I/O error or both copies corrupt |
| `save(data: &[u8]) -> i32` | `0`, `-1` longer than 4096, `-5` I/O error. Empty `data` deletes the blob |

- Atomic: `save` writes `N.new`, unlinks `N.dat`, renames; `load` falls back to `N.new` and checks a CRC
  (`fw/wp2-loader/src/svc.rs`). A cut write never gives you half a blob.
- It writes NAND synchronously in the app thread: save on change or at exit, never per frame.
- The blob survives an update (install of the same name) and is deleted by `wp2apps remove` or by installing another
  app into the slot.
- Put a magic/version in front (herdr-agents saves `"H1"`) and treat a blob you cannot parse as empty.
- A 4 KiB buffer belongs in a `static mut`, not on the 8 KiB stack.

### System (1.1)

| Call | Returns | Notes |
|---|---|---|
| `battery() -> Option<Battery>` | `percent` (the stock estimate), `charging`, `full`, `charger` | `None` on a driver error |
| `own_back(on: bool)` (1.3) | – | The app handles `BUTTON` itself ("back", an exit dialog); see `BUTTON` above and `docs/abi-1.3.md`. No-op on an older loader |
| `media_key(cmd: u8) -> i32` | `-22` | Phone music: `MEDIA_PLAY`, `MEDIA_PAUSE`, `MEDIA_PREV`, `MEDIA_NEXT`. `0`, `-19` with no phone link or media (`docs/abi-1.4.md`, "Music") |
| `media_bind(title: Option<lv::Obj>, artist: Option<lv::Obj>) -> i32` | `-22` | The loader writes the phone's track into these labels once a second and sends `MEDIA` (`x` playing, `y` available) at the bind and on change; `media_unbind()` stops it. Unbind before deleting the labels |
| `heart_rate(on: bool) -> i32` | `0`, `-22` on an older loader | Starts or stops the stock heart-rate measurement for `HEART` events. Held only while the app runs and is not paused; stopped on pause, exit, crash, force stop and view delete; started again on resume. Costs battery (the PPG LED). Works in canvas and LVGL apps; `docs/abi-1.4.md`, "Heart rate" |
| `gps(on: bool) -> i32` | `0`, `-22` on an older loader | Turns the watch's GPS on or off for `GPS` events, with the same lifecycle as `heart_rate` (off on pause, exit, crash, force stop; a GPS a stock workout runs is only read). Costs battery; the stock also stores its own track points while it has a fix; `docs/abi-1.4.md`, "GPS" |
| `gps_fix() -> Option<GpsFix>` | the latest fix, `None` without `gps(true)` or on an older loader | `lat_e7`, `lon_e7` (1e-7°), `alt_dm`, `fix`, `sats`, `ttff_s`, `ms`. No speed or course: derive them from successive fixes (`sdk/examples/gps-probe`) |
| `keep_screen_on(on: bool)` | – | Records the wish; the next UI tick takes or drops the stock wake lock. Held only while the app runs and is not paused; dropped on pause, exit, crash, force stop and view delete; taken again on resume. Costs display power: hold it with a deadline (herdr-agents: 20 s after a buzz) |

### Widget (card left of the watch face)

An app can have a widget on the cards left of the watch face: a wide (344×166) stock-looking tile with a grey title
and a white value, on the stock card grey in the system font. The app only stores the text; the stub draws it.

| Call | Returns |
|---|---|
| `widget(title: &str, value: &str) -> i32` | `0`, `-5` I/O error, `-22` on a loader without widgets (before 2026-11-15) |

- `title`: one line, up to 23 bytes. `value`: up to 39 bytes, `\n` starts a second line. Longer text is cut at a
  char boundary; a line wider than the tile ends in `…`.
- Stored in `/NAND:/wp2apps/<slot>.wgt` (64 bytes) until the next call or the app's removal. Each call writes NAND
  synchronously: call it when the value changes or at exit, never per frame.
- The card reads the record when it is laid out, and that happens every time the watch face shows. The app runs only
  under the loader view, so its update is on the card the next time the user goes back to the face and swipes left.
- From stub 8 the phone can also set the record while the app does not run (`PUSH`, at most once a minute), and a
  small blob the app gets as its first host message at start: `docs/widget-push.md`.
- Before the first call the tile shows the package name; an empty slot shows "Apps".
- Declare it: `cmf_wp2_app::app_features!(cmf_wp2_app::WIDGET);` next to `entry!`. The user then adds it on the
  watch: a long press on the app's row in Apps → "Add widget" (again: "Remove widget"). It goes next to other app
  widgets, two per card, or onto a new card at the end; the stock cards stay as they are.
- On one card it also mixes with stock widgets (from the host or, later, FMC): a card has two rows in list order, a wide
  widget takes a row, two small ones (class 1 + class 0) share one; a third row is not shown.
- From the host it is widget `0x40` with param `slot + 1` (`wp2apps list` shows the slot):
  `wp2apps cards set widgets:0x40/1,0x40/2 widgets:0x40/3 --hci-port P` (replaces the whole list). Param `0` is the
  "Apps" tile that opens the menu.
- A tap on the tile starts the app through the loader, as a tap on its menu row does; leaving the app goes to the menu.
- Examples: `hello` (colour), `vibration-lab` (the last preset), `hr-probe` (the last reading), `herdr-agents`
  (agent counts); `tests/test_app_widgets_e2e.py`.

```rust
api.widget("Vibration Lab", "Strength 35%\nPattern Tick");
```

#### Columns card (stub 10)

From stub 10 (and the first loader released after `loader-2026-11-22-884e9a6`) the tile can also be a `columns` card in the layout of the stock
weather-by-hour widget: one to four columns, each a caption, one of the app's own icons and a value.

| Call | Returns |
|---|---|
| `widget_columns(&[Column]) -> i32` | `0`, `-5` I/O error, `-22` when the watch cannot draw it (stub before 10, older loader), for 0 or more than 4 columns or an icon over 15 |
| `widget_v2(rec: &[u8]) -> i32` | the raw v2 record (`docs/widget-push.md`), same codes |

- `Column { caption, icon: Option<u8>, value }`: caption and value up to 9 bytes each, cut at a char boundary. Write
  `°` as usual: the stock fonts have only `º` (U+00BA), and the stub draws `°` as that, as the stock widget does.
- Icons: 40×40 SVGs in `widget-icons/` next to `Cargo.toml` (`00-sun.svg`, `01-cloud.svg`, …); the index is the
  position in file name order. `scripts/build-app` renders them into the package's widget icon trailer
  (`docs/app-icons.md`). An index (0..15) the package does not have leaves that column without an icon; the card
  still shows caption and value, so keep them readable alone.
- On `-22`, fall back to `widget(title, value)`. **Write `º` (U+00BA) there, not `°`:** the text card is drawn as
  is, on every stub, and its font has no `°` (an emulator frame on stub 8 showed `21°` as `21`):

```rust
let rc = api.widget_columns(&[
    Column { caption: "Now", icon: Some(0), value: "21°" },
    Column { caption: "10:00", icon: Some(1), value: "19°" },
]);
if rc == -22 {
    api.widget("Forecast", "21º");
}
```

- Layout (stock widget 97): columns 80 px apart from x 20 (fewer columns share 320 px), caption in `font24L` at
  y 18, the icon centred, the value in `font24M` at y 116, all white on the tile grey.
- Example: `widget-columns`; `tests/test_widget_templates_e2e.py`. The phone can push the same card
  (`wp2apps widget set`, `docs/widget-push.md`).

### Page (next to the watch face)

An app can also be a whole page of its own next to the watch face, a swipe away from it, instead of the menu.

- Declare it: `cmf_wp2_app::app_features!(cmf_wp2_app::WIDGET | cmf_wp2_app::PAGE);` (any mix of the two). Then a long
  press on its row in Apps → "Add page" puts it first left of the face ("Remove page" takes it away). One app at a
  time: adding another app's page replaces it (the loader runs one app).
- The page is the stub view `0xF8 + slot`; from the host `wp2apps cards set view:0xF9 …` for slot 1.
- The app starts when the face (and with it the page) is laid out and stays running behind it. A swipe away gives
  `PAUSE`, back `RESUME` (as a stock popup does): draw nothing and run no motor in between.
- `api.on_page() -> bool` tells the two apart, e.g. a compact screen on the page. On the page the crown press and the
  button stay with the watch (no `BUTTON`): the app does not exit itself. If it returns anyway, it starts again when
  the page comes back.
- A tap on an app widget or the Apps menu takes the loader over: the page's app stops, and the page starts it again the
  next time the face shows.
- Examples: `hello` (says "hello page"), `herdr-agents`; `tests/test_app_pages_e2e.py`.

`wp2apps info app.wp2app` lists what a package declares (`"features": ["widget", "page"]`). The record is 8 bytes in
`.wp2info` after the icon (`"WP2F"`, flags), so a package with it installs on any loader; an older loader just offers no
"Add widget" / "Add page".

### Host messages (1.1)

| Call | Returns |
|---|---|
| `send(data: &[u8]) -> i32` | `0` queued; `-1` length not `1..=128` (`MSG_MAX`); `-11` outbox full (4 queued); `-107` no host listening |
| `recv(buf: &mut [u8]) -> Option<&[u8]>` | the oldest message, removed from the inbox; `None` if empty or `buf` is too short (use `[0u8; MSG_MAX]`) |

Details and the host side are in section 4.

### Event loop rules

- Block in `event(...)`. For animation, draw, `show`, then wait the rest of the frame in `event(left)`
  (agent-glow `FRAME_MS = 33`). For a static screen, `event(u32::MAX)` or a slow tick for a clock.
- Handle `BUTTON` in every loop, nested ones included, and leave within 1000 ms.
- On `PAUSE`: motor off, stop timers, wait for `RESUME`.
- Keep timeouts derived from `now()` deadlines, not from counting `event` calls.

## 4. Talking to the Mac

### The channel

The app exchanges up to 128-byte messages with a host (today: the Python package on a Mac) over the stub's BLE
channel. Protocol: `docs/abi-1.1.md`.

| | |
|---|---|
| Size | 1–128 B per message, both directions |
| Queues | inbox 4, outbox 4 messages |
| Host → app | acknowledged. Status `0` OK, `1` BAD_REQUEST, `7` NO_APP (nothing runs, another slot, or an ABI 1.0 app), `8` FULL, `9` PAUSED; the host retries FULL and PAUSED |
| App → host | sent only after the watch answers a host frame, so the host polls with PING (default every 0.25 s). `send` returns `-107` when no host listens. A new host key drops queued messages; the host sees gaps in a per-boot `seq` |
| Latency (emulator, Netsim, MTU 23) | host → app 14–16 ms per acked 128 B message; echo round trip median 270 ms (one poll); app → host 72 ms per message in a burst of 41, none lost (`docs/abi-1.1.md`, from `tests/test_api_e2e.py`). Not measured on hardware |
| Security | the pairing secret is readable by any BLE client, so anyone in range can read and send app messages. Treat input as untrusted; never send personal data (names, titles, notification text, locations, keys) |

Hosts that cannot bind or poll (an iPhone app next to Nothing X) use the stub's own channel key and get app messages
unsolicited: `docs/abi-1.4.md`.

### Python host

`wp2apps.channel.Channel` (`src/wp2apps/channel.py`), asyncio:

| Member | Behaviour |
|---|---|
| `Channel(address=None, hci_port=None, slot=ANY, poll=0.25)` | exactly one of `address` (real watch) or `hci_port` (emulator) |
| `.key` | session key; set it before the first call to resume a bound session instead of binding (no popup) |
| `.slot` | `channel.ANY` (0xFF) = whatever app runs; set it to the slot that talked to you |
| `.poll` | seconds between PINGs while `recv` waits; each PING is one encrypted frame each way (watch power) |
| `await connect()` | connects, resuming `.key` first; retries with backoff 1, 2, 4 … 30 s |
| `await send(data, timeout=10)` | reconnects on link loss, retries FULL/PAUSED; raises `channel.NoApp`, `TimeoutError`, or `ValueError` (refused) |
| `await recv(timeout=None)` | next `Message(seq, slot, data)` from the app (of `.slot` unless `ANY`); raises `TimeoutError` |
| `await drop()` / `close()`, `async with` | closes the link |

Lower level: `channel.ping(session) -> State(running_slot, flags)`, `channel.send(session, …)`,
`channel.recv(session, …)` on a `link.Session`.

CLI, for poking an app by hand (both reuse the cached session key):

```bash
uv run --locked wp2apps send 'hello' --address <UUID>       # or --hci-port P; --hex, --slot N, --wait S
uv run --locked wp2apps listen --count 5 --address <UUID>   # first line: running slot, abi_1_1, paused;
                                                            # then one JSON line per message: seq, slot, data, text
```

### A complete pair: app and daemon

`line` shows a line of text that the Mac sends once a second. App (`sdk/examples/line/src/main.rs`, `test = false`,
no icon), built and run in the emulator with the daemon below:

```rust
#![no_std]
#![no_main]

// Shows one line of text from a Mac daemon. Protocol: "d1" + kind; R (app -> Mac) asks for the line,
// L (Mac -> app) is seq + UTF-8 text.
use cmf_wp2_app::{Api, BUTTON, MSG_MAX, PAUSE, RESUME};

cmf_wp2_app::entry!(main);

const ASK_MS: u32 = 3000;

fn draw(api: &Api, text: &str) {
    api.rect(0, 200, api.width, 16, 0x0000);
    api.text(233 - 8 * text.chars().count() as i16, 200, text, 0xFFFF);
    api.show();
}

// true on RESUME, false on BUTTON
fn paused(api: &Api) -> bool {
    loop {
        match api.event(u32::MAX).map(|e| e.kind) {
            Some(RESUME) => return true,
            Some(BUTTON) => return false,
            _ => {}
        }
    }
}

fn main(api: &'static Api) -> i32 {
    let mut synced = false; // got a line since the last (re)start
    let mut last_seq = 0u8;
    let mut asked: Option<u32> = None;
    draw(api, "waiting for Mac...");
    loop {
        let now = api.now();
        if !synced && asked.is_none_or(|t| now.wrapping_sub(t) >= ASK_MS) {
            api.send(b"d1R"); // -107 while no host listens: asked again in 3 s
            asked = Some(now);
        }
        match api.event(500).map(|e| e.kind) {
            Some(BUTTON) => return 0,
            Some(PAUSE) => {
                if !paused(api) {
                    return 0;
                }
                (synced, asked) = (false, None); // messages were refused meanwhile: ask again
            }
            _ => {}
        }
        let mut buf = [0u8; MSG_MAX];
        while let Some(m) = api.recv(&mut buf) {
            if let [b'd', b'1', b'L', seq, text @ ..] = m {
                if synced && *seq != last_seq.wrapping_add(1) {
                    synced = false; // a gap: the Mac resends on the next R
                } else {
                    (synced, last_seq) = (true, *seq);
                }
                draw(api, core::str::from_utf8(text).unwrap_or("?"));
            }
        }
    }
}
```

Daemon, modelled on `src/wp2apps/herdr.py` (run here with `uv run --locked python line_daemon.py <hci-port>`):

```python
"""Mac side of the `line` app: sends the Mac's clock once a second while the app is open."""
import asyncio
import sys
import time

from wp2apps import channel
from wp2apps.cli import cache_key, cached_key

MAGIC = b"d1"


async def run(ch: channel.Channel, target: str) -> None:
    open_, seq, key = False, 0, None
    while True:
        ch.poll = 0.25 if open_ else 1.0  # each poll is one PING frame: slow down while the app is closed
        try:
            m = await ch.recv(timeout=1.0)  # connects (resuming ch.key), PINGs to collect app messages
        except TimeoutError:
            m = None
        if ch.key != key:
            key = ch.key
            cache_key(target, key)  # reuse it next time: a fresh bind shows the "Paired" popup
        if m and m.data == MAGIC + b"R":
            ch.slot, open_ = m.slot, True  # answer the slot that asked
        if not open_:
            continue
        seq = (seq + 1) & 0xFF
        try:
            await ch.send(MAGIC + b"L" + bytes([seq]) + time.strftime("%H:%M:%S").encode(), timeout=5)
        except (channel.NoApp, TimeoutError, ValueError):
            ch.slot, open_ = channel.ANY, False  # closed or stuck: wait for the next R


def main(address: str | None = None, hci_port: int | None = None) -> None:
    target = f"ble:{address}" if address else f"hci:{hci_port}"
    ch = channel.Channel(address=address, hci_port=hci_port)
    ch.key = cached_key(target)  # same cache as `wp2apps send/listen/herdr`
    asyncio.run(run(ch, target))


if __name__ == "__main__":
    main(hci_port=int(sys.argv[1]))
```

A daemon that belongs in the repository goes to `src/wp2apps/<name>.py` with a subcommand in `src/wp2apps/cli.py`,
as `wp2apps herdr` does.

### Protocol design

- Start every message with a magic and version (`"h1"`, `"d1"`), then a kind byte. Ignore anything else.
- The app pulls, the host pushes. The app asks for a snapshot (`R`) on start, on `RESUME` and after a gap, and repeats
  the request every few seconds until it gets one (`send` fails with `-107` while nobody listens). The host sends
  nothing until it is asked, and stops after `NoApp` or a send that stays FULL/PAUSED for 5 s.
- Number host messages with a `seq` byte; a gap means lost messages, so the app asks for a snapshot again.
- Snapshot framing as in herdr-agents: `B` begin, upserts, `E` end with a count the app checks; deltas after that;
  a heartbeat `H` every 10 s so the app can show "stale" (`docs/herdr-agents.md`).
- Keep each record well under 128 B and cut UTF-8 on a char boundary on the host (`clip` in `herdr.py`) and again
  on the watch (`fit` in `herdr-agents/src/model.rs`).
- Poll slowly while the app is closed (herdr: 1 s) and at 0.25 s only while it is open.

### Operating rules

- **Bind before the app opens, never while it runs.** A full bind shows the stock "Paired" popup; it pauses the
  app, and closing it with the button sends the stock to the clock, which ends the app. Order: install → close the
  popup → start the daemon (it binds once if no key is cached; close that popup) → open the app. After that the key
  in `~/.cache/wp2apps/sessions.json` (mode 0600, `WP2APPS_SESSIONS` overrides it) resumes without a popup until the
  watch reboots or another client binds. The file is personal data: never in the repository.
- **One central at a time.** Turn the phone's Bluetooth off; if the Nothing X app connects it binds with a new key
  and the daemon loses the session. The channel key `K` doesn't change this: it lets a second app share the
  link on the same phone (`docs/abi-1.4.md`, "Writing a host"), not a second device.
- **macOS can hold the LE link** after a crash, a sleep or even a normal session: the watch stops advertising and the
  daemon retries without connecting (the watch shows as connected in the macOS Bluetooth settings while no process
  of ours holds it). Turning the Mac's Bluetooth off and on frees it.
- `install`, `list` and `remove` always bind (popup). Do not run them while an app that talks to a daemon is open.

## 5. UI kit: `cmf-wp2-ui`

`sdk/cmf-wp2-ui` is the shared drawing, layout, text, widget, haptic and motion code. `no_std`, no heap, no floats
(the target has no FPU). Add `cmf-wp2-ui = { path = "../../cmf-wp2-ui" }`; `sdk/examples/kit-gallery` shows every
piece, one page each, and `herdr-agents` and `agent-glow` are built on it.

### Surface

Everything draws through `Surface { fn rect(x, y, w, h, rgb565); fn text(x, y, &str, rgb565) }` (i32 coordinates,
`w`/`h` <= 0 draws nothing). It is implemented for `&Api` (`let mut screen = api; draw(&mut screen)`), for `Fb` (a
466×466 host framebuffer over a `&mut [u16]` with the loader's own clipping and font, so host tests see the watch's
pixels and count rects) and for `Count` (counts rects and pixels, draws nothing). The simulator runs the same code
through `&Api`. Constants: `SIZE` 466, `C` 233 (centre and radius), `CHAR` 16, `ONE` 1024 (fixed-point one).

### Drawing (`draw`)

Every shape writes each pixel once per call (no flicker), merges same-colour neighbours on a row into one `rect`
(`Run`) and blends its edge with a background colour you pass (the app only has opaque rects, so it must know what
lies under an edge). Rect counts below are for the sizes given; frame cost on the watch is about 1.8 µs per rect plus
compute (agent-glow numbers).

| Call | What | Rects |
|---|---|---|
| `disc(s, cx, cy, r, c, bg)` | AA filled circle | r 20: 143, r 8: 51 |
| `ring(s, cx, cy, r0, r1, c, bg)` | AA ring, hole untouched | r 24..36: 432 |
| `arc(s, cx, cy, r0, r1, start, sweep, on, off, bg)` | AA ring segment, clockwise from `start` (u16 turns, 0 = 3 o'clock, 49152 = 12) over `sweep` (65536 = full); `off: Some(c)` paints the rest of the ring in the same pass (a track) | r 130..150: ~2.1 k |
| `line(s, x0, y0, x1, y1, w, c, bg)` | AA line, butt ends | ~2 per px of length (200 px, w 5: 432) |
| `stroke(s, ox, oy, ux, uy, r0, r1, w, old, shade, bg)` | AA stroke along a Q14 unit vector, colour by distance; `old` erases the part of the previous stroke it no longer covers, in the same pass | as `line` |
| `rrect(s, x, y, w, h, r, c, bg)` | AA rounded rect, `r = h / 2` is a pill; the straight middle is one rect | 120×40 pill: 136 |
| `gradient(s, x, y, w, h, from, to, vert)` | linear gradient, one rect per distinct 565 colour | one per colour step (246 px wide: 95) |
| `radial(s, cx, cy, bands, clear, under)`, `radial_n::<N>(...)` | concentric `Band { r, paint, soft }`: `Solid`, `Skip` (shows `under`), `Keep` (already on screen, only its edge is blended) or `Conic` (colour by angle from a 256-entry table, for spinners and glows); `soft` 0 hard, 1 AA, `GRAD` a gradient into the next band | agent-glow's orb: ~6 k. Stack: `radial` takes ~2.5 KB (24 bands), `radial_n::<N>` ~100 B per band; `disc`/`ring` use 1–2 |

Colours (`color`): `Rgb(r, g, b)` with `rgb565()` (rounded), `from565` (exact round trip), `scale(k)`, `mix(o, t)`;
`rgb565(r, g, b)` is `const`; `blend565`, `dim565` work on 565 values.

### Round screen (`geom`)

`chord(y0, y1, margin) -> (x0, x1)`: the span visible on every row of `[y0, y1)`; `text_box(y, margin) -> (x, chars)`
for a text line; `half(y)`, `inside(x, y)`, `centred(cx, w)`, `polar(r, angle)`.

### Text (`text`)

`fit(bytes, max)` (longest valid UTF-8 prefix by bytes), `clip(s, chars) -> (&str, cut)`, `chars`, `width`,
`fitted(s, x, y, text, cells, c)` (cuts with a drawn ellipsis in the last cell), `centre(s, cx, y, text, c)`,
`wrap(text, cells, &mut [&str; N]) -> lines` (greedy by words, `\n` breaks, the last slot gets the rest so `fitted`
ends it with an ellipsis; call it with two slots per line to give each line of a round screen its own width, as
kit-gallery's text page does), `int(n, &mut [u8; 11]) -> &str`, `big(s, x, y, text, scale, c)` (the loader's font at
`scale` px per font pixel, Cyrillic included; `scale` 2 equals `text`; "12:30" at 6: 36 rects).

### Widgets (`widget`)

Widgets own no event loop: the app passes crown deltas, `TOUCH_DOWN` points (its `y` is the reliable one) and
`now`; widgets return what happened and `draw` repaints only what changed, returning `true` when it drew (then
`show`). `invalidate()` forces a repaint (after `RESUME`).

- `List::new(top, row_h, rows, margin)`: `set_len`, `crown(delta) -> moved`, `hit(y) -> Option<item>`, `row(r) ->
  Row { y, h, x0, x1 }` (follows the round edge; `cells(x)` chars that fit), `more()`, `invalidate(item)`,
  `draw(s, bg, |s, item, row| ...)` repaints dirty rows only, each cleared over its chord first.
- `FocusList::new(top, bottom, cy, pitch, margin)`: the stock app list's selection on a centre line. `crown(delta,
  now)` and `select(i, now)` move it with a 160 ms glide (`GLIDE_MS`), `jump(i)` without; `hit(y, now)`,
  `selected()`, `set_len`, `moving(now)`. `draw(s, now, bg, |s, item, focus| ...)` clears `top..bottom` and paints
  the items that fit, each with `Focus { y, x0, x1, d, t }`: `t` is ONE on the centre line and 0 a pitch away, so
  highlight and enlarge by `t` and dim by `d`. Redraws only while it moves (or after `invalidate`). For an LVGL app
  use the native one instead: `stock::focus_page` (section 5a).
- `Label<N>::new(cx, y, scale, fg, bg)`: centred text or a number (`set`, `set_int`, `set_colour`), clipped to the
  circle with an ellipsis; clears only what it drew last. Use it for headers, titles and big numbers.
- `ProgressRing::new(cx, cy, r0, r1, on, off, bg)`: `set(0..=ONE)` from 12 o'clock; redraws the whole ring on change.
- `Toggle::new(x, y, w, h, on, off, bg)`: pill and knob, `tap(x, y, now) -> Option<new state>`, slides 150 ms;
  `jump(on)` sets it without the slide; `animating(now)`.
- `Button::new(x, y, w, h, label, colour, bg)`: `tap(x, y, now) -> hit`, pressed look for 150 ms, `wait(now)`.
- `page_dots(s, y, pages, current, on, off)`.

### Haptics (`haptic`)

`Pattern::new(&[(duty ‰, ms), ..., (0, 0)])` is checked at compile time in a `const`: it must end with the motor off
and no on-stretch may reach 1500 ms (the loader cuts at 2000). Ready: `TICK` (tap feedback, 250 ‰ 20 ms), `DOUBLE`,
`ALERT` (3 × 110 ms full), `DONE` (650 ‰ 220 ms). `Player` is pure (`start(p, now)`, `poll(now) -> Option<duty>`,
`wait(now)`, `stop() -> Some(0) if the motor may be on`); `Motor::new(api)` drives the watch at 31 Hz: `play(p)`,
`update()` every loop turn, `wait()` to cap the event timeout, `off()` on `PAUSE`; it also turns the motor off when
dropped (when `main` returns) and clears a `-4` lock.

### Motion (`motion`)

`sin`/`cos` (u16 turns → Q14), `angle(dx, dy)` (atan2, 1/256 turn), `isqrt`, `wave(t, period)` (0 → ONE → 0),
`lerp`, easings over `0..=ONE`: `linear`, `ease_in`, `ease_out`, `ease_in_out` (cubic), `spring`.
`Tween { from, to, start, ms, ease }`: `Tween::at(v)`, `go(to, now, ms)` (restarts from the current value, no
jump), `value(now)`, `done(now)`; wrapping-safe millis.

### A loop with the kit

```rust
let mut screen = api;
let mut motor = Motor::new(api);
let mut list = List::new(100, 56, 5, 10);
list.set_len(ITEMS.len());
loop {
    motor.update();
    if list.draw(&mut screen, 0, |s, i, row| { fitted(s, row.x0, row.y + 20, ITEMS[i], row.cells(row.x0), 0xFFFF); }) {
        api.show();
    }
    let wait = motor.wait().unwrap_or(1000);
    match api.event(wait) {
        Some(e) if e.kind == BUTTON => break,
        Some(e) if e.kind == CROWN => { list.crown(e.delta); }
        Some(e) if e.kind == TOUCH_DOWN && list.hit(e.y as i32).is_some() => motor.play(haptic::TICK),
        Some(e) if e.kind == PAUSE => motor.off(), // then wait for RESUME and list.invalidate_all()
        _ => {}
    }
}
```

### Still in the examples

| Need | Where |
|---|---|
| Storage snapshot (versioned blob, garbage loads nothing) | `herdr-agents/src/model.rs`: `Model::save`, `Model::load` |
| Snapshot/delta protocol with seq gaps and resync | `herdr-agents/src/model.rs`: `decode`, `Model::apply`, `Effect` |
| Frame loop at 33 ms with `STATS` for the e2e | `agent-glow/src/main.rs`, `kit-gallery/src/main.rs` |
| Settings screen: tap selects, crown changes | `vibration-lab/src/main.rs`, `lab.rs` |
| Every 1.1 call | `api-demo/src/main.rs` |
| LVGL with a framebuffer fallback (simulator); a growing log, newest on top, oldest deleted | `api-demo/src/main.rs`, `log.rs` |
| Text into a buffer with `core::fmt::Write` | `Buf<N>` in `api-demo/src/main.rs` |

Missing today: touch move, gestures and long press, the accelerometer
(looked at and not shipped, `docs/abi-1.1.md`; heart rate and GPS ship as `heart_rate` and `gps`), a notice before the screen
blanks, a random source and a phone-side host. In the kit: rounded arc caps, a scroll bar widget, and a ring that
repaints only the changed sector.

## 5a. LVGL apps (ABI 1.2)

Instead of drawing with `rect`, an app can build a screen from the watch's own LVGL 8.3 widgets, fonts and
scrolling. Contract, records and budgets: `docs/abi-1.2.md`. Examples: `sdk/examples/ui-gallery` (every widget in
the stock look: a stock focus list of sections with buttons, switches, gauges, a time picker, the stock fonts, label
long modes, the Exercise cards and both pagers; the table below),
`herdr-agents` (a live focus list) and `sdk/examples/lv-demo`
(a crown-scrolled page with the title, a card with a slider driving an arc, a row per haptic pattern).

**Always the stock look: `cmf_wp2_app::stock`.** The firmware has no theme: a label without a font falls back to
LVGL's small built-in one, a widget without styles is bare. `stock` gives:
- the system font: `stock::font` (its own batch), then `stock::theme` (first in the screen batch);
- the stock colours: `ACCENT`, `ROW_BG`, `GREY` and the others;
- the parts of the Apps menu and the stock Settings list: `page` (full-screen, crown, round scroll bar), `title`,
  `row` (letter circle, name, grey detail), `card`, plus `slider` and `arc` styles.

- the stock app list: `focus_page(b, icon)` and `focus_row` (FOCUS: the fisheye, the centred row CHECKED, `FOCUSED`
  events with its index), as herdr-agents uses them. Like the stock list: rows from x 16, no pill behind the centred
  row (the fisheye alone marks it), child 0 zooms around its centre, the crown and a drag scroll freely (no snap).
  Put child 0 at the row's left edge and the text at `stock::FOCUS_TEXT_X`. Unlike the stock list, the first and
  the last row can reach the centre line, so every row can be the focused one. Proof: `tests/test_focus_e2e.py`.
- the stock Exercise list: `focus_page(b, FOCUS_CARDS)` and `focus_card` (cards that narrow away from the centre;
  a loader from 2026-11-13 on, `docs/abi-1.4.md`).
- a vertical pager: `pager::<N>(b, n)` (full-screen pages that snap, page dots at the right edge; a loader from
  2026-11-14 on), below.

### Pages (vertical pager)

`stock::pager::<N>(&mut b, n)` builds the stock `lv_tileview` as a column of `n` (≤ `N`) full-screen pages with page dots on the
right edge (a white pill for the current page, grey dots for the others). Swipe up/down or turn the crown: one page
per step, and it snaps to a page. Example: ui-gallery's "Pages" section (`sdk/examples/ui-gallery/src/screens.rs`,
`pages`); e2e `tests/test_pager_e2e.py`, frames `runs/pager/`.

```rust
let mut p = stock::pager::<3>(&mut b, 3)?;       // n pages of at most N; p.root (del it to remove all), p.obj, p.pages, p.dots
b.flex(p.pages[0], Flow::Column, FlexAlign::Start, FlexAlign::Center, FlexAlign::Start)?;
stock::title(&mut b, p.pages[0], "Today")?;     // build each page's content into p.pages[i]
b.submit(&lv)?;
// event loop: every event through the pager; it moves the dots and returns the page
if let Some(i) = p.event(&mut b, &e)? { b.submit(&lv)?; /* page i is shown */ }
p.go(&mut b, 2)?;                                // glide to a page; the page event follows
p.jump(&mut b, 1)?;                              // show a page at once (open on it), dots too
```

- **A page taller than the screen scrolls first.** A drag scrolls the page's own content and the pager takes over at
  its end (LVGL's scroll chain); the crown does the same: it scrolls the page by the stock crown step and, at the end,
  the next step goes to the next page (the loader, `pager_crown`). Going back a page keeps that page's scroll.
  No stock vertical pager was traced to copy this from; it is the rule that makes drag and crown agree.
- **The pager takes the crown when it is created** (as a roller does): `knob(other)` moves it away, `knob(p.obj)`
  brings it back. The app gets no `CROWN` events while the pager has it.
- **The page event comes without `events`:** `Event { kind: LV, x: pager, y: 28 (VALUE_CHANGED), delta: page }` once
  the pager settles on a page (after a swipe, a crown step or `go`), coalesced.
- **The stock back swipe still works.** The pager only scrolls vertically, so a right swipe is still the stock "back"
  (`BUTTON`, or the app's own back with `own_back`). `tests/test_pager_e2e.py` checks it over the pager.
- Pages go with their pager: `del` refuses a page, and a pager holds pages only. Pages are plain, transparent
  objects without padding. They get no round scroll bar.
- A loader older than 2026-11-14 answers `-22` to the pager's `create`: install the loader first.

Labels with a fixed size: `b.long_mode(label, LongMode::Dot)` keeps them on one line ending in "...".

No borders (`BorderWidth` > 0): on a rounded object they stop the emulator (its display engine lacks the path; the
watch is untested), so draw an outline as a pill inside a pill (ui-gallery's "More").

Do not pick other fonts or colours unless the owner asks for them.

### Horizontal pager

`stock::hpager::<N>(&mut b, n)` builds the stock workout screen's layout: `n` full-screen pages side by side
(`HPager { obj, pages, at, n }`), one page per swipe left/right. Fill `pages[i]` like any parent; feed every event
to `HPager::event` (it returns the page it settled on); `go(i)` glides, `jump(i)` shows at once. The crown stays
with the app (CROWN events). While a horizontal pager lives the right swipe pages back and is **not** `BUTTON`:
the app leaves with the crown press. Loader of 2026-11-16 (`docs/abi-1.4.md`, "Horizontal pager"); an older loader
answers `-22` at `create`. ui-gallery's `!hpager` is the example.

`stock::music(&mut b, page)` puts the stock workout screen's music page into a page: "No music" or the phone's
track, previous / play-pause / next and volume, with the stock images and fonts. The loader fills it and sends the
keys and opens the stock volume popup; `Music::event(&e)` tells the app which button was tapped, `Music::playing(&mut b, on)` sets the play button
until the phone's state changes. One at a time; an older loader answers `-22`. ui-gallery's `!music` is the
example (`docs/abi-1.4.md`, "Music tile").

### Stock components: SDK and ui-gallery

What the stock apps are built from (`docs/recon-lvgl.md`, "Widget classes present" and "Vendor components"; the
stock app list, Settings, Exercise and workout screens in the emulator), what the SDK gives for it and where
ui-gallery shows it. "Composed": no stock widget, built from `Obj`/`Btn`/`Slider` and styles in the app.

| Stock component | SDK | ui-gallery |
|---|---|---|
| Full-screen list, crown scrolling, round scroll bar (`jx_scroll_bar`) | `stock::page`, `Batch::knob`, `scroll_bar` | every section |
| Accent title | `stock::title` | every section |
| Settings row: icon circle, name, grey detail | `stock::row` (a letter in the circle, no image) | Lists |
| App list with the fisheye | `stock::focus_page(icon)`, `focus_row` | the menu |
| Exercise list: cards narrowing off the centre | `stock::focus_page(FOCUS_CARDS)`, `focus_card` | Cards |
| Panel for controls | `stock::card` | Switches, Gauges, Text |
| Pill button: accent, secondary, outline, disabled | composed (`Class::Btn` + styles; no `stock` helper) | Buttons |
| Toggle switch (no `lv_switch` in the firmware) | composed: a 0..1 `Slider` with `stock::slider` | Switches |
| Checkbox (no `lv_checkbox`) | composed: a `Checkable` `Btn` | Switches |
| Slider | `Class::Slider` + `stock::slider` | Gauges |
| Progress bar | `Class::Bar` | Gauges |
| Arc / ring progress | `Class::Arc` + `stock::arc`, `angles` | Gauges |
| Time picker (`lv_roller`, vendor `text_roller`) | `Class::Roller`, `options` | Picker |
| Stock fonts (16 `font*.font` files) | `StockFont`, 4 slots at a time | Fonts (18M, 24M, 32M, 48M) |
| Label long modes (dots, scrolling text) | `long_mode`, `LongMode` | Text |
| Vertical pager with page dots (`lv_tileview`) | `stock::pager` | Pages |
| Horizontal pager, the workout screen | `stock::hpager` | Swipe; `!hpager` with music labels |
| Phone music labels | `Api::media_bind` | `!hpager` (tests/test_pager_e2e.py) |
| Workout music page (sport1_view tile) | `stock::music` (`Class::Music`) | `!music` (tests/test_music_e2e.py) |
| Tap haptic | `cmf_wp2_ui::haptic::TICK` | rows, buttons, switches |
| Images and icons (`lv_img`, vendor `simple_img`, `lv_imgbtn`) | no (no image source in the LVGL records) | no |
| Frame animation (`jx_pic_anim`) | no | no |
| Chart (`lv_chart`: heart rate, sleep, steps) | no | no |
| Popup / confirm dialog (stock popup views) | no (composable from `card` and buttons) | no |
| Loading spinner (no `lv_spinner`) | no (composable: an `Arc` the app turns) | no |
| QR code (`lv_qrcode`) | no | no |
| Key matrix (`lv_btnmatrix`) | no | no |
| Canvas (`lv_canvas`) | no in LVGL; pixel drawing is `cmf-wp2-ui` | no (kit-gallery) |
| Widget tile left of the watch face; page next to it | `Api::widget` (the stub draws it); `app_features!(PAGE)` | no (not a screen inside an app) |

### LVGL or `cmf-wp2-ui`?

| | LVGL (`cmf_wp2_app::lv`) | pixel (`cmf-wp2-ui`) |
|---|---|---|
| Fits | lists, buttons, sliders, rollers, text in the stock fonts, crown scrolling, many static widgets | custom drawing: rings, glows, animation every frame, anything the widgets lack |
| Costs | each `submit` waits up to one 10 ms UI tick; GUI heap (lv-demo's screen: 6 868 B) | app-side CPU and a full-width write-back per frame (section 2) |
| Simulator | **not yet** (`lv_submit` returns `-95`) | yes (`scripts/sim`) |
| Loader | needs 1.2 (`api.lv()` is `None` before) | any |

An app is one or the other: the first submit hides the framebuffer image, so `rect`/`text` are no longer shown.

### A minimal screen

```rust
use cmf_wp2_app::lv::*;
use cmf_wp2_app::{stock, Api, BUTTON, LV};

fn main(api: &'static Api) -> i32 {
    let Some(lv) = api.lv() else { return -95 };       // loader older than 1.2
    let Some(mut b) = Batch::new() else { return -1 }; // one batch per app, a 2 KiB static
    // the system font in a batch of its own: a missing file stops the batch
    if stock::font(&mut b).and_then(|_| b.submit(&lv)).is_err() { return 1; }
    let build = |b: &mut Batch| -> Result<[Obj; 2], LvError> {
        stock::theme(b)?;                                // every label: system font, white
        let (page, _bar) = stock::page(b)?;              // crown scrolls it, round scroll bar
        stock::title(b, page, "Hello")?;
        Ok([stock::row(b, page, "W", "Wi-Fi", "On")?, stock::row(b, page, "B", "Bluetooth", "")?])
    };
    let Ok(rows) = build(&mut b) else { return 2 };
    if b.submit(&lv).is_err() { return 3; }             // the whole screen: one tick
    loop {
        match api.event(u32::MAX) {
            Some(e) if e.kind == BUTTON => return 0,
            Some(e) if e.kind == LV && e.y == 7 && e.x as u16 == rows[0].0 => { /* row 0 CLICKED */ }
            _ => {}
        }
    }
}
```

This snippet was not run on the emulator. The same calls are what `lv-demo` does, and that app is e2e-tested. Its
screen builder (`src/screen.rs`) is host-tested: each batch validates against the loader's checker and fits 2048 B.

### Rules that cost time

- **One batch per screen update.** Each `submit` blocks until the next UI tick (up to 10 ms). Build a screen, or a
  group of changes, in one batch; `Batch::submit` empties the buffer, `LvError::Full` means "submit and continue".
- **Open fonts in their own batch.** `-2` stops a batch at the failed font. Slots: 4 (`stock::FONT` is slot 0).
  Names: `StockFont`.
- **No theme.** A widget without styles for its parts is invisible or bare (slider, arc, button). Use `stock`, or style
  the main part and `stock::INDICATOR` / `KNOB` / `SELECTED` through the selector. A selector holds one part (MAIN..ITEMS) and at most one state; image sources and the
  Actions style ids are not available (`docs/abi-1.2.md`, STYLE).
- **Arcs are 0..100**, `i16` values, no `range`. Bars and sliders take `range`.
- **Events.** Subscribe with `events` (it adds; there is no unsubscribe); you get `Event { kind: LV, x: handle, y: LVGL event code, delta: value }`
  (CLICKED 7, PRESSED 1, RELEASED 8, LONG_PRESSED 5, VALUE_CHANGED 28). `VALUE_CHANGED` coalesces per (handle, code): a
  drag leaves the latest value, so read `delta` and do not count events.
- **Children of a flex column keep their order.** There is no "move to index": an object created later lands at the
  end. Rebuild a part that must stay above others, or put changing parts last (lv-demo's rows).
- **Crown.** The crown goes to the root (`CROWN ±1`) unless an object is the knob. `knob(list)` makes a scrollable
  object scroll with the crown; `scroll_bar(list)` adds the round bar, which goes with `list` (`del` refuses the bar
  itself). **A roller takes the crown when it is created**: send `knob(...)` after creating it. A plain `obj` is
  scrollable by default: clear `Flag::Scrollable` before `knob(panel)` if you want `CROWN` events from it.
- **A rightward drag across a slider is the stock back gesture** (`BUTTON`). Test sliders with leftward drags.
- **Rollers: keep them short and not infinite, mostly with `F48M` on the selected row.** Every frame of a scroll,
  LVGL measures every row with the selected row's font, and infinite mode repeats the rows 7 times. The stock
  firmware gives `font48M` a 6-glyph cache (the 10 KiB default, it has no stock preset; `font32M` holds 73 glyphs).
  The digits of a time picker therefore miss it on every row, and each miss is a NAND read plus an RLE unpack. The
  ui-gallery picker (24 + 60 rows) drew at 4-6 fps while scrolling in infinite mode and at 22-38 fps in normal mode.
- **Budgets.** 192 objects, 4 fonts, 128 B text, 240 B roller options, 2048 B per batch. A bad batch returns `-22` or
  `-12` and changes nothing; the SDK's handle table is then out of step, so treat it as a bug.
- **Lifetime.** Exit, crash, force stop and view delete delete the root, every object and the fonts. An app has
  nothing to free. `BUTTON` still gives it 1 s.
- **Bitmaps.** `Batch::image` makes an `lv_img` object and `Batch::set_image` points it at `&'static [u16]` pixels
  the app carries (`bitmaps/*.png`, docs/bitmap-assets.md): drawn in place, one handle, re-submit after changing
  the pixels. An older loader answers `-22`; fall back to no image as with `widget_columns`.

### Testing LVGL apps

Host tests for the screen builder work as for any `Batch` code (`cargo test -p lv-demo`). There is no simulator yet.
The emulator e2e is `tests/test_lv_e2e.py`: it reads the loader's `LV_STATE` for object coordinates and flags, and
the GUI heap with `scripts/recon/gui_heap_probe.py`'s walk. Installing a 1.2 loader on a physical watch needs the
owner's yes.

## 6. Testing

### Host unit tests

Put logic that does not need `Api` (protocol, model, layout, maths, patterns) in a module and test it with
`#[cfg(test)]` on the Mac: `(cd sdk && cargo test -p <name> --target aarch64-apple-darwin)`. For drawing, write
against `cmf_wp2_ui::Surface` and test with `cmf_wp2_ui::Fb` over a `Vec<u16>`: same pixels as the watch, and
`fb.rects` counts the cost (`herdr-agents/src/view.rs` tests check that partial redraws end on a full redraw's
image). Before a commit run the three suites from AGENTS.md.

### Simulator

`scripts/sim <name> [options]` builds the app for the Mac with `--cfg wp2_sim` and runs it against `sim/`, a host
implementation of the whole ABI 1.1 table: the loader's `fill_rect`/`text` (wp2-core `gfx`, so text is pixel-exact),
the round panel mask, its event queue (15 events, input dropped while paused), `PAUSE`/`RESUME`, the 2 s motor
cap and lock, the 1 s button escape, storage (`runs/sim/<name>/data.dat`, same format, limit and atomic replace),
local time (`TZ`), a configurable battery, and the channel (`send` is a stdout line, collected every 250 ms like
a PING poll). Build and launch take ~2 s cold, ~0.3 s warm. A no_std app with the `entry!` at the crate root
(hello) needs no change; an app with a `device` module needs the cfg lines of the template above.

- **Window** (default): mouse = touch, wheel or arrows = crown, Esc or B = button, P = pause/resume, M = type a
  message (Enter sends), lines on stdin = messages. The status bar shows shows/s, the motor, keep-on and sent
  messages. `--scale 2` doubles it.
- **Headless** (`--headless`): a virtual clock that jumps to the next due time, so a 5 s script runs in ~0.2 s.
  Input as `--do 't 233 300; wait 500; c -1; b'` or `--script FILE`: `t X Y [MS]` (tap, up after 80 ms),
  `down X Y`, `up X Y`, `c N` (N detents), `b`, `p`, `m TEXT`, `mx HEX`, `wait MS`, `shot NAME`.
  `--frames DIR` writes `NAME.png` per `shot`, `at-MS.png` for `--at MS,…`, every frame with `--every-show`, and
  `last.png`; the JSON lines log (`DIR/log.jsonl`) has `input`, `vibrate`, `motor_cut`, `msg`, `send`, `save`,
  `load`, `keep_screen_on`, `pause`/`resume`, `shot` and `end` (`exit`, `killed` by the escape, `timeout`, `idle`).
  The run ends 2 s after the last input unless `--until MS`. Exit status: 0, 1 for a non-zero app code, 2 killed.
- **Daemons**: `--tcp PORT` puts the channel on `127.0.0.1:PORT` (hex lines; `send` is `-107` until a client
  connects). `wp2apps herdr --sim PORT` drives herdr-agents there; `wp2apps.channel.SimChannel(port)` has the
  `send`/`recv` of `Channel`.

```bash
scripts/sim herdr-agents --headless --frames runs/sim/x --do 'wait 300; t 233 141; wait 200; shot tap; b'
scripts/sim api-demo --epoch 1794000000 --battery 15c            # window, fixed clock, charging
```

Not modelled: speed (draw calls are free, `now()` advances 1 µs per call on the virtual clock), the 8 KiB app
stack, tearing and the dirty-box flush, the brightness of the panel (the emulator capture is ×0.627 darker),
the loader menu and the bind popup. `tests/test_sim.py` runs every example this way.

### Emulator e2e

Pattern: `tests/test_herdr_e2e.py` (app + daemon), `tests/test_demo_e2e.py` (animation, pause, button),
`tests/test_api_e2e.py` (1.1 calls). Helpers in `tests/e2e/harness.py`:

| Helper | Does |
|---|---|
| `installed(names, name=…)` | boots the paired NOR on its own Netsim, installs `target/<name>.wp2app` for each name; yields `(emu, hci_port)` |
| `open_menu(emu)`, `open_app(emu, row)` | close the "Paired" popup, wait out the screen-off after the BLE disconnect, open the loader menu, tap row `row` (centre y = 180 + 128·row) |
| `emu.send(*lines, pause=…)` | input: `t 1 X Y` / `t 0 X Y` touch down/up, `c 1` / `c 0` crown press (BUTTON), `r -80` one crown detent (`delta +1`) |
| `emu.frame()` → `(w, h, rgb)` | the panel; `save_png` in `tests/test_demo_e2e.py` writes it out |
| `emu.motor()` | `motor.log` rows `(ms, duty, hz)` |
| `popup(emu)`, `close_popup(emu)` | charger popup: PAUSE, then RESUME |
| `emu.peek(STUB_RAM + 6, 1)` | running slot, `ff` = none |
| `u32(emu, STUB_RAM + APP_BLOCK_OFF) + symbol(elf, "STATS")` | address of an app `#[unsafe(no_mangle)] static`, to read counters the app keeps for the test (api-demo, agent-glow) |

Run: `WP2_E2E=1 WP2_NETSIM_INSTANCE=<unique> nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.<module> -v'`.
Build `scripts/build-fw` and the apps first. Two `netsimd` on one instance stop each other, so every parallel run
gets its own instance. `WP2_GUI=1` opens a window (Tk 8.6, `nix-shell -p tk`). Logs, UART and frames go to
`runs/e2e/<name>-<time>/`. It takes minutes per test; run it in a pane, not in the background.

Known flakes and their workarounds:

- **gdb `OPEN_VIEW` stall**: `subprocess.TimeoutExpired` from `emu.call` in `open_menu` when the guest went idle
  after the screen blanked. Not an app bug; re-run.
- **Screen-off race**: the panel blanks ~5.7 s after the BLE install connection closes ("Ble wake lock 0"), even
  with `keep_awake`. `harness.ready` waits it out. After an app exits, tap the menu row right away instead of
  calling `open_app` again (as `test_herdr_e2e` does).
- **Colours**: the panel capture scales brightness (white reads ~160 of 255, ×0.627). Compare hue, not absolute
  values (`hue` in `tests/test_herdr_e2e.py`).
- **TOUCH_UP `y`** is wrong in the emulator; drive the app from `TOUCH_DOWN`.
- The charger popup leaves the charger plugged (on purpose: unplugging sets a 5 s auto-sleep and the deep sleep
  stops the emulator), so a later screen wake brings the charge popup back over the app.

### On the watch (owner approval for every write)

Check what the emulator cannot: how the haptics feel, the real frame rate and tearing, text legibility and
colours on the panel, behaviour when the screen blanks or a notification arrives, BLE latency at the real MTU, and
battery use with `keep_screen_on` or a polling daemon.

## 7. Checklist before shipping an app

- [ ] `BUTTON` is handled in every loop, the motor goes off and `main` returns within 1000 ms.
- [ ] `PAUSE` turns the motor off and stops animation; `RESUME` redraws and resyncs with the host.
- [ ] Every wait goes through `event(...)`; no busy loop, no `event` call that can swallow a `BUTTON` mid-pattern.
- [ ] No motor run near 2 s; `-4` is cleared with `vibrate(0, _)`.
- [ ] State is saved on change (the app can die without notice), not per frame; the blob has a version and fits
      4096 B; a bad blob loads as empty.
- [ ] Buffers over ~1 KiB are statics, the image stays well under 128 KiB (`wp2apps info`).
- [ ] Text fits the circle (section 2 table) and each `text` call is ≤ 64 bytes.
- [ ] Nothing personal goes over BLE, in either direction, and nothing personal is logged or committed.
- [ ] Host messages are parsed defensively (magic, version, lengths).
- [ ] `keep_screen_on(true)` only with a deadline.
- [ ] LVGL app: the system font and stock look through `cmf_wp2_app::stock` (no LVGL default font, no own palette),
      every widget part is styled, the screen is one batch, fonts are in their own batch, the crown goes to the right
      object after a roller, nothing relies on a rightward slider drag.
- [ ] `icon.svg` present (or the letter fallback is intended), version bumped, host tests pass, the e2e or a manual
      emulator run covers start, input, pause and exit.

## Read-only storage and installed-app icons

The additive loader service in [Storage inspection](storage-inspection.md) exposes typed capacity, partition, installed-app and bounded file-scan APIs. `Storage::open` detects support; older loaders return `UNSUPPORTED`. `Batch::installed_app_icon` can display another installed app's icon by slot. No firmware or API-table change is required.
