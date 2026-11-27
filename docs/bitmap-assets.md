# Bitmap assets

An app can carry bitmap images and draw them. Two paths, one pixel format, no package-format change:

- **Canvas apps** (`cmf-wp2-ui` or raw `rect`): `Api::pixels` copies RGB565 pixels into the framebuffer through the
  `BLIT` record (`lv_submit` opcode `0x1B`, like `SENSOR`: handled on the app thread, no LVGL needed).
- **LVGL apps**: `Batch::image` creates an `lv_img` object and `Batch::set_image` points it at the app's pixels
  through `IMAGE_SRC` (opcode `0x19`). The loader keeps an `lv_img_dsc_t` for each of at most **8 image objects** alive at once and draws **in place** — the
  pixels are never copied and never leave the app block.

Both read the pixels from the app's own package image, exactly like the menu icon has always done: they are an
ordinary `static` in `.rodata`, covered by `body_crc`, relocated with the code at launch, and alive until the app
stops. No header fields, no trailer, nothing for an older loader or stub to misread.

The loader range-checks every pointer against `[app_block, app_block + app_size)` before it runs, so a record can
never make the loader render memory outside the running app (the batch buffer itself was already checked this way).

## Bringing a bitmap into an app

1. Put `bitmaps/foo.png` next to the crate's `Cargo.toml` (8-bit gray, RGB, palette, gray+alpha or RGBA,
   non-interlaced — what `resvg` and ordinary PNG exporters write; each side 1..=466).
2. Declare the assets next to `entry!`:

   ```rust
   cmf_wp2_app::bitmaps!();
   use bitmaps::FOO; // foo.png, kebab-case becomes SCREAMING_SNAKE
   ```

3. Draw them:

   ```rust
   // canvas
   api.pixels(&FOO.px, x, y, FOO.w, FOO.h);

   // LVGL (one image per batch update, like every other record)
   let img = b.image(cmf_wp2_app::lv::ROOT)?;
   b.set_image(img, FOO.px, FOO.w, FOO.h)?;
   ```

