"""Bitmap assets in the emulator: bitmap-lv shows its baked orb through IMAGE_SRC (the loader's descriptor pool) and
bitmap-gallery its ring through BLIT, both without stopping the guest. Needs target/{loader,bitmap-lv,bitmap-gallery}
.wp2app. WP2_NETSIM_INSTANCE=92."""
import unittest
from unittest import skipUnless

from tests.e2e import harness
from tests.test_demo_e2e import save_png
from wp2apps import paths

RUNNING = harness.STUB_RAM + 6
OUT = paths.ROOT / "runs" / "bitmap"


def lit(rgb, w, cx, cy, r=32):
    """Share of non-black pixels in the (2r)^2 square around (cx, cy)."""
    px = [rgb[3 * (y * w + x):3 * (y * w + x) + 3] for y in range(cy - r, cy + r) for x in range(cx - r, cx + r)]
    return sum(1 for p in px if any(p)) / len(px)


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd, gdb and llvm-nm)")
class BitmapTest(unittest.TestCase):
    def test_lvgl_image_and_canvas_blit(self):
        OUT.mkdir(parents=True, exist_ok=True)
        with harness.installed(["loader", "bitmap-lv", "bitmap-gallery"], name="bitmap") as (emu, _port):
            # (menu row, centre of the 64x64 picture): the orb where the stock page lays it out (seen in the
            # emulator), the ring where bitmap-gallery draws it. Both are rings: about 35 % of the square is lit,
            # the "needs a bitmap loader" text or a blank screen light far less
            for row, (name, cx, cy) in enumerate([("bitmap-lv", 217, 152), ("bitmap-gallery", 233, 160)]):
                with self.subTest(app=name):
                    harness.open_app(emu, row=row)
                    emu.sleep(1.5)
                    w, h, rgb = emu.frame()
                    save_png(OUT / f"{name}.png", w, h, rgb)
                    self.assertGreater(lit(rgb, w, cx, cy), 0.15, f"{name}: no picture at ({cx}, {cy})")
                    self.assertNotIn("stopped on", (emu.run / "qemu.log").read_text())
                    emu.send("c 1", "c 0", pause=2)
                    self.assertEqual(emu.peek(RUNNING, 1), b"\xff", f"{name} did not exit")
            uart = emu.uart()
            self.assertNotIn("ZEPHYR FATAL ERROR", uart)
            self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")


if __name__ == "__main__":
    unittest.main()
