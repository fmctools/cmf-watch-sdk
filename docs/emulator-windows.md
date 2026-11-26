# Testing the App Loader on the emulator (Windows)

Verified end to end on 2026-11-27: patched firmware boots in QEMU, the App Loader and the three
bitmap apps install over the emulator's H4 link, an app launches, and `frame.png` is captured —
including a photo streamed into `bitmap-stream`. Everything below is what this machine uses.

## One-time setup

1. **MSYS2 (UCRT64)** at `C:/Users/krtky/msys64` with the emulator's libraries and the tools:

   ```bash
   pacman -S --needed mingw-w64-ucrt-x86_64-glib2 mingw-w64-ucrt-x86_64-pixman mingw-w64-ucrt-x86_64-dtc mingw-w64-ucrt-x86_64-libwinpthread mingw-w64-ucrt-x86_64-zlib mingw-w64-ucrt-x86_64-tk mingw-w64-ucrt-x86_64-gdb-multiarch
   ```

   `tk` provides `wish.exe` (the watch window), `gdb-multiarch` prepares the virtual storage and
   drives `emu.call`, the rest are QEMU's DLLs (a missing one shows as exit `0xC0000135`).

2. **Runtime archives** — download `cmf-emulator-windows-x64.tar.gz` and
   `cmf-emulator-wp2-rom.tar.gz` from the same preview release of
   [fmctools/cmf-watch-utils](https://github.com/fmctools/cmf-watch-utils/releases), then:

   ```powershell
   tar -xf cmf-emulator-windows-x64.tar.gz -C ..\cmf-watch-utils\runtime --strip-components 1
   tar -xf cmf-emulator-wp2-rom.tar.gz -C ..\cmf-watch-utils\runtime --strip-components 1
   ```

   and save the pinned NAND ID file as `runtime/nand_id_all.bin` (URL in the utils "First boot"
   guide). `runtime/` must contain `qemu-system-arm.exe`, `compat.bin`, `provision-nand.elf`.

3. **Fixtures** (virtual NOR/NAND from the stock dump, one time):

   ```powershell
   cd ..\cmf-watch-utils
   $env:PATH = "C:\Users\krtky\msys64\ucrt64\bin;" + $env:PATH
   uv run cmf-watch-utils emu prepare-fixtures --fixtures fixtures --stock dumps\binaries\firmware\JX402_V1.0.0.73.bin --nand-id runtime\nand_id_all.bin --qemu runtime\qemu-system-arm.exe --rom runtime\compat.bin --provisioner runtime --runtime-dir "C:\Users\krtky\msys64\ucrt64\bin" --gdb "C:\Users\krtky\msys64\ucrt64\bin\gdb-multiarch.exe"
   ```

   `fixtures/nor.bin` is also the `STOCK_NOR` the workspace Makefile expects.

4. **netsimd** (the H4 bridge both QEMU and `wp2apps` connect to): from the Android SDK
   repository package `emulator-windows_x64-*.zip`, extract `emulator/netsimd.exe` and
   `emulator/lib64/` into `<workspace>/.work/tools/android-emulator/` (already present here;
   `WP2_NETSIMD` overrides the path).

5. **Firmware and apps** (from the fork root):

   ```powershell
   cargo build --release -p wp2-stub -p wp3-stub -p wp2-loader
   # llvm-objcopy + wp2apps pack/firmware as scripts/build-fw does -> target/app-wp2.bin and
   # target/loader.wp2app (prebuilt copies: <workspace>/.work/artifacts/firmware/)
   bash scripts/build-app bitmap-gallery 1
   ```

## The demo script

`scripts/emu-demo-windows.py` wraps the e2e harness with Windows shims (plain qemu, gdb
data-directory and fresh gdbserver ports, fast QMP retries, lsof-free netsimd helper) and runs
the whole loop:

```powershell
cd projects\cmf-watch-sdk
uv run --locked python scripts\emu-demo-windows.py
uv run --locked python scripts\emu-demo-windows.py stream
```

What it does: builds the cached paired NOR (first run provisions a synthetic pairing secret in
its own boot, then `runs/e2e/paired-nor.bin` is reused), boots it headless with netsimd on a free
HCI port, installs `loader` then the three apps over `wp2apps install --hci-port`, wakes the
watch and closes popups (`ready`), opens the loader menu with a gdb-driven `OPEN_VIEW`, taps a
row, waits, quits, and converts the captured `frame.ppm` to `frame.png`. Output:
`runs/e2e/bitmap-<time>/frame.png`.

Menu rows follow install order: row 0 = `bitmap-gallery` (baked ring asset), row 1 =
`bitmap-stream` (waits for `waiting for photo...`), row 2 = `bitmap-lv`.

## Troubleshooting (all hit while writing this)

| Symptom | Cause / fix |
|---|---|
| QEMU exits `0xC0000135` | MSYS2 DLLs not on PATH: the script adds `ucrt64/bin`; for manual runs pass `--runtime-dir` |
| QMP takes minutes to fail | Windows loopback refuses answer after ~2 s, so the stock 200 retries take 7 min: the script's `WinQmp` uses 12 |
| gdb `NameError`/`gdb._execute_unwinders` | MSYS2 gdb misses its relocated data dir: `--data-directory ucrt64/share/gdb` |
| Second gdb call hangs ("packet error") | gdbserver rebinds die in TIME_WAIT: fresh port per call (the script does `gdbserver none` + `free_port`) |
| `GETSECRET,FAIL` on install | stock NOR has no pairing secret: let the script provision `paired-nor.bin` first (two netsimd instances on one `--instance` exit, so provision before the long-lived one) |
| `loader is not active` | `loader.wp2app` was not installed into the L slot: install it first |
| `emu.call` wedges 120 s | guest mutex timing race: `open_app_retry` retries, register state is untouched at that point |
| `test_sim`/`test_harness` fail with WinError 193 | they execute the `#!/bin/sh` wrappers directly: expected on Windows; the demo script bypasses them |

For a GUI window instead of headless, install `tk` (wish.exe) and drop `--headless` (the harness
gates it behind `WP2_GUI=1`); real installs on a physical watch still need the owner's explicit
approval (AGENTS.md hard rule 1).
