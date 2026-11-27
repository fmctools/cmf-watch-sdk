"""What a watch still on stub 8 or 9 (the watch face guard) does with stub 10's packages and records
(docs/app-icons.md, docs/widget-push.md), and what the loaders released before the templates do with them: run
before the stub 10 OTA.

Needs WP2_STUB8_APP and WP2_STUB9_APP, patched app.bin files built from tag stub-v8 and from 516990f (`git archive`,
its scripts/build-fw, target/app-wp2.bin); for the older loaders also WP2_OLD_LOADERS, their release assets separated by ':'
(`gh release download loader-...`). Besides them target/app-wp2.bin (stub 10), target/loader.wp2app and widget-columns.
Frames go to runs/widget-old-stubs/."""
import asyncio
from contextlib import contextmanager
import json
import os
from pathlib import Path
import struct
import unittest
from unittest import mock, skipUnless
import zlib

from tests.e2e import harness, netsim
from tests.test_demo_e2e import save_png
from wp2apps import cli as wp2, firmware, install, package, paths

OUT = paths.ROOT / "runs" / "widget-old-stubs"
OLD_STUBS = {n: p for n in (8, 9) if (p := os.environ.get(f"WP2_STUB{n}_APP"))}
OLD_LOADERS = [Path(p) for p in os.environ.get("WP2_OLD_LOADERS", "").split(":") if p]
PKG = b"/NAND:/wp2apps/0.pkg"
WGT = b"/NAND:/wp2apps/0.wgt"


@contextmanager
def watch(stub: int, loader: Path, name: str):
    """A paired watch on that stub with that loader installed."""
    OUT.mkdir(parents=True, exist_ok=True)
    env = {}
    if stub in OLD_STUBS:
        nor = OUT / f"nor-stub{stub}.bin"
        nor.write_bytes(firmware.stage_nor(netsim.paired_nor(), Path(OLD_STUBS[stub]).read_bytes()))
        env["WP2_NOR"] = str(nor)
    with mock.patch.dict(os.environ, env), harness.installed([], name=name) as (emu, port):
        harness.cli_out(["install", loader, "--hci-port", port])
        yield emu, port


def session(port, action):
    return asyncio.run(wp2.with_watch(type("Args", (), {"hci_port": int(port), "address": None})(), action))


@skipUnless(harness.E2E and OLD_STUBS, "set WP2_E2E=1 and WP2_STUB8_APP, WP2_STUB9_APP (older patched app.bin)")
class OldStubsTest(unittest.TestCase):
    def setUp(self):
        self.full = (paths.ROOT / "target" / "widget-columns.wp2app").read_bytes()
        self.plain = package.strip_trailer(self.full)
        self.assertTrue(package.parse(self.full).widget_icons, "the fixture has a trailer")
        self.loader = paths.ROOT / "target" / "loader.wp2app"

    def falls_back(self, emu, name):
        """widget-columns writes the v1 card only when widget_columns returned exactly -22"""
        harness.open_app(emu, 0)
        save_png(OUT / f"{name}.png", *emu.frame())
        emu.send("c 1", "c 0", pause=3)  # BUTTON: the app returns
        self.assertEqual(harness.read_file(emu, WGT, 128), install.widget_record("Forecast", "21º"), name)
        uart = emu.uart()
        self.assertNotIn("FATAL", uart.upper(), name)
        self.assertEqual(uart.count("*** Booting Zephyr"), 1, f"{name}: guest rebooted")

    def test_full_package_refused_at_end(self):
        for stub in OLD_STUBS:
            with self.subTest(stub=stub):
                with watch(stub, self.loader, f"stub{stub}-trailer") as (emu, port):
                    async def run(s):
                        self.assertEqual((await install.stub(s)).version, stub)
                        begin = struct.pack("<BII", 0, len(self.full), zlib.crc32(self.full))
                        await install._call(s, install.BEGIN, begin)
                        for offset, part in install.chunks(self.full):
                            await install._call(s, install.DATA, struct.pack("<I", offset) + part)
                        # BEGIN and every DATA were taken: the refusal is END's
                        with self.assertRaisesRegex(ValueError, "0x90a3: INVALID"):
                            await install._call(s, install.END, b"\0")
                        self.assertIsNone(await install.info(s, 0), "nothing installed")

                        await install.upload(s, 0, self.plain)
                        self.assertEqual((await install.info(s, 0)).name, "widget-columns", "the stripped package installs")
                        # a refused update leaves the installed package in place
                        with self.assertRaisesRegex(ValueError, "0x90a3: INVALID"):
                            await install.upload(s, 0, self.full)
                        self.assertEqual((await install.info(s, 0)).name, "widget-columns")

                        for n in (1, 2, 3, 4):
                            with self.assertRaisesRegex(ValueError, "BAD_REQUEST", msg=f"PUSH of {n} columns"):
                                await install.push(s, 0, install.PUSH_WIDGET, install.columns_record([("Now", None, "1°")] * n))
                    session(port, run)
                    harness.ready(emu)
                    self.assertEqual(harness.read_file(emu, PKG, package.HEADER_SIZE), self.plain[:package.HEADER_SIZE])
                    self.assertIsNone(harness.read_file(emu, b"/NAND:/wp2apps/0.tmp", 4), "no .tmp left behind")
                    self.assertNotIn("FATAL", emu.uart().upper())

    def test_install_strips_and_loader_answers_22(self):
        for stub in OLD_STUBS:
            with self.subTest(stub=stub):
                with watch(stub, self.loader, f"stub{stub}-fallback") as (emu, port):
                    path = paths.ROOT / "target" / "widget-columns.wp2app"
                    out = json.loads(harness.cli_out(["install", path, "--hci-port", port]))  # the CLI strips for stub 8 and 9
                    self.assertEqual(str(out["installed"]), "0")
                    self.falls_back(emu, f"stub{stub}-new-loader")
                    harness.cli_out(["cards", "set", "widgets:0x40/1", "--hci-port", port])
                    harness.ready(emu)
                    harness.swipe(emu, 420, 233, 40, 233, steps=4)
                    w, h, rgb = frame = emu.frame()
                    save_png(OUT / f"stub{stub}-card.png", *frame)
                    # "21" ends at x 117: the degree sign (º, U+00BA; font32M has no °) is lit right of it
                    degree = [max(rgb[(y * w + x) * 3:(y * w + x) * 3 + 3]) for y in range(124, 156) for x in range(119, 140)]
                    self.assertGreater(sum(v > 70 for v in degree), 10, "the text card shows its degree sign")
                    self.assertNotIn("FATAL", emu.uart().upper())

    @skipUnless(OLD_LOADERS, "set WP2_OLD_LOADERS (released loader packages)")
    def test_released_loaders_run_the_app_and_answer_22(self):
        for loader in OLD_LOADERS:
            for stub in (*OLD_STUBS, 11):  # 11 (this build): after the OTA, before the loader update, with the trailer on NAND
                name = f"stub{stub}-{loader.stem}"
                with self.subTest(name), watch(stub, loader, name) as (emu, port):
                    session(port, lambda s: install.upload(s, 0, self.full if stub == 11 else self.plain))
                    self.falls_back(emu, name)


if __name__ == "__main__":
    unittest.main()
