"""Stub 9 widget templates (docs/widget-push.md, docs/app-icons.md): widget-columns writes a `columns` card with
its own icons through the loader; a host PUSHes one for hello (no icons); malformed records on NAND show the
package name and nothing faults; a package whose icon trailer is corrupt still installs and draws no icons.
Needs target/app-wp2.bin (stub 9), target/loader.wp2app, widget-columns and hello (scripts/build-app). Frames go
to runs/widget-templates/."""
import asyncio
import json
import unittest
from unittest import skipUnless

from tests.e2e import harness
from tests.test_demo_e2e import save_png
from wp2apps import cli as wp2, install, paths

OUT = paths.ROOT / "runs" / "widget-templates"
X, W, H = 61, 344, 166
TOP, BOTTOM = 61, 239
BG = (26, 28, 31)  # the tile, as the emulator frame reads RGB565 0x2966


def crop(frame, y0, x0=X, w=W, h=H):
    fw, _, rgb = frame
    return b"".join(rgb[(y * fw + x0) * 3:(y * fw + x0 + w) * 3] for y in range(y0, y0 + h))


def lit(pixels: bytes) -> int:
    """pixels that are not the tile grey"""
    return sum(1 for i in range(0, len(pixels), 3) if max(abs(pixels[i + c] - BG[c]) for c in range(3)) > 12)


def icon_box(frame, y0, col, cols=3):
    """the 40x40 icon of column `col`: x 20 + col * 320 / cols, centred vertically on the tile"""
    return crop(frame, y0 + 63, X + 20 + col * (320 // cols), 40, 40)


def glyph_rows(frame, y0, y1, x0=79, x1=157):
    """(first, last) screen row with text in the box"""
    w, _, rgb = frame
    rows = [y for y in range(y0, y1) if max(rgb[(y * w + x0) * 3:(y * w + x1) * 3]) > 70]
    return rows[0], rows[-1]


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd, gdb and llvm-nm)")
class WidgetTemplatesTest(unittest.TestCase):
    def shot(self, emu, name):
        """the face laid out again (the loader view deletes the cards, the crown brings them back), then card 1"""
        harness.ready(emu)  # a BLE command before: wait out its screen-off, or the watch sleeps deep and QEMU stops
        emu.call(harness.OPEN_VIEW, 0xF0, 0, thread=b"ui_s")
        emu.sleep(2)
        emu.send("c 1", "c 0", pause=3)
        harness.swipe(emu, 420, 233, 40, 233, steps=4)
        frame = emu.frame()
        save_png(OUT / f"{name}.png", *frame)
        emu.send("c 1", "c 0", pause=3)
        self.assertNotIn("FATAL", emu.uart().upper(), name)
        return frame

    def test_columns_cards(self):
        OUT.mkdir(parents=True, exist_ok=True)
        with harness.installed(["loader", "widget-columns", "hello"], name="widget-templates") as (emu, port):
            cli = lambda *a: harness.cli_out([*a, "--hci-port", port])
            self.assertIn('"version": 9', cli("list").replace(" ", " "))
            cli("cards", "set", "widgets:0x40/1,0x40/2")
            harness.ready(emu)
            names = self.shot(emu, "0-names")

            # the app writes its record through the loader (op 0x14, 66 B)
            harness.open_app(emu, 0)
            emu.send("c 1", "c 0", pause=3)  # BUTTON: the app returns
            rec = install.columns_record([("Now", 0, "21°"), ("10:00", 1, "19°"), ("11:00", 2, "-3°")])
            self.assertEqual(harness.read_file(emu, b"/NAND:/wp2apps/0.wgt", 128), rec)
            cols = self.shot(emu, "1-columns")
            self.assertNotEqual(crop(cols, TOP), crop(names, TOP))
            # where stock widget 97 has them (runs of wgt_reg 97 on the same tile, docs/recon.md)
            self.assertEqual(glyph_rows(cols, 78, 112), (86, 102), "caption rows as in 97")
            self.assertEqual(glyph_rows(cols, 174, 216), (184, 200), "value rows as in 97")
            for c in range(3):
                self.assertGreater(lit(icon_box(cols, TOP, c)), 200, f"icon {c} not drawn")

            # the phone's PUSH for hello, which has no widget icons
            pushed = install.columns_record([("Now", None, "1°"), ("12:00", None, "2°")])
            cli("widget", "set", "1", "--columns", "Now::1°", "12:00::2°")
            self.assertEqual(harness.read_file(emu, b"/NAND:/wp2apps/1.wgt", 128), pushed)
            frame = self.shot(emu, "2-pushed")
            self.assertNotEqual(crop(frame, BOTTOM), crop(names, BOTTOM))
            self.assertEqual(lit(icon_box(frame, BOTTOM, 0, 2)), 0, "no icon")

            # malformed records on NAND (same length, bytes changed in place): the package name, no fault
            for n, (at, data) in enumerate([(1, b"\x07"),          # unknown template
                                            (2, b"\x03"),          # n says 3, two columns follow
                                            (13, b"\x00"),         # icon 0 of a package without icons
                                            (3, b"x" * 10),        # caption without a NUL
                                            (14, b"\xc3" + bytes(9))]):  # cut UTF-8
                harness.write_file(emu, b"/NAND:/wp2apps/1.wgt", 0, pushed)
                harness.write_file(emu, b"/NAND:/wp2apps/1.wgt", at, data)
                frame = self.shot(emu, f"3-malformed-{n}")
                self.assertEqual(crop(frame, BOTTOM), crop(names, BOTTOM), f"malformed {n}")

            # a corrupt trailer (a pixel flipped, crc kept): installs, the stub zeroes it, no icons
            data = bytearray((paths.ROOT / "target" / "widget-columns.wp2app").read_bytes())
            data[-1] ^= 0xFF
            # wp2apps refuses it (package.parse), so straight BEGIN/DATA/END: what a careless host would send
            asyncio.run(wp2.with_watch(type("Args", (), {"hci_port": int(port), "address": None})(),
                                       lambda s: install.upload(s, 2, bytes(data))))
            self.assertEqual(json.loads(cli("list"))["slots"]["2"]["name"], "widget-columns", "installed")
            cli("widget", "set", "2", "--columns", "Now:0:5°")  # icon 0: valid only with a trailer
            cli("cards", "set", "widgets:0x40/3,0x40/2")
            harness.ready(emu)
            frame = self.shot(emu, "4-bad-trailer")
            self.assertEqual(crop(frame, TOP), crop(names, TOP), "icon 0 without icons: the package name")


if __name__ == "__main__":
    unittest.main()
