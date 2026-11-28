"""GPS for apps (docs/abi-1.4.md, "GPS"): gps-probe asks for it, the loader sets ux2sys 0x63 to 1, the emulator's
GPS model sends NMEA for a synthetic position (`g 1 lat_e6 lon_e6`) and the app gets GPS events and fixes; a request
off, a crash and exit turn the GPS off again (a pause keeps it, as the stock workouts do); a GPS the stock already runs is read and left on.
Needs target/{loader,gps-probe}.wp2app."""
import struct
import unittest
from unittest import skipUnless

from tests.e2e import harness
from tests.test_back_e2e import tap_row
from tests.test_demo_e2e import save_png
from tests.test_e2e import START, dismiss_popup
from wp2apps import paths

OUT = paths.ROOT / "runs" / "gps-e2e"
ELF = paths.ROOT / "sdk/target/thumbv7em-none-eabi/release/gps-probe"
MODE = 0x380443F4 + 1  # wp2_core::stock::GPS_STATE + 1: the mode gps_service has in force
UX2SYS_SET, PROP_GPS_MODE = 0x100E07B9, 0x63
RESULT, EVENTS, FIX, SATS, LAT, LON, TTFF, SPEED = range(8)  # PROBE, gps-probe main.rs
LAT_E6, LON_E6 = 12_345_678, -98_765_432  # synthetic: the emulator's GPS input


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd, gdb and llvm-nm)")
class GpsTest(unittest.TestCase):
    def probe(self, emu):
        addr = harness.u32(emu, START + harness.APP_BLOCK_OFF) + harness.symbol(ELF, "PROBE")
        return struct.unpack("<8i", emu.peek(addr, 32))

    def mode(self, emu):
        return emu.peek(MODE, 1)[0]

    def running(self, emu):
        return emu.peek(START + 6, 1) != b"\xff"

    def gps_lines(self, emu, mark):
        return [l for l in emu.uart()[mark:].splitlines() if "gps_service_proc" in l or "gps_disable ok" in l]

    def test_gps(self):
        OUT.mkdir(parents=True, exist_ok=True)
        with harness.installed(["loader", "gps-probe"], name="gps") as (emu, port):
            dismiss_popup(emu)
            harness.ready(emu)
            emu.send(f"g 1 {LAT_E6} {LON_E6}")
            harness.open_apps(emu)
            self.assertEqual(self.mode(emu), 0)
            mark = len(emu.uart())
            tap_row(emu, 0)
            emu.sleep(4)
            p = self.probe(emu)
            self.assertEqual(p[RESULT], 0)
            self.assertEqual(self.mode(emu), 1, "GPS on, the workouts' mode")
            self.assertIn("_gps_service_proc enable[1]", emu.uart()[mark:])
            self.assertGreaterEqual(p[EVENTS], 2)
            self.assertEqual((p[FIX], p[SATS]), (1, 8), "the emulator's GGA: fix 1, 8 satellites")
            self.assertLessEqual(abs(p[LAT] - LAT_E6 * 10), 10)
            self.assertLessEqual(abs(p[LON] - LON_E6 * 10), 10)
            self.assertGreaterEqual(p[TTFF], 1)
            w, h, rgb = emu.frame()
            save_png(OUT / "1-fix.png", w, h, rgb)

            events = p[EVENTS]
            emu.send(f"g 1 {LAT_E6 + 1000} {LON_E6}")  # 0.001° north, about 111 m
            emu.sleep(10)
            p = self.probe(emu)
            self.assertIn(p[EVENTS] - events, range(7, 14), "about one GPS event per second")
            self.assertLessEqual(abs(p[LAT] - (LAT_E6 + 1000) * 10), 10, "the app follows the position")
            self.assertEqual(sum("enable[1]" in l for l in self.gps_lines(emu, mark)), 1, "one start per hold")

            emu.send("t 1 233 233", "t 0 233 233", pause=3)  # request off
            self.assertEqual(self.mode(emu), 0, "off on request")
            self.assertEqual(self.probe(emu)[FIX], 0, "the app is told there is no fix")
            emu.send("t 1 233 233", "t 0 233 233", pause=3)  # on again
            self.assertEqual(self.mode(emu), 1)

            emu.send("r -80", pause=3)  # the app panics
            self.assertFalse(self.running(emu))
            self.assertEqual(self.mode(emu), 0, "off after the crash")

            tap_row(emu, 0)
            emu.sleep(3)
            self.assertEqual(self.mode(emu), 1)
            mark = len(emu.uart())
            emu.send("c 1", "c 0", pause=3)  # BUTTON: the app returns
            if self.running(emu):  # a loaded host let the panel go dark: that press only woke it
                emu.send("c 1", "c 0", pause=3)
            self.assertFalse(self.running(emu))
            emu.sleep(2)
            self.assertEqual(self.mode(emu), 0, "off after exit")
            self.assertIn("gps_disable ok", emu.uart()[mark:], "the receiver was powered down")

            # last: the charge popup that pauses stays plugged and comes back at every wake
            tap_row(emu, 0)
            emu.sleep(3)
            self.assertEqual(self.mode(emu), 1)
            mark = len(emu.uart())
            harness.popup(emu)  # PAUSE
            emu.sleep(2)
            self.assertEqual(self.mode(emu), 1, "held while paused, as the stock workouts do")
            harness.close_popup(emu)  # RESUME
            emu.sleep(2)
            self.assertEqual(self.mode(emu), 1, "on after resume")
            self.assertNotIn("enable[1]", "".join(self.gps_lines(emu, mark)), "never restarted: the fix is kept")
            uart = emu.uart()
            self.assertNotIn("ZEPHYR FATAL ERROR", uart)
            self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")

    def test_stock_owner_left_on(self):
        """GPS already on (as a stock workout or its list leaves it): the app reads it, its exit leaves it on."""
        with harness.installed(["loader", "gps-probe"], name="gps-owner") as (emu, port):
            dismiss_popup(emu)
            harness.ready(emu)
            emu.send(f"g 1 {LAT_E6} {LON_E6}")
            harness.open_apps(emu)
            emu.poke(harness.STUB_RAM + 0xF00, struct.pack("<I", 1))
            emu.call(UX2SYS_SET, PROP_GPS_MODE, harness.STUB_RAM + 0xF00, thread=b"ui_s")
            emu.sleep(2)
            self.assertEqual(self.mode(emu), 1)
            mark = len(emu.uart())
            tap_row(emu, 0)
            emu.sleep(4)
            p = self.probe(emu)
            self.assertEqual((p[RESULT], p[FIX]), (0, 1), "the app reads the stock's fix")
            emu.send("c 1", "c 0", pause=3)
            self.assertFalse(self.running(emu))
            self.assertEqual(self.mode(emu), 1, "the stock's GPS stays on")
            self.assertEqual(self.gps_lines(emu, mark), [], "the loader never set the mode")


if __name__ == "__main__":
    unittest.main()
