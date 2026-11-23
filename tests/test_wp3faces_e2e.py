"""Watch 3 Pro faces on the patched WP2 app.bin (docs/recon.md, "Watch 3 Pro faces"): the procedural widget types
4..7 (tag 0x82 + 0x5C) must not fault WFManager. The faces are the owner's files in WP2_WP3_FACES (default
~/Downloads/wp3-faces-for-wp2), never committed. A stock run imports the face into a virtual NAND, the patched app
boots from that NAND. Needs target/app-wp2.bin from scripts/build-fw and cmf-watch-utils runtime/p10."""
import functools
import hashlib
import json
import os
import shlex
import shutil
import struct
import subprocess
import unittest
from pathlib import Path
from unittest import skipUnless

from fwkit import lz4

from tests.e2e import harness
from wp2apps import paths

FACES = Path(os.environ.get("WP2_WP3_FACES") or Path.home() / "Downloads/wp3-faces-for-wp2")
APP = paths.ROOT / "target/app-wp2.bin"
# no 0x82 widget of a new type: boots the same before and after the patch
HOST_TARGET = os.environ.get("WP2_HOST_TARGET") or "aarch64-apple-darwin"
CONTROL = "wp2-ported/fmc-cluster-clock.bin"
TYPES = {4: "wp2-ported/stock-rainbow-lollipop.bin", 5: "wp3-original/stock-briqbot.bin",
         6: "wp2-ported/stock-datapulse_BOOTLOOP.bin", 7: "wp2-ported/stock-luminous-beam_BOOTLOOP.bin"}


def emulate(out: Path, fixtures: Path, seconds: int, *extra) -> subprocess.CompletedProcess:
    cmf = paths.cmf_watch_utils()
    qemu = out.parent / "qemu"
    # QEMU_WP2_MODEL makes this QEMU pick another board model than the wp2-stock profile expects
    qemu.write_text(f'#!/bin/sh\nunset QEMU_WP2_MODEL\nexec {shlex.quote(str(cmf / "runtime/p10/qemu-system-arm"))} "$@"\n')
    qemu.chmod(0o755)
    return subprocess.run(["uv", "run", "cmf-watch-utils", "emu", "run", "--profile", "wp2-stock",
                           "--fixtures", str(fixtures), "--qemu", str(qemu), "--rom", str(cmf / "runtime/p10/compat.bin"),
                           *map(str, extra), "--allow-reboot", "--headless", "--seconds", str(seconds),
                           "--audio", "none", "--output", str(out)],
                          cwd=cmf, capture_output=True, text=True, timeout=600)


