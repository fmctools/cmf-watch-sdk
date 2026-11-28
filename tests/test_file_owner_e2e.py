"""Named app files follow their app, not its slot (loader owner.rs): another app in the slot doesn't inherit them, the
same app keeps them, and a removed app's go when the menu lists the apps again."""
import json
import unittest
from unittest import skipUnless

from tests.e2e import harness

DIR = b"/NAND:/wp2apps/"


def create(emu, path: bytes, data: bytes):
    """A new NAND file holding `data` (harness.write_file only overwrites one that exists)."""
    file, name, buf = harness.STUB_RAM + 0xE40, harness.STUB_RAM + 0xE20, harness.STUB_RAM + 0xE70
    emu.poke(name, path + b"\0")
    emu.poke(file, bytes(0x2C))
    emu.poke(buf, data)
    ui = b"ui_s"
    if emu.call(harness.FS_OPEN, file, name, 0x13, thread=ui):  # read, write, create
        raise RuntimeError(f"fs_open {path!r} failed")
    try:
        if emu.call(harness.FS_WRITE, file, buf, len(data), thread=ui) != len(data):
            raise RuntimeError(f"fs write {path!r} failed")
        emu.call(harness.FS_SYNC, file, thread=ui)
    finally:
        emu.call(harness.FS_CLOSE, file, thread=ui)


def owner(name: bytes) -> bytes:
    return name.ljust(24, b"\0")


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd and gdb)")
class FileOwnerTest(unittest.TestCase):
    def test_files_follow_their_app(self):
        with harness.installed(["loader", "hello", "api-demo"], name="file-owner") as (emu, port):
            slots = {v["name"]: k for k, v in json.loads(harness.cli_out(["list", "--hci-port", port]))["slots"].items()
                     if k != "L"}
            h, d = slots["hello"].encode(), slots["api-demo"].encode()
            read = lambda p, n=24: harness.read_file(emu, DIR + p, n)
            harness.ready(emu)
            # hello's slot holds another app's file and marker; api-demo's a file written before markers existed
            create(emu, DIR + h + b"_notes.txt", b"theirs")
            create(emu, DIR + h + b".own", owner(b"other"))
            create(emu, DIR + d + b"_keep.txt", b"mine")

            harness.open_menu(emu)
            self.assertIsNone(read(h + b"_notes.txt"), "another app's file stayed in hello's slot")
            self.assertEqual(read(h + b".own"), owner(b"hello"))
            self.assertEqual(read(d + b"_keep.txt", 4), b"mine", "a file without a marker is the app's own")
            self.assertEqual(read(d + b".own"), owner(b"api-demo"))

            # the same app again (an update keeps the name): its files stay
            create(emu, DIR + h + b"_notes.txt", b"ours")
            harness.open_menu(emu)
            self.assertEqual(read(h + b"_notes.txt", 4), b"ours")

            # api-demo removed: its files and marker go with the next menu
            harness.cli_out(["remove", "--slot", d.decode(), "--hci-port", port])
            harness.open_menu(emu)
            self.assertIsNone(read(d + b"_keep.txt"))
            self.assertIsNone(read(d + b".own"))
            self.assertEqual(read(h + b"_notes.txt", 4), b"ours", "another slot's files were touched")


if __name__ == "__main__":
    unittest.main()
