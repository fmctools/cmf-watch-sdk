# Firmware image for OTA

`make fw-ota VERSION_CODE=0x10003` builds `target/fw-wp2.bin`. It is the stock `1.0.0.73` image with the patched
`app.bin` (stub, heap reservation, a fourth vibration level at 35 %; see `wp2apps firmware --help`). All other
partitions (`res`, `fonts`, `res_e`, `sdfs_k`, AGPS trailer) are stock. `make fw-motor` uses the same builder for
the motor-only image.

The patched `app.bin` also labels Settings > About as `<version>-fmc` (`wp2apps firmware --no-fmc-label` leaves it
out), so custom firmware is easy to identify on the watch itself. Every machine-readable version stays stock:
`AT GETVERSION`, the BLE version replies and the OTA `version_code` (docs/recon.md "About page label").

## How it is built (`wp2apps ota-image`)

- The stub grows `app.bin` from `0x25A080` to `0x25CDE8` bytes, so `fwkit edit-app` (in-place patches) cannot carry
  it. Instead the builder unpacks the outer AOTA and the inner `TEMP.bin` AOTA with `fwkit unpack`, replaces
  `app.bin` and its manifest entry (`file_size`, `orig_size`, `checksum` = CRC32), then repacks both with `fwkit pack`.
- `TEMP.bin` is stored as `LZMA` chunks: `b"LZMA"`, `u32 16`, `u32 packed`, `u32 size`, then an XZ stream (preset 6,
  2 MiB chunks). Re-encoding the stock TEMP gives the stock bytes exactly (`tests/test_firmware.py`).
- In the outer manifest, `TEMP.bin`'s `file_size`, `orig_size` and `checksum` all describe the stored bytes, as in stock.
- **AOTA data CRC.** Header `+0x18` is the CRC32 of `[u16 +0x12, u32 +0x14)`, and the header CRC at `+0x04` covers it.
  `fwkit pack` and `edit-app` leave `+0x18` stale. Before `ota-image` fixed it (`firmware.fix_aota_crc`), every
  image we built had a stale one, including `fw-motor.bin` / `fw-motor-v10001.bin` from 2026-11-11. The emulator's
  OTA client rejects such an image. Whether the watch checks this field is unknown.
- `--version-code` sets the outer manifest's `version_code` (stock `0x10000`).
- `--temp-only` keeps only `ota.xml` and `TEMP.bin` and drops the AGPS trailer. `cmf-watch-utils emu ota` accepts
  only this form. Its `TEMP.bin` is byte-identical to the one in the full image.

## Watch 3 Pro (1.0.1.77)

`make fw-ota-wp3` builds `target/fw-wp3.bin`: the stock `1.0.1.77` image with only the watch face guard
(`fw/wp3-stub`, docs/recon.md "Watch face boot guard"). No loader, no apps. It needs
`dumps/binaries/firmware/JX402_w3pro_V1.0.1.77.bin` in cmf-watch-utils; the commit the release workflow pins has
none, so `scripts/build-fw` leaves the model out there and FMC gets no 3 Pro firmware. Never installed on a watch,
and the emulator cannot install it either: flash only with the owner's approval.

## Emulator proof (2026-11-12)

```bash
uv run --locked wp2apps ota-image --app target/app-wp2.bin --temp-only --version-code 0x10002 --output target/fw-wp2-temp.bin
scripts/emu-ota target/fw-wp2-temp.bin runs/ota      # stock OTA over Netsim, then the emulator's install
WP2_NOR=runs/ota/install/nor.bin WP2_E2E=1 nix-shell -p gdb llvm --run \
  'uv run --locked python -m unittest tests.test_lv_e2e tests.test_e2e.HelloTest tests.test_e2e.MotorTest -v'
```

- The virtual stock watch took the whole transfer and answered `watch_reported_image_valid`.
- `emu recover` installed the staged update. The `app.bin` in the resulting NOR is byte-identical to
  `target/app-wp2.bin`.
- The e2e runs above passed 14/14 on that NOR.
- Limits:
  - The install step is the emulator's host model, not the watch's bootloader.
  - The full image's other partitions were not transferred in the emulator. They are stock bytes.

## Real watch (2026-11-12, owner's approval)

Installed `fw-wp2.bin` (`VERSION_CODE=0x10002`) with the stock client of cmf-watch-utils
(`tools/ota_client.py --image …`), then the loader and apps with `wp2apps install --address`.

- The watch pulled only what differs: TEMP.bin and a short range near the end, 1.6 MB of 57 MB, in 1.5 min. Then
  `A041 01` came, `0x9041 A5` (END) was sent, and the watch installed the update and rebooted. `AT GETVERSION` still
  reads `1.0.0.73`.
- **A stale authkey makes INIT2 reboot the watch.** With the authkey from an older bind, `A052` came but INIT2 got no
  `A040`, and the watch rebooted (no data had been sent; it came back on stock). Every bind, by the phone too, makes a
  new random authkey. `cmf_watch.py pair` (a fresh bind) fixed it, and `--stop-after-a040` confirmed it before
  the transfer.
- **macOS keeps the watch connected after `wp2apps install`.** After one or two installs, the system showed the watch
  as `Connected` with no client running, the watch stopped advertising, and the next install found nothing.
  Switching the Mac's Bluetooth off and on (`blueutil -p 0; blueutil -p 1`) released it.
- Turn off the phone's Bluetooth during the update so it does not bind in between.

## Release to FMC

`.github/workflows/fmc-release.yml` (manual run): builds, tests and checks, then uploads a draft record to FMC
(`scripts/publish-fmc draft`). A moderator account sees the draft on the site and flashes it from there. When the owner
approves the `fmc-prod` environment, `promote` reads the image back from the draft, checks its sha256 against the
draft job's, moves those exact bytes onto the live record and tags the commit (`stub-v<N>`, `loader-<date>-<sha7>`).
First release this way: `stub-v5` and `loader-2026-11-15-f52cca1`.
