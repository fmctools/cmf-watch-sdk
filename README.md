# cmf-watch-sdk

Installable apps for the CMF Watch Pro 2, board JX402, stock firmware `1.0.0.73`. A small stub goes
into the stock firmware, a loader lives on NAND, apps are written in Rust and installed over BLE.
The watch is never reflashed to add or remove an app.

Status: proven in the emulator. On a real watch: the stub firmware (OTA), the 1.2 loader and the example apps, see [docs/ota.md](docs/ota.md).

- Design: [docs/specs/2026-11-10-wp2-app-loader-design.md](docs/specs/2026-11-10-wp2-app-loader-design.md)
- Writing an app (quick start, platform limits, API, Mac channel, testing): [docs/app-guide.md](docs/app-guide.md)
- Bitmap assets and streaming images (`Api::pixels`, `Batch::set_image`, `bitmaps!`, wire protocol,
  limits, receiver pattern): [docs/bitmap-assets.md](docs/bitmap-assets.md)
- Testing the loader and apps on Windows: [docs/emulator-windows.md](docs/emulator-windows.md)
- Publishing an app to the FMC store: [cmf-tech-community/apps](https://github.com/cmf-tech-community/apps), built
  with this SDK; the apps in `sdk/examples/` are the SDK's tests and demos
- App ABI 1.1 (host messages, storage, time, battery, screen on): [docs/abi-1.1.md](docs/abi-1.1.md)
- App ABI 1.2 (LVGL apps: command buffers, stock widgets and fonts, events, budgets): [docs/abi-1.2.md](docs/abi-1.2.md)
- herdr agents on the watch (example app + Mac daemon): [docs/herdr-agents.md](docs/herdr-agents.md)
- Plan, part 1: [docs/plans/2026-11-10-wp2-app-loader-foundations.md](docs/plans/2026-11-10-wp2-app-loader-foundations.md)

## Relation to cmf-watch-utils

The emulator, the firmware tooling and the reference firmware come from
[cmf-watch-utils](https://github.com/arnav-kr/cmf-watch-utils). This repository expects a checkout at
`../cmf-watch-utils` with `runtime/` and `fixtures/` set up. None of its code is copied here. Emulator
fixes go upstream, and this repository pins the commit it needs.

## Running in the emulator

Host tools `llvm-objcopy`, `tk` and `gdb` come from `nix-shell -p …`. `make help` lists shortcuts
for the commands below.

Build the patched `app.bin` with the stub and heap reservation, the loader package and an app package:

```bash
nix-shell -p llvm --run 'scripts/build-fw'                   # target/app-wp2.bin, target/loader.wp2app
nix-shell -p llvm --run 'scripts/build-app vibration-lab 1'  # target/vibration-lab.wp2app (name, version)
```

Write `target/app-wp2.bin` into a NOR image and boot it:

```bash
uv run --locked wp2apps nor <stock-nor> --app target/app-wp2.bin --output <out>
scripts/emu --nor <out>    # cmf-watch-utils emu run under nix-shell -p tk
```

Apps install over the stub's own BLE channel through Netsim. Start `netsimd` with an H4 port. The CLI
looks for it at `$WP2_NETSIMD`, default `~/Library/Android/sdk/emulator/netsimd`. Start the emulator
with the same `--hci-port`, then:

```bash
uv run --locked wp2apps install target/loader.wp2app --hci-port P
uv run --locked wp2apps install target/vibration-lab.wp2app --hci-port P   # optional --slot N
uv run --locked wp2apps list --hci-port P
uv run --locked wp2apps remove --slot N --hci-port P
uv run --locked wp2apps send '!time' --hci-port P     # a message to the running ABI 1.1 app
uv run --locked wp2apps listen --count 1 --hci-port P   # its messages as JSON lines
```

On the watch, the loader is the "Apps" row at the end of the stock app list. The row only shows up in
the list style, not in the waterfall style.

## Tests

```bash
uv run --locked python -m unittest discover -s tests -v     # e2e skipped without WP2_E2E=1
(cd sdk && cargo test --target aarch64-apple-darwin)
(cd fw && cargo test -p wp2-core --target aarch64-apple-darwin)
WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e -v'
```

Before the e2e run, build the firmware with `scripts/build-fw` and the apps `hello`, `vibration-lab`,
`motor-hog` and `motor-hold` with `scripts/build-app`. A full run takes about half an hour and logs to
`runs/e2e/`.

Each e2e test starts its own `netsimd` on instance `$WP2_NETSIM_INSTANCE`, default 71. Two `netsimd`
on the same instance stop each other. Give a parallel run, or a run next to an emulator you're using,
its own instance.

## Real watch

`install`, `list` and `remove` take `--address` instead of `--hci-port`. Get the address from
`wp2apps scan`. On macOS it's a CoreBluetooth UUID, not a MAC.

A frame longer than MTU − 3 goes out as a write with response, and the OS turns it into an ATT long
write. That's how installs work at the default MTU of 23.

Firmware image for OTA: `make fw-ota`, see [docs/ota.md](docs/ota.md). `make fw-ota-wp3` builds one for the Watch 3
Pro `1.0.1.77` with only the watch face guard (no loader, no apps; never installed on a watch).

Nothing gets written to a physical watch without the owner's approval.

## Release to FMC

The patched firmware and the App Loader reach users through the [FMC store](https://fmc.tools) (firmware shelf).
Releases go through `.github/workflows/fmc-release.yml`, started by hand: Actions → FMC release → Run workflow,
with `firmware`, `loader` or `both` and an optional store description (markdown, `\n` for a line break; empty keeps
the live text).

1. **draft** runs the unit tests and `cargo test`, builds the image and `loader.wp2app`, checks them (firmware:
   `STUB_VERSION` above the live record's `stub`, board `jx402_01_3089c`, `fwkit verify`; loader: a build other than
   the live one) and uploads a new unpublished record to FMC (`tested: emulator`).
2. The owner flashes the draft from the site. Drafts show only to FMC accounts with `moderator`, with a Draft badge.
3. The owner approves the `fmc-prod` environment. **promote** puts the draft's bytes on the live record
   (`tested: hardware`), deletes the draft and tags the commit: `stub-v<N>` for firmware, `loader-<date>-<sha7>` for the
   loader. Each tag gets a GitHub release: the draft's description, then the commits since the previous tag of its
   kind. That is the changelog FMC Companion shows. The loader's carries `loader.wp2app`; the firmware's carries no
   file, the image is never attached anywhere: it is based on the vendor's.

A firmware release needs a higher `STUB_VERSION` in `fw/wp2-core/src/layout.rs`: FMC offers the update by that
number. The loader is matched by its build CRC. CI signs in to FMC as a `publishers` account
(`FMC_PUBLISHER_EMAIL`, `FMC_PUBLISHER_PASSWORD` secrets) that can only write firmware records. More in
[docs/ota.md](docs/ota.md#release-to-fmc).

## Limits

- The patched firmware adds a row to Settings → Vibration between Standard and Off: "Weak" at 35 %
  (`wp2apps firmware --weak PERMILLE`, `--weak 0` drops it). It is English in every UI language: a translation needs
  `res` edits (strings, scene, a glyph in the roller font). Stock levels
  keep their strength; `--motor PERMILLE` scales every stock vibration instead.
- Packages live in `/NAND:/wp2apps` on the stock FAT volume.
- Addresses are valid for JX402 `1.0.0.73` only, except the watch face guard's, which also exist for the Watch 3
  Pro `1.0.1.77`. Other builds need them re-derived.

## License

MIT, see [LICENSE](LICENSE).
