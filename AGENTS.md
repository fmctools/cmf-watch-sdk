# AGENTS.md

## Mission

Make the CMF Watch Pro 2 run separately installable apps: a minimal stub in the stock
`app.bin`, a loader on NAND, a Rust SDK and apps installed over BLE. The first app is a
vibration lab. See `docs/specs/` for the design and `docs/plans/` for the current plan.

Building an app → read `docs/app-guide.md` first; images → `docs/bitmap-assets.md` (baked `bitmaps/*.png`,
`Api::pixels`, LVGL `IMAGE_SRC`, phone streaming, limits) and `docs/emulator-windows.md` for the Windows
emulator loop. LVGL apps always use the system font and the stock look from
`cmf_wp2_app::stock` (`stock::font`, `stock::theme`, `page`/`title`/`row`): never LVGL's default font or an own
palette unless the owner asks.

## Hard rules

1. No writes to a physical watch without explicit approval from the owner. Read-only BLE
   (`scan`, `info`, known read-only AT queries) is fine. Everything is proven in the emulator first.
2. No firmware binaries, vendor resources, built apps or personal data in commits. Personal data:
   MAC address, serial number, pairing secret, authkey, session key, GPS tracks, notification
   contents. Placeholder MAC: `AA:BB:CC:DD:EE:FF`.
3. The stock OTA sequence is preserved (INIT2 to the OTA characteristic, mandatory end ack).
4. No code is copied from `cmf-watch-utils` (AGPL-3.0); it is used as an external tool only.
   This repository is MIT.
5. Addresses are valid for JX402 `1.0.0.73` only; other builds are re-derived.

## Layout

```text
docs/specs/   design documents
docs/plans/   implementation plans
docs/recon.md stock app.bin findings for the loader (from plan 1, task 1)
docs/storage-inspection.md read-only storage/app-catalog service (Storage::open probes for it)
docs/bitmap-assets.md bitmap assets and phone-to-watch image streaming (wire format, limits, receiver)
docs/emulator-windows.md Windows emulator setup and the scripts/emu-demo-windows.py driver
src/wp2apps/  Python CLI: package, inspect and install apps
sdk/          Rust workspace: cmf-wp2-app crate and example apps
fw/           Rust workspace: wp2-core (shared no_std logic, host-tested), stub and loader
scripts/      emulator and build wrappers
tests/        unittest suite
```

## Environment

- macOS host. Host tools come from `nix-shell -p …`, not Homebrew.
- Windows works too: MSYS2 UCRT64 runtime, gdb-multiarch and netsimd
  (`docs/emulator-windows.md`); `scripts/emu-demo-windows.py` boots the patched NOR, installs the
  loader plus the bitmap examples and screenshots the panel.
- Python 3.12+ with uv. Rust via rustup, target `thumbv7em-none-eabi`.
- `../cmf-watch-utils` checkout with `runtime/` (QEMU + ROM), `fixtures/` (incl. `wfc-index.txt`)
  and `dumps/binaries/firmware/JX402_V1.0.0.73.bin`. Override with `CMF_WATCH_UTILS=/path`.
- The emulator viewer needs Tk 8.6+: run it under `nix-shell -p tk`; macOS `/usr/bin/wish` (8.5)
  shows a blank window.

## Verification

```bash
uv run --locked python -m unittest discover -s tests -v
(cd sdk && cargo test --target aarch64-apple-darwin)
(cd fw && cargo test -p wp2-core --target aarch64-apple-darwin)
```

