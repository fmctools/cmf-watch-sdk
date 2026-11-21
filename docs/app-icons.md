# App icons

An app may carry a 96×96 icon for the loader's menu. Without one, the row shows the app's initial in a grey circle.

## Format

The icon sits at **image offset 0**, in front of the code, so it is covered by the package's `body_crc` and the
stub needs nothing new. The package header's `entry` then points past it (`0x6C10` for a 96×96 icon).

| off | field |
|---|---|
| 0x00 | magic `WP2I` |
| 0x04 | `u8 version` = 1 |
| 0x05 | `u8 cf` = 5, `LV_IMG_CF_TRUE_COLOR_ALPHA` (RGB565 LE + A8, 3 B per pixel) |
| 0x06 | `u16 w`, `u16 h` = 96, 96 |
| 0x0A | `u16` 0 |
| 0x0C | `u32 size` = `w·h·3` = 27 648 |
| 0x10 | pixels, row-major |

The loader accepts only this exact descriptor (`wp2_core::pkg::icon_ok`) and only when it fits in `image_size`;
anything else, a short read or a failed allocation gives the letter. `wp2apps pack` refuses a broken descriptor,
`wp2apps info` prints `"icon"`, and `wp2apps list` reports `"icon": true` for an app whose entry is not 0.

## Building

Put `icon.svg` (96×96 viewBox) next to the crate's `Cargo.toml` and call `cmf_wp2_app::app_icon!();` next to
`entry!`. `scripts/build-app` renders the SVG with `resvg` (from `nix-shell -p resvg` if it is not on `PATH`),
converts it with `python -m wp2apps.icon` (straight alpha, 4×4 ordered dither to RGB565) and passes the result in
`WP2_ICON`; the macro puts it in the `.wp2info` section, which `app.ld` links first.

Style of the bundled icons: a gradient circle that fills the 96 px, a soft top-left sheen, a thin light rim and a
white glyph about 40 px tall with a small shadow, as in the stock app list.

## Rendering

While the menu is built, the loader reads each slot's descriptor right after the header and, if it is valid, the
pixels into a `RES` block aligned to 64: `[64 B][27 648 B pixels]`. The 64 B mimic the stock resource-cache header
(`+8` block size, `+16/+20` w/h, `+24` 3, `+32` pixel address): the emulator's VG-Lite model reads 3-byte pixels
only behind it, the hardware ignores it (`docs/recon.md`, applist-style). The block is synced out of the PSRAM
cache, then shown by `lv_img_create` + `lv_img_set_src` with an `lv_img_dsc_t` kept in the loader block. Blocks are
freed after the menu's objects on view delete. Cost: 27 712 B of `RES` per icon while the menu exists (also while
an app runs from it).


## Showing another installed app

Apps can use `Batch::installed_app_icon(object, slot, IconSize::Native)` with the storage-inspection loader. This is separate from declaring the app's own launcher icon. See [Storage inspection](storage-inspection.md) for capability checks, supported sizes, fallback and lifetime rules.
