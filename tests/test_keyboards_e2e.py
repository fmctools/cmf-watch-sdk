"""The keyboards demo on the emulator: the rotary keyboard hides the status bar and gives it back, delete held
deletes until lifted, typing leaves no marks on the dial, and the button goes back to QWERTY before it leaves."""
import shutil
import unittest
from unittest import skipUnless

from tests.e2e import harness
from tests.test_e2e import bright_pixels, dismiss_popup
from wp2apps import paths

OUT = paths.ROOT / "runs" / "keyboards"
LOADER_ELF = paths.ROOT / "fw" / "target" / "thumbv7em-none-eabi" / "release" / "wp2-loader"
RUNNING = harness.STUB_RAM + 6
DIAL_KEY = [(213, 387), (167, 387)]  # the bottom row with the emoji key, without it (cmf_wp2_ui::keyboard)
DEL = (133, 332)  # the rotary keyboard's delete (cmf_wp2_ui::dial BAR)
TEXT_BOX = (127, 86, 212, 83)  # dial::TEXT_Y/TEXT_PX/TEXT_ROWS, cleared by draw_text_in


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd and gdb)")
class KeyboardsTest(unittest.TestCase):
    def test_rotary_keyboard(self):
        with harness.installed(["loader", "keyboards"], name="keyboards") as (emu, port):
            dismiss_popup(emu)
            harness.open_app(emu, 0)
            block = harness.u32(emu, harness.STUB_RAM + harness.LOADER_BLOCK_OFF)
            shown = lambda: emu.peek(block + harness.symbol(LOADER_ELF, "status::SHOWN (.0)"), 1)[0]
            OUT.mkdir(parents=True, exist_ok=True)
            save = lambda name: shutil.copy(emu.run / "frame.ppm", OUT / f"{name}.ppm")
            self.assertEqual(shown(), 1)
            save("qwerty")

            for x, y in DIAL_KEY:
                emu.send(f"t 1 {x} {y}", f"t 0 {x} {y}", pause=2)
                if shown() == 0:
                    break
            self.assertEqual(shown(), 0, "the rotary keyboard hides the status bar")
            # four letters from the middle, a space
            for _ in range(4):
                emu.send(f"t 1 233 233", f"t 0 233 233", pause=0.6)
            emu.send(f"t 1 233 332", f"t 0 233 332", pause=2)
            typed = emu.frame()
            save("dial-typed")
            w = typed[0]
            self.assertGreater(bright_pixels(typed[2], w, TEXT_BOX), 50, "the text shows")

            # the screen drawn whole again shows the same: typing left no marks
            emu.send("c 1", "c 0", pause=2)
            self.assertEqual(shown(), 1, "back to QWERTY: the status bar again")
            self.assertNotEqual(emu.peek(RUNNING, 1), b"\xff", "the first press only goes back")
            for x, y in DIAL_KEY:
                emu.send(f"t 1 {x} {y}", f"t 0 {x} {y}", pause=2)
                if shown() == 0:
                    break
            again = emu.frame()
            diff = sum(a != b for a, b in zip(typed[2], again[2])) // 3
            self.assertLess(diff, 200, f"{diff} pixels differ from a full redraw")

            # delete held: everything goes, then it stops
            emu.send(f"t 1 {DEL[0]} {DEL[1]}", pause=2)
            emu.send(f"t 0 {DEL[0]} {DEL[1]}", pause=1.5)
            save("dial-deleted")
            self.assertEqual(bright_pixels(emu.frame()[2], w, TEXT_BOX), 0, "delete held cleared the text")

            emu.send("c 1", "c 0", pause=2)
            self.assertNotEqual(emu.peek(RUNNING, 1), b"\xff")
            emu.send("c 1", "c 0", pause=2)
            self.assertEqual(emu.peek(RUNNING, 1), b"\xff", "the second press leaves")
            self.assertNotIn("ZEPHYR FATAL ERROR", emu.uart())


if __name__ == "__main__":
    unittest.main()
