"""Storage inspection (docs/storage-inspection.md): typed service, scan cleanup on a forced stop, installed-app icons.

Needs target/storage-probe.wp2app and target/storage-icons.wp2app, built from tests/probes/storage (README there)."""
import shutil
import struct
import unittest
from unittest import skipUnless

from tests.e2e import harness
from tests.test_e2e import START, dismiss_popup
from wp2apps import paths

OUT = paths.ROOT / "runs" / "storage"
LOADER_ELF = paths.ROOT / "fw/target/thumbv7em-none-eabi/release/wp2-loader"
PROBE_ELF = paths.ROOT / "tests/probes/storage/target/thumbv7em-none-eabi/release/storage-probe"
ICONS_ELF = paths.ROOT / "tests/probes/storage/target/icons/thumbv7em-none-eabi/release/storage-probe"


def tap_row(emu, row):
    y = 180 + 128 * row  # wp2-loader menu.rs
    emu.send(f"t 1 200 {y}", f"t 0 200 {y}", pause=2)


def loader(emu, name, size=4):
    """A loader static; scalar statics carry rustc's " (.0)" suffix in the ELF."""
    return emu.peek(harness.u32(emu, START + harness.LOADER_BLOCK_OFF) + harness.symbol(LOADER_ELF, name), size)


def marker(emu, elf):
    addr = harness.u32(emu, START + harness.APP_BLOCK_OFF) + harness.symbol(elf, "STORAGE_PROBE")
    return struct.unpack("<8I", emu.peek(addr, 32))


def lit(rgb, x0, y0, side):
    """Pixels that are not black in a side x side box."""
    return sum(any(rgb[3 * ((y0 + y) * 466 + x0 + x):3 * ((y0 + y) * 466 + x0 + x) + 3])
               for y in range(side) for x in range(side))


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd and gdb)")
class StorageTest(unittest.TestCase):
    def start(self, emu, row, elf, ready):
        dismiss_popup(emu)
        harness.ready(emu)
        harness.open_apps(emu)
        free = harness.u32(emu, harness.HEAP + 8)
        tap_row(emu, row)
        for _ in range(40):
            if emu.peek(START + 6, 1) != b"\xff" and marker(emu, elf)[0] == ready:
                break
            emu.sleep(0.25)
        return free, marker(emu, elf)

    def stop(self, emu, free):
        """Crown: the probe owns back and never reads events, so the loader's forced escape stops it."""
        emu.send("c 1", "c 0", pause=0)
        start = emu.clock()
        while emu.peek(START + 6, 1) != b"\xff" and emu.clock() - start < 8:
            emu.sleep(0.25)
        emu.sleep(1)
        self.assertEqual(emu.peek(START + 4, 3), b"\x01\x01\xff")  # loader menu, no app
        self.assertEqual(harness.u32(emu, START + harness.APP_BLOCK_OFF), 0)
        self.assertEqual(loader(emu, "SCAN_BUFFER (.0)"), bytes(4), "scan cache freed")
        self.assertEqual(loader(emu, "DIR_OPEN (.0)", 1), b"\x00", "directory closed")
        self.assertEqual(harness.u32(emu, harness.HEAP + 8), free, "app block, scan cache and thumbnails freed")

    def assert_clean(self, emu):
        uart = emu.uart()
        self.assertNotIn("ZEPHYR FATAL ERROR", uart)
        self.assertNotIn("MllcFailedHook", uart)
        self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")

    def test_canvas_scan_then_forced_stop(self):
        OUT.mkdir(parents=True, exist_ok=True)
        with harness.installed(["loader", "storage-probe", "hello"], name="storage") as (emu, port):
            free, (ready, version, slots, total, avail, files, complete, partial) = self.start(emu, 0, PROBE_ELF, 1)
            print(f"\nslots {slots}, total {total}, free {avail}, files {files}, complete {complete}, partial {partial}")
            self.assertEqual((ready, version), (1, 1))
            self.assertIn(slots, (8, 16))
            self.assertTrue(0 < avail <= total, (total, avail))
            self.assertLessEqual(files, 96)
            self.assertNotEqual(loader(emu, "SCAN_BUFFER (.0)"), bytes(4), "scan cache allocated")
            self.assertEqual(loader(emu, "DIR_OPEN (.0)", 1), b"\x01", "scan left a directory open")
            self.assertEqual(loader(emu, "LV_STATE"), bytes(4), "storage must not create an LVGL root")
            shutil.copy(emu.run / "frame.ppm", OUT / "probe.ppm")
            self.stop(emu, free)
            tap_row(emu, 1)  # another app still starts and returns on BUTTON
            self.assertNotEqual(emu.peek(START + 6, 1), b"\xff")
            emu.send("c 1", "c 0", pause=3)
            self.assertEqual(emu.peek(START + 4, 3), b"\x01\x01\xff")
            self.assert_clean(emu)

    def test_installed_icons(self):
        OUT.mkdir(parents=True, exist_ok=True)
        # vibration-lab (with icon.svg) in slot 0 and row 0, the probe in row 1
        with harness.installed(["loader", "vibration-lab", "storage-icons"], name="storage-icons") as (emu, port):
            free, m = self.start(emu, 1, ICONS_ELF, 2)
            self.assertEqual(m[0], 2)
            emu.sleep(1)
            shutil.copy(emu.run / "frame.ppm", OUT / "icons.ppm")
            rgb = emu.frame()[2]
            thumb, native, missing = lit(rgb, 90, 180, 64), lit(rgb, 240, 164, 96), lit(rgb, 190, 300, 64)
            print(f"\nlit pixels: thumbnail {thumb}, native {native}, missing {missing}")
            self.assertGreater(thumb, 500)
            self.assertGreater(native, 1000)
            self.assertEqual(missing, 0)
            self.stop(emu, free)
            self.assert_clean(emu)


if __name__ == "__main__":
    unittest.main()
