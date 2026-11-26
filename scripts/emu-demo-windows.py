#!/usr/bin/env python3
"""Windows emulator demo: boot the patched NOR, install the loader + bitmap apps, launch one, screenshot.

Run from the repository root (its venv has wp2apps and the tests package):

    uv run --locked python scripts/emu-demo-windows.py           # launch bitmap-gallery, capture frame
    uv run --locked python scripts/emu-demo-windows.py stream    # launch bitmap-stream and stream a photo

Prerequisites (see docs/emulator-windows.md): cmf-watch-utils runtime/ + fixtures prepared, MSYS2
UCRT64 libraries, netsimd on this machine, target/app-wp2.bin and target/loader.wp2app built.
Overrides: WP2_MSYS2 (default C:/Users/krtky/msys64), WP2_NETSIMD (path to netsimd.exe).

Windows shims applied to tests/e2e/harness.py at runtime: plain qemu instead of the sh wrapper,
gdb-multiarch called with its data-directory (and exposed as gdb.exe), a fresh gdbserver port per
session, fast QMP retries, and a netsimd context manager without lsof.
"""
import os
import struct
import subprocess
import sys
import time
import zlib
from contextlib import contextmanager
from pathlib import Path

FORK = Path(__file__).resolve().parents[1]
WS = FORK.parent.parent
UTILS = FORK.parent / "cmf-watch-utils"
QEMU = UTILS / "runtime" / "qemu-system-arm.exe"
MSYS2 = Path(os.environ.get("WP2_MSYS2", r"C:\Users\krtky\msys64"))
UCRT_BIN = MSYS2 / "ucrt64" / "bin"
GDB_DATA = MSYS2 / "ucrt64" / "share" / "gdb"
GDB_DIR = WS / ".work" / "tools" / "bin"
NETSIMD = Path(os.environ.get("WP2_NETSIMD", WS / ".work" / "tools" / "android-emulator" / "emulator" / "netsimd.exe"))
MODE = sys.argv[1] if len(sys.argv) > 1 else "gallery"
PACKAGES = ("loader", "bitmap-gallery", "bitmap-stream", "bitmap-lv")

os.environ["WP2_QEMU"] = str(QEMU)
os.environ["PATH"] = os.pathsep.join([
    str(GDB_DIR),
    str(NETSIMD.parent / "lib64"),
    str(UCRT_BIN),  # qemu's glib/pixman/dtc DLLs
    os.environ["PATH"],
])
sys.path.insert(0, str(FORK))

# gdb-multiarch under the name the harness spawns (CreateProcess needs a .exe)
GDB_DIR.mkdir(parents=True, exist_ok=True)
if not (GDB_DIR / "gdb.exe").is_file() and (UCRT_BIN / "gdb-multiarch.exe").is_file():
    (GDB_DIR / "gdb.exe").write_bytes((UCRT_BIN / "gdb-multiarch.exe").read_bytes())

from tests.e2e import harness  # noqa: E402  (fork checkout on sys.path, env set above)

harness.fast_qemu = lambda directory, qemu=None: QEMU  # Windows: no sh wrapper


class WinQmp(harness.Qmp):
    """Windows loopback refusals take ~2 s here; the stock 200 retries would stall 7 minutes."""

    def __init__(self, port: int) -> None:
        import socket
        last = None
        for _ in range(12):
            try:
                self.sock = socket.create_connection(("127.0.0.1", port), timeout=3)
                break
            except OSError as error:
                last = error
                time.sleep(0.1)
        else:
            raise RuntimeError(f"QMP port {port} did not open: {last}")
        self.stream = self.sock.makefile("rwb", buffering=0)
        self.stream.readline()
        self.request("qmp_capabilities")


harness.Qmp = WinQmp


def win_gdb(self, *commands: str) -> str:
    """Emu.gdb with MSYS2's real data-directory; a fresh gdbserver port dodges TIME_WAIT rebinds."""
    self.qmp.hmp("gdbserver none")
    self.gdb_port = harness.free_port()
    reply = self.qmp.hmp(f"gdbserver tcp::{self.gdb_port}")
    if "error" in reply:
        raise RuntimeError(f"could not open gdbserver: {reply}")
    script = self.run / "e2e.gdb"
    script.write_text("\n".join(["set architecture armv7e-m", f"target remote :{self.gdb_port}",
                                  *commands, "detach"]))
    try:
        result = subprocess.run([str(GDB_DIR / "gdb.exe"), "--batch",
                                 "--data-directory", str(GDB_DATA), "-x", str(script)],
                                capture_output=True, text=True, timeout=120)
    finally:
        self.qmp.hmp("gdbserver none")
    if result.returncode or "communication error" in result.stderr.lower():
        raise RuntimeError(f"gdb failed:\n{result.stdout}{result.stderr}")
    return result.stdout


harness.Emu.gdb = win_gdb


