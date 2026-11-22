"""Stub 9 widget templates (docs/widget-push.md, docs/app-icons.md): widget-columns writes a `columns` card with
its own icons through the loader; a host PUSHes one for hello (no icons); malformed records on NAND show the
package name and nothing faults. An icon the package does not have (no trailer, a corrupt one, fewer icons, a
package installed on stub 8 and kept over the update) costs that icon, not the card.
Needs target/app-wp2.bin (stub 9), target/loader.wp2app, widget-columns and hello (scripts/build-app). Frames go
to runs/widget-templates/."""
import asyncio
import json
import os
from pathlib import Path
import unittest
from unittest import skipUnless

from tests.e2e import harness, netsim
from tests.test_demo_e2e import save_png
from wp2apps import cli as wp2, firmware, install, package, paths

OUT = paths.ROOT / "runs" / "widget-templates"
X, W, H = 61, 344, 166
TOP, BOTTOM = 61, 239
STUB8 = os.environ.get("WP2_STUB8_APP")  # app.bin built from tag stub-v8, see test_widget_stub8_e2e.py
REC = install.columns_record([("Now", 0, "21°"), ("10:00", 1, "19°"), ("11:00", 2, "-3°")])
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


def texts(frame, y0):
    """the caption and value bands of a tile (clear of the icons and the rounded corners)"""
    return crop(frame, y0 + 14, X + 20, 304, 40) + crop(frame, y0 + 110, X + 20, 304, 40)


def session(port, action):
    return asyncio.run(wp2.with_watch(type("Args", (), {"hci_port": int(port), "address": None})(), action))


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
                                            (13, b"\x10"),         # icon 16: no package has one
                                            (13, b"\xfe"),         # icon 254
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
            self.assertTrue(crop(frame, TOP) != crop(names, TOP), "the card, not the package name")
            self.assertEqual(lit(icon_box(frame, TOP, 0, 1)), 0, "the pixels are on NAND, the trailer is not valid")

    def same_card(self, frame, y0, full, icons, what):
        """the tile has the text of `full` (a tile with all three icons) and only the icons in `icons`"""
        self.assertTrue(texts(frame, y0) == texts(full, TOP), f"{what}: caption and value")  # not assertEqual: a 70 KB diff
        for col in range(3):
            got = icon_box(frame, y0, col)
            if col in icons:
                self.assertTrue(got == icon_box(full, TOP, col), f"{what}: icon {col}")
            else:
                self.assertEqual(lit(got), 0, f"{what}: no icon in column {col}")

    def test_missing_icons_keep_the_card(self):
        OUT.mkdir(parents=True, exist_ok=True)
        with harness.installed(["loader", "widget-columns"], name="widget-missing") as (emu, port):
            full = (paths.ROOT / "target" / "widget-columns.wp2app").read_bytes()
            app = package.parse(full)
            plain = package.strip_trailer(full)
            damaged = full[:-1] + bytes([full[-1] ^ 0xFF])
            one = package.pack(app.image, name=app.name, version=app.version, entry=app.entry, relocs=app.relocs,
                               abi_minor=app.abi_minor, widget_icons=app.widget_icons[:1])

            async def prepare(s):
                for slot, data in ((1, plain), (2, damaged), (3, one)):
                    await install.upload(s, slot, data)
                for slot in range(4):
                    await install.push(s, slot, install.PUSH_WIDGET, REC)
            session(port, prepare)
            cards = lambda a, b: harness.cli_out(["cards", "set", f"widgets:0x40/{a + 1},0x40/{b + 1}", "--hci-port", port])

            cards(0, 1)
            frame = whole = self.shot(emu, "5-stripped")
            for col in range(3):
                self.assertGreater(lit(icon_box(whole, TOP, col)), 200, f"the reference has icon {col}")
            self.same_card(frame, BOTTOM, whole, (), "stripped package")

            cards(2, 3)
            frame = self.shot(emu, "6-damaged-and-fewer")
            self.same_card(frame, TOP, whole, (), "damaged trailer")
            self.same_card(frame, BOTTOM, whole, (0,), "one icon, three named")

            # count 1 in the head with all three icons' pixels still behind it: a read past the count would show them
            harness.write_file(emu, b"/NAND:/wp2apps/0.pkg", len(plain) + 5, b"\x01")
            cards(0, 1)
            frame = self.shot(emu, "7-count-below-index")
            self.same_card(frame, TOP, whole, (0,), "pixels past the count are not read")

    @skipUnless(STUB8, "set WP2_STUB8_APP (app.bin built from tag stub-v8)")
    def test_update_from_stub8_keeps_the_card(self):
        """Installed under stub 8, then the same NOR data and NAND boot with stub 9's app.bin; nothing reinstalled."""
        OUT.mkdir(parents=True, exist_ok=True)
        nor = firmware.stage_nor(netsim.paired_nor(), Path(STUB8).read_bytes())
        with netsim.netsim() as port, harness.Emu("update-stub8", nor=nor, hci_port=port) as emu:
            emu.wait_uart("launcher enter", 60)
            emu.keep_awake()
            for name in ("loader", "widget-columns"):
                harness.cli_out(["install", paths.ROOT / "target" / f"{name}.wp2app", "--hci-port", port])
            harness.cli_out(["cards", "set", "widgets:0x40/1", "--hci-port", port])
            self.assertEqual(json.loads(harness.cli_out(["list", "--hci-port", port]))["stub"]["version"], 8)
            harness.open_app(emu, 0)
            emu.send("c 1", "c 0", pause=3)
            emu.send("c 1", "c 0", pause=3)
            harness.ready(emu)
            emu.sleep(5)  # settings and FAT are written behind

            harness.swap_app(emu, (paths.ROOT / "target" / "app-wp2.bin").read_bytes())
            listing = json.loads(harness.cli_out(["list", "--hci-port", port]))
            self.assertEqual(listing["stub"]["version"], 9)
            self.assertEqual(listing["slots"]["0"]["name"], "widget-columns", "the same NAND")
            harness.ready(emu)
            self.assertEqual(harness.read_file(emu, b"/NAND:/wp2apps/0.wgt", 128),
                             install.widget_record("Forecast", "21º"), "the record from stub 8 is still there")

            # the phone's PUSH first (it needs no new loader), then the app's own record through the loader
            harness.cli_out(["widget", "set", "0", "--columns", "Now:8:17°", "10:00:9:16°", "--hci-port", port])
            frame = self.shot(emu, "8-update-pushed")
            self.assertGreater(lit(texts(frame, TOP)[304 * 40 * 3:]), 100, "values drawn, not the package name")
            for col in range(2):
                self.assertEqual(lit(icon_box(frame, TOP, col, 2)), 0, "stored stripped: no icons")

            harness.open_app(emu, 0)
            emu.send("c 1", "c 0", pause=3)
            self.assertEqual(harness.read_file(emu, b"/NAND:/wp2apps/0.wgt", 128), REC)
            frame = self.shot(emu, "9-update-app")
            self.assertEqual(glyph_rows(frame, 78, 112), (86, 102))
            self.assertEqual(glyph_rows(frame, 174, 216), (184, 200), "values drawn, not the package name")
            for col in range(3):
                self.assertEqual(lit(icon_box(frame, TOP, col)), 0, "stored stripped: no icons")


if __name__ == "__main__":
    unittest.main()