`scripts/build-app` and `scripts/sim` regenerate `src/bitmaps.gen.rs` from `bitmaps/*.png` before cargo
(`uv run python -m wp2apps.bitmaps <crate-dir>` by hand otherwise). The generated file is checked in, so plain
`cargo build` and `cargo test` work on a fresh clone; regeneration is idempotent (unchanged content keeps the
file's mtime). `Bitmap::ok()` is false when a generated array does not match its dimensions.

At build time, alpha is flattened onto black and colours are dithered to RGB565 with the same 4x4 ordered dither as
the app icon (`wp2apps.bitmaps`). There is no alpha channel in v1: transparent PNG areas become black, so put
bitmaps on black screens (or draw a background behind them).

## Limits

Every pixel byte lives in the package image, so the budgets interact. These are the numbers that keep apps
working:

| Budget | Limit | Source |
|---|---|---|
| Package image (all assets + icon + code + data + bss) | **131 072 B (128 KiB)** — `wp2apps pack` refuses more | `APP_SIZE`, `fw/wp2-core/src/layout.rs` |
| Recommended total for bitmap pixels | **49 152 B (48 KiB)** — the generator warns above it; leaves ~80 KiB for code, data, bss and the 27 664 B menu icon | `wp2apps.bitmaps` `SOFT_BYTES` |
| One bitmap | **1..=466 px** each side (validated by loader and generator); opaque RGB565, **2 B per pixel** | `lvcmd::check`, `wp2apps.bitmaps` |
| Image objects (LVGL) | **≤ 8 alive at once** (`MAX_IMAGES`): CREATE of a ninth is refused (`INVALID`); DEL frees one. Canvas `pixels` has no such limit | `lvcmd::check`, loader descriptor pool |
| Pixel pointer | **2-aligned** (`&[u16]` always is); an odd `ptr` is refused (`BLIT` then falls back to rects) | `lvcmd::blit`, `image_src_in` |
| Full-screen baked image | **Impossible**: 466x466 = 434 312 B > the whole package. Stream it instead (below), or bake a cropped/scaled version | `FB_BYTES` vs `APP_SIZE` |
| Typical sizes | 40x40 = 3 200 B, 64x64 = 8 192 B, 96x96 = 18 432 B, 128x128 = 32 768 B — a rule of thumb is **at most 2-3 medium (96x96) or ~6 small (64x64) bitmaps** in an app that also has real code | arithmetic |
| Bitmap count | no explicit cap — the package byte budget is the cap | — |
| Stored app data (`save`) | 4 096 B: too small for pixels; bitmaps belong in the package, not `.dat` | `STORE_MAX` |
| App stack | 8 KiB: never build a pixel buffer on the stack; generated assets are ``'static`` and the stream app keeps only a 128 B message buffer | `ThreadBuf` |
| LVGL objects | each image takes one of **192 handles**; its record is 13 B of the **2 048 B batch**; the loader keeps one 12 B descriptor per handle (≤ 2 316 B total) plus `lv_img_t` on the GUI heap; exit deletes everything as before | `MAX_OBJS`, `BATCH_MAX` |
| Canvas blit cost | zero-copy into the framebuffer; `show` then flushes the rows of the union box (section 2 of app-guide: a 96x96 blit is ~9 192 px of write-back, a full screen 434 312 B) | `ui::flush` |
| LVGL redraw | pixels stay in PSRAM; the loader syncs the cache when `set_image` runs. **Re-submit `set_image` after you change pixels in place**, or LVGL keeps the old cache lines | `lv::exec` |

Compatibility:

| Environment | `api.pixels` | LVGL `image` / `set_image` |
|---|---|---|
| New loader | one row-wise copy (`0x1B`) | draws from app pixels (`0x19`) |
| Older loader (record unknown) | automatic fallback: same picture as spans of `rect` (slower, correct) | first `submit` of the batch returns `-22`; rebuild the screen without images, as with `widget_columns` |
| Simulator | `lv_submit` answers `-95` → fallback works | LVGL is not modelled (`-95`), as everywhere |

## Streaming images from the phone (temporary)

For images that do not fit the package — or to change pictures without reinstalling — the branch carries a
deliberately simple prototype:

- Watch: `sdk/examples/bitmap-stream` (canvas app). `python -m wp2apps.stream` on the phone or Mac.
- Every message is at most `MSG_MAX` 128 bytes, little endian:

| Message | Bytes |
|---|---|
| `START` | `B5 01` ‖ `w u16` ‖ `h u16` (1..=466) |
| `DATA` | `B5 02` ‖ `offset u32` (even, byte offset into the row-major RGB565 image) ‖ payload (1..=120, even) |
| `DONE` | `B5 03` |

`DATA` chunks may arrive in any order, duplicate, or overlap: each one redraws itself, so nothing depends on the
sequence; `offset` and `w/h` bounds are validated and clipped before anything is drawn. `DONE` is just the status
line.

Run it:

```console
# real watch (the bitmap-stream app installed in some slot)
uv run --locked python -m wp2apps.stream photo.png --hci-port 0 --slot 3 --channel-key

# simulator (two terminals)
scripts/sim bitmap-stream --tcp 8765
uv run --locked python -m wp2apps.stream photo.png --tcp 8765
```

### Receiving in an app (the streaming API from the watch side)

Receiving is ordinary ABI 1.1 host messages plus `Api::pixels` — no new `Api` fields, so any
1.2-era loader can run a receiver (the fast copy needs the bitmap loader; older loaders take the
`rect` fallback automatically). The complete, tested receiver is
`sdk/examples/bitmap-stream` (`src/protocol.rs` parses frames and splits them into row spans);
the shape of the loop:

```rust
// one host message (MSG_MAX 128), 2-aligned so DATA payloads read as RGB565 words
#[repr(align(2))]
struct Aligned([u8; 128]);
static mut BUF: Aligned = Aligned([0; 128]);

match event.kind {
    MESSAGE => {
        let buf = unsafe { &mut *addr_of_mut!(BUF.0) };
        while let Some(m) = api.recv(buf) {
            match protocol::parse(m) {
                // START: remember w/h, clear the screen, status "receiving"
                Some(Msg::Start { w, h }) => { receiving = Some((w, h)); }
                // DATA: payload = RGB565 words; one pixels() call per row-span, then show
                Some(Msg::Data { offset, payload }) => {
                    let Some((w, h)) = receiving else { continue };
                    let ox = (api.width - w) / 2;   // centre it: the panel is a disc
                    let oy = (api.height - h) / 2;
                    let px = unsafe {
                        core::slice::from_raw_parts(payload.as_ptr() as *const u16, payload.len() / 2)
                    };
                    for (x, y, n, src) in protocol::segments(offset, payload.len(),
                                                              w as usize, h as usize) {
                        api.pixels(&px[src..src + n], (ox + x as u16) as i16,
                                   (oy + y as u16) as i16, n as u16, 1);
                    }
                    api.show();   // progressive reveal: each chunk flushes only its rows
                }
                Some(Msg::Done) => { /* small status line, keep the picture */ }
                _ => {}
            }
        }
    }
    _ => {}
}
```

Rules that keep a receiver working:

- **Drain the whole inbox per `MESSAGE`** — it is only four messages deep; the sender paces or
  waits for an ACK per chunk anyway.
- **Never buffer the whole image**: chunks go straight into the framebuffer, so the receiver's
  RAM cost is the 128 B message buffer regardless of picture size (a full 466×466 stream fits
  this rule; it just takes longer).
- **Centre every draw** at `(466-w)/2, (466-h)/2` — image coordinates start at the top-left
  corner, which is off the round panel.
- **Handle `PAUSE`/`RESUME`**: messages are refused with status 9 while paused; after resume
  ask the host to restart with `START` (or ignore a stream that was interrupted).
- A `DATA` offset or length that runs past the image is clipped by `segments` — treat every
  message as untrusted (anybody in BLE range can send one).

The receiver keeps **no image buffer**: every chunk is blitted straight into the framebuffer (progressive reveal),
so streaming a full screen costs one screen of RAM — zero — but the whole picture must fit the screen and the app
must stay running. It draws the picture **centred**: the panel is a disc, and a corner-anchored image (0,0) lies
entirely outside the visible area (app-guide, "Safe area"). A `static` buffer alternative would be package-bounded
like every other static.

Practical limits today:

| | |
|---|---|
| Payload per message | 120 B = 60 pixels; `ceil(w·h·2 / 120) + 2` messages total |
| Examples | 64x64 → 35 messages, 96x96 → 154, 128x128 → 274, full screen → 3 621 |
| Throughput | the default `--pace 0.01` is a lower bound of 10 ms per message plus one BLE round trip per `send` (the watch inbox is only `DEPTH 4`): a 64x64 arrives in roughly a second, a full screen is minutes — treat **<= 128x128 as the comfortable demo range** |
| While paused | `send` is refused (`PAUSED`); restart with `START` after `RESUME` |
| Persistence | none: leaving the app or the watch rebooting loses the picture; baked images are the durable path |
| Protocol gaps (why temporary) | no CRC per chunk, no retransmit, no compression, no scaling, no resume — add before any of this ships as a feature |

Security follows the host-channel rules (`abi-1.4.md`): anyone in BLE range can send, so every field is treated as
untrusted (dimension caps, even offsets, clipping) and the only effect of a hostile chunk is pixels on the screen.
The image content itself is the app owner's responsibility; keep personal pictures out of anything you would not
show on the watch.

## Where the code lives

| File | Change |
|---|---|
| `fw/wp2-core/src/lvcmd.rs` | `Rec::Image` (`0x19`), `blit()` (`0x1B`), `image_src_in()` range check, `Class::Image` |
| `fw/wp2-core/src/gfx.rs` | `Canvas::blit` (clipped row copies) |
| `fw/wp2-loader/src/app.rs` | `blit`: framebuffer copy + dirty box |
| `fw/wp2-loader/src/lv.rs` | `BLIT` and range checks in `submit`, `IMAGE_SRC` execution, `lv_img` construction, descriptors |
| `sdk/cmf-wp2-app/src/lib.rs` | `Bitmap`, `bitmaps!`, `Api::pixels` with rect fallback |
| `sdk/cmf-wp2-app/src/lv.rs` | `Class::Image`, `Batch::image`, `Batch::set_image` |
| `src/wp2apps/bitmaps.py` | PNG -> `src/bitmaps.gen.rs` |
| `src/wp2apps/stream.py` | phone-side sender |
| `sdk/examples/bitmap-gallery`, `bitmap-stream` | demo apps |

## Verification

- Host tests: `wp2-core` (record decode/validation, range checks, blit clipping), `cmf-wp2-app` (record bytes,
  fallback spans), `bitmap-stream` (protocol and row spans), `tests/test_bitmaps.py`, `tests/test_stream.py`.
- Cross-build: both example apps compile for `thumbv7em-none-eabi`, and `wp2-loader` checks.
- Not yet run here: emulator e2e (needs `nix-shell`/`CMF_WATCH_UTILS`) and anything on a physical watch
  (owner's yes required). The 4x4 dither and on-screen appearance are unverified on hardware.

## Testing on the emulator (Windows, verified 2026-11-27)

`docs/emulator-windows.md` is the full recipe: runtime archives, MSYS2 libraries, netsimd,
fixtures, then `scripts/emu-demo-windows.py` to boot the patched NOR, install `loader` +
`bitmap-gallery` + `bitmap-stream` + `bitmap-lv`, launch one, and capture `frame.png`. The
`stream` mode launches `bitmap-stream` and sends `ring.png` through the PC sender with
`--channel-key` over the emulator's H4 link — the same frames the Android companion sends over
BLE.