def open_app_retry(emu, row: int, tries: int = 3) -> None:
    """emu.call(OPEN_VIEW) occasionally wedges waiting for the guest mutex; retryable, regs untouched."""
    last = None
    for attempt in range(1, tries + 1):
        try:
            harness.open_app(emu, row, pause=3)
            if emu.peek(harness.STUB_RAM + 6, 1)[0] != 0xFF:
                return
            last = RuntimeError(f"row {row} did not start")
        except (RuntimeError, subprocess.TimeoutExpired, OSError) as error:
            last = error
            print(f"open attempt {attempt}/{tries}: {error}", flush=True)
            time.sleep(3)
    raise last


@contextmanager
def win_netsim():
    """tests.e2e.netsim.netsim without lsof (Windows has none); same netsimd flags."""
    port = harness.free_port()
    proc = subprocess.Popen([str(NETSIMD), "--instance", "71", "--hci-port", str(port),
                             "--no-cli-ui", "--no-web-ui"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        time.sleep(3)
        if proc.poll() is not None:
            raise RuntimeError(f"netsimd exited {proc.returncode}")
        yield port
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except Exception:
            proc.kill()


import tests.e2e.netsim as netsim_mod  # noqa: E402
netsim_mod.netsim = win_netsim


def ppm_to_png(src: Path, dst: Path) -> None:
    data = src.read_bytes()
    magic, dims, maxval, pixels = data.split(b"\n", 3)
    assert magic == b"P6" and maxval.strip() == b"255", magic
    w, h = map(int, dims.split())
    raw = b"".join(b"\0" + pixels[y * w * 3:(y + 1) * w * 3] for y in range(h))
    chunk = lambda kind, body: (struct.pack(">I", len(body)) + kind + body
                                + struct.pack(">I", zlib.crc32(kind + body)))
    png = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b""))
    dst.write_bytes(png)


def main() -> None:
    for need in (QEMU, UTILS / "runtime" / "compat.bin", UTILS / "fixtures" / "nor.bin", NETSIMD):
        if not need.is_file():
            raise SystemExit(f"missing {need} — see docs/emulator-windows.md")
    # one-time: boots stock, sets the pairing secret, dumps runs/e2e/paired-nor.bin (its own
    # netsimd instance; it must finish before the long-running one below starts)
    print("paired NOR (one-time provision boot, cached after this) ...", flush=True)
    nor = harness.patched_paired_nor()

    port = harness.free_port()
    net = subprocess.Popen([str(NETSIMD), "--instance", "71", "--hci-port", str(port),
                            "--no-cli-ui", "--no-web-ui"],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    emu = None
    try:
        time.sleep(3)
        if net.poll() is not None:
            raise SystemExit(f"netsimd exited {net.returncode}")
        print(f"netsimd on {port}", flush=True)

        emu = harness.Emu("bitmap", nor=nor, hci_port=port, seconds=300)
        emu.__enter__()
        emu.wait_uart("launcher enter", 90)
        emu.keep_awake()
        for pkg in PACKAGES:
            print(f"installing {pkg} ...", flush=True)
            try:
                harness.cli_out(["install", FORK / "target" / f"{pkg}.wp2app", "--hci-port", port])
            except RuntimeError:
                print(f"netsimd alive: {net.poll()}", flush=True)
                raise
        harness.ready(emu)
        if MODE == "stream":
            open_app_retry(emu, 1)  # slot 1 = bitmap-stream
            emu.sleep(1)
            print("sending photo ...", flush=True)
            sender = subprocess.run(
                ["uv", "run", "--locked", "python", "-m", "wp2apps.stream",
                 str(FORK / "sdk" / "examples" / "bitmap-gallery" / "bitmaps" / "ring.png"),
                 "--hci-port", str(port), "--pace", "0.005", "--channel-key"],
                cwd=FORK, capture_output=True, text=True, timeout=120)
            if sender.stdout.strip():
                print(sender.stdout.strip(), flush=True)
            if sender.stderr.strip():
                print(sender.stderr.strip(), flush=True)
            if sender.returncode:
                raise RuntimeError(f"stream sender failed ({sender.returncode})")
            emu.sleep(4)
        else:
            open_app_retry(emu, 0)  # menu row 0 = bitmap-gallery
            emu.sleep(6)
        slot = emu.peek(harness.STUB_RAM + 6, 1)[0]
        print(f"running_slot={slot:#x}", flush=True)
    finally:
        if emu is not None:
            try:
                emu.qmp.request("quit")
            except Exception:
                pass
            try:
                emu.vm.wait(timeout=60)
            except Exception:
                emu.vm.terminate()
        net.terminate()
        try:
            net.wait(timeout=5)
        except Exception:
            net.kill()

    run = emu.run
    frame = run / "frame.ppm"
    print(f"run dir: {run}", flush=True)
    print(f"frame.ppm: {frame.is_file()}", flush=True)
    if frame.is_file():
        png = run / "frame.png"
        ppm_to_png(frame, png)
        print(f"png: {png}", flush=True)


if __name__ == "__main__":
    main()