@functools.cache
def etc2_textures(face: Path) -> str:
    """Texture index lines of the face's format 28 images as the stub decodes them. The emulator's VG-Lite model
    tells a packed RGB565 + alpha texture from ARGB8888 by these hashes only, and the import lists cf 5 alone."""
    data = face.read_bytes()
    at, end = 39 + struct.unpack_from("<H", data, 37)[0], len(data) - 36
    lines = ""
    while at + 8 <= end:
        head, size = struct.unpack_from("<II", data, at)
        if head & 0x1F == 28:
            w, h = head >> 10 & 0x7FF, head >> 21
            blocks = lz4.decompress(data[at + 8:at + 8 + size], (w + 3) // 4 * ((h + 3) // 4) * 16)
            pixels = subprocess.run(["cargo", "run", "-q", "-p", "wp2-core", "--example", "etc2", "--target",
                                     HOST_TARGET, "--", str(w), str(h)], cwd=paths.ROOT / "fw", input=blocks,
                                    capture_output=True, check=True).stdout
            lines += f"\n{w}:{h}:{hashlib.sha256(pixels).hexdigest()}"
        at += 8 + size
    return lines


def boot_face(face: Path, app: Path, seconds: int = 45, keep_nand: bool = False) -> tuple[int, int, Path]:
    """(boots, faults, run directory) of `app` with `face` as the default face. The run's 138 MB NAND copy goes
    afterwards unless keep_nand (for a second boot from the run's fixtures)."""
    cmf = paths.cmf_watch_utils()
    root = harness.RUNS / f"wp3faces-{face.stem}-{harness.time.strftime('%H%M%S')}"
    root.mkdir(parents=True)
    # the CLI imports a face only for the stock app; its nand.bin holds it whatever the stock does with it
    imported = emulate(root / "import", cmf / "fixtures", 3, "--watch-face", face)
    if not (root / "import/wfc-index.txt").is_file():
        raise AssertionError(f"no import of {face.name}: {imported.stderr[-800:]}")
    fixtures = root / "fixtures"
    fixtures.mkdir()
    for name, target in (("nor.bin", cmf / "fixtures/nor.bin"), ("nand-id.bin", cmf / "fixtures/nand-id.bin"),
                         ("nand-defaults.bin", root / "import/nand.bin")):
        (fixtures / name).symlink_to(target)
    index = (root / "import/wfc-index.txt").read_text() + etc2_textures(face)
    (fixtures / "wfc-index.txt").write_text(index)
    ran = emulate(root / "patched", fixtures, seconds, "--app", app)
    if not keep_nand:
        for nand in root.glob("*/nand.bin"):
            nand.unlink()
    log = root / "patched/uart0.log"
    if not log.is_file():
        raise AssertionError(f"no run of {face.name}: {ran.stderr[-800:]}")
    uart = log.read_text(errors="replace")
    return uart.count("*** Booting Zephyr"), uart.count("ZEPHYR FATAL ERROR"), root / "patched"


def lit(frame: Path, left: int, top: int, right: int, bottom: int) -> int:
    """Non-black pixels of the frame in that box."""
    _, size, _, rgb = frame.read_bytes().split(b"\n", 3)
    w = int(size.split()[0])
    return sum(max(rgb[3 * (y * w + x):3 * (y * w + x) + 3]) > 24
               for y in range(top, bottom) for x in range(left, right))


@skipUnless(harness.E2E and FACES.is_dir() and APP.is_file(),
            "set WP2_E2E=1 and WP2_WP3_FACES, build target/app-wp2.bin (scripts/build-fw)")
class Wp3FacesTest(unittest.TestCase):
    def check_face(self, name: str):
        boots, faults, run = boot_face(FACES / name, APP)
        print(json.dumps({"face": name, "boots": boots, "faults": faults, "run": str(run)}), flush=True)
        self.assertEqual((boots, faults), (1, 0))

    def test_control(self):
        self.check_face(CONTROL)

    def test_type4_rainbow_lollipop(self):
        self.check_face(TYPES[4])

    def test_type5_briqbot(self):
        self.check_face(TYPES[5])

    def test_type6_datapulse(self):
        self.check_face(TYPES[6])

    def test_type7_luminous_beam(self):
        self.check_face(TYPES[7])

    def test_datapulse_panels_are_drawn(self):
        boots, faults, run = boot_face(FACES / TYPES[6], APP, 8)  # the screen goes dark at about 9 s
        self.assertEqual((boots, faults), (1, 0))
        frame = run / "frame.ppm"
        # the face itself has nothing in these boxes: the hourly weather and the chart of the procedural widget
        self.assertGreater(lit(frame, 80, 45, 400, 175), 200, "no weather panel")
        self.assertGreater(lit(frame, 75, 295, 390, 420), 100, "no chart panel")

    def test_briqbot_is_drawn(self):
        boots, faults, run = boot_face(FACES / TYPES[5], APP, 8)
        self.assertEqual((boots, faults), (1, 0))
        # without the widget only the hands are there
        self.assertGreater(lit(run / "frame.ppm", 0, 0, 466, 466), 20000, "no robot")

    @skipUnless(harness.QEMU.is_file() and shutil.which("gdb"), "set WP2_QEMU and put gdb on PATH")
    def test_lollipop_ring_lights_up(self):
        # the stock's parser drops this widget (no images): the rings are the face's background
        *_, run = boot_face(FACES / TYPES[4], APP, 3, keep_nand=True)
        self.addCleanup((run.parent / "import/nand.bin").unlink, missing_ok=True)
        with harness.Emu("wp3lollipop", nor=harness.patched_paired_nor(), fixtures=run.parent / "fixtures") as emu:
            emu.wait_uart("launcher enter", 90)
            emu.keep_awake()
            emu.sleep(3)

            def ring() -> int:
                w, _, rgb = emu.frame()
                # the ring 181..185 px from the centre, above it
                return max(rgb[3 * (y * w + x)] for y in range(48, 53) for x in range(225, 242))

            idle = ring()
            emu.send("t 1 233 50", pause=0.25)
            down = ring()
            emu.send("t 0 233 50", pause=0.25)
            self.assertGreater(down, idle + 40, "the ring under the finger is not lit")
            self.assertEqual(emu.uart().count("ZEPHYR FATAL ERROR"), 0)

    def test_etc2_images_are_drawn(self):
        # format 28: the hands, time and day of Vive Lite, the dial's numerals of Briqbot
        boots, faults, run = boot_face(FACES / "wp3-original/stock-vive-lite.bin", APP, 8)
        self.assertEqual((boots, faults), (1, 0))
        self.assertGreater(lit(run / "frame.ppm", 180, 180, 286, 286), 300, "no hands")
        boots, faults, run = boot_face(FACES / TYPES[5], APP, 8)
        self.assertEqual((boots, faults), (1, 0))
        # "12" above the robot is lighter than the dial
        _, size, _, rgb = (run / "frame.ppm").read_bytes().split(b"\n", 3)
        w = int(size.split()[0])
        light = sum(rgb[3 * (y * w + x)] > 90 for y in range(40, 65) for x in range(215, 251))
        self.assertGreater(light, 40, "no numerals")

    @skipUnless(harness.QEMU.is_file() and shutil.which("gdb"), "set WP2_QEMU and put gdb on PATH")
    def test_theme_source(self):
        # Eleginn's source 0x99, asked of the dispatcher itself: its original stops the emulator on A8 images
        dispatch, out = 0x100DAA91, harness.STUB_RAM + 0xE08  # next to harness.Emu.call's own scratch word
        with harness.Emu("wp3source", nor=harness.patched_paired_nor()) as emu:
            emu.wait_uart("launcher enter", 90)
            emu.keep_awake()
            saved = emu.peek(out, 4)
            emu.poke(out, b"\xff" * 4)
            self.assertEqual(emu.call(dispatch, 0x99, 1, out, 0, thread=b"ui_s") & 0xFFFFFFFF, 0)
            self.assertLess(struct.unpack("<I", emu.peek(out, 4))[0], 10, "not a theme index")
            for unknown in (0x8E, 0x98, 0x9A):
                self.assertEqual(emu.call(dispatch, unknown, 1, out, 0, thread=b"ui_s") & 0xFFFFFFFF, 0xFFFFFFF0)
            emu.poke(out, saved)

    def test_luminous_beam_calories(self):
        boots, faults, run = boot_face(FACES / TYPES[7], APP, 8)
        self.assertEqual((boots, faults), (1, 0))
        _, size, _, rgb = (run / "frame.ppm").read_bytes().split(b"\n", 3)
        w = int(size.split()[0])
        dark = sum(max(rgb[3 * (y * w + x):3 * (y * w + x) + 3]) < 100 for y in range(330, 356) for x in range(240, 290))
        # the dial is light there, the digits before "%" are dark
        self.assertGreater(dark, 40, "no calories")


if __name__ == "__main__":
    unittest.main()
