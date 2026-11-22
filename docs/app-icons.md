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

## Widget icons (stub 10)

An app may also ship up to 16 icons for its `columns` widget card (`docs/app-guide.md`, "Columns card"). They do not
go into the image (the loader reads the whole image into RAM at every start): they are a **trailer after the
relocation table**.

| off | field |
|---|---|
| 0x00 | magic `WP2W` |
| 0x04 | `u8 version` = 1 |
| 0x05 | `u8 count`, 1..16 |
| 0x06 | `u8 size` = 40 (stock widget 97's icons are 40×40 CF 5, measured in the emulator) |
| 0x07 | `u8` 0 |
| 0x08 | `u32 crc32` of the pixel bytes after this header |
| 0x0C | `count` × 40·40·3 = 4 800 B pixels, CF 5 as above, icon `i` at `0x0C + i·4800` |

The package is `header ‖ image ‖ 4·reloc_count ‖ trailer`; the header's sizes and `body_crc` do **not** cover the
trailer (a deviation from the design, which had `body_crc` cover it):

- `body_crc` stays the image and relocations, so every loader, old or new, loads the app exactly as before and never
  reads the trailer.
- Stub 10 accepts the extra bytes at `END` and checks the trailer with its own CRC. A bad trailer (head, length or
  CRC) has its magic zeroed: the app still installs and runs, its card just has no icons.
- A column that names an icon the package does not have (no trailer, a bad one, fewer icons) is drawn with its caption
  and value and no icon; nothing is read for it (`wp2_core::widget::icon_at`). An app installed on stub 8 or 9 is stored
  stripped, so after the update to stub 10 its card has no icons until the app is installed again.
- A stub before 10 refuses any package longer than `64 + image + 4·relocs` (`Invalid`). Hosts send such a stub the
  package cut at that length, which is byte for byte the package without icons (`package.strip_trailer`;
  `wp2apps install` does it, the companion must too). Old stubs draw no `columns` cards anyway.
- The card reads the trailer head at create (one seek), and only the icons the record names, each into its own
  `RES` block (`[64 B][4 800 B][dsc]`, as below), freed with the card. A missing or short read leaves that column
  without an icon.
- Cost: 4 956 B of `RES` per icon drawn (4 876 B block + allocator). Emulator, one boot, the heap's low-water mark
  (`HEAP + 0xC`) reset before each of: no app cards 3.53 MiB free; 16 tiles with only names 2.05 MiB; 16 `columns`
  tiles of 4 icons each 1.90 MiB. The difference of the last two is 158 592 B, 32 icons: the icons of 8 tiles live at
  once (14 of 16 are shown, the stock has 7 card pages). No cards again: back to 3.53 MiB, no fault.

Building: `widget-icons/NN-name.svg` (40×40 viewBox) next to `Cargo.toml`; `scripts/build-app` renders each with
`resvg` and calls `wp2apps pack --widget-icons <dir of PNGs>`, indices in file name order. `pack` and `parse` refuse
a broken trailer; `wp2apps info` prints `"widget_icons": N`.

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
