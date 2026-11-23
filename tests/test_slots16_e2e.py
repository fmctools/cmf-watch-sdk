"""Stub 8: 16 app slots. Fills all of them (15 copies of vibration-lab under other names, so 15 icons, and hello in
slot 15 with its page on view 0x107), then checks the list, the menu with every icon loaded, and the page.
Logs the RES heap with the full menu open and the free space of /NAND: (statvfs) to runs/e2e/<run>/slots16.json.
Needs target/app-wp2.bin (scripts/build-fw), target/loader.wp2app, hello and vibration-lab (scripts/build-app),
llvm-nm on PATH."""
import asyncio
import json
import struct
import unittest
from unittest import skipUnless

from tests.e2e import harness
from tests.test_demo_e2e import save_png
from wp2apps import cli, install, package, paths

RUNNING = harness.STUB_RAM + 6
LOADER_ELF = paths.ROOT / "fw" / "target" / "thumbv7em-none-eabi" / "release" / "wp2-loader"
ICONS_OFF = 4 + 16 * 4 + 32 + 4 + 4  # menu.rs `Ui` (repr(C)): cont, names, font, font_open (padded), bar, icons
HELLO = 15


def statvfs(emu, path: bytes) -> dict:
    """Zephyr fs_statvfs through the mount's API (+0x44, after mkdir at +0x3C, docs/recon.md "Mounts")."""
    mp_ptr, name, buf = harness.STUB_RAM + 0xE20, harness.STUB_RAM + 0xE40, harness.STUB_RAM + 0xE70
    emu.poke(name, path + b"\0")
    emu.poke(mp_ptr, bytes(4))
    if emu.call(harness.FS_FIND_MOUNT, mp_ptr, name, 0, thread=b"ui_s"):
        raise RuntimeError("fs_get_mnt_point failed")
    mp = harness.u32(emu, mp_ptr)
    fn = harness.u32(emu, harness.u32(emu, mp + 0x1C) + 0x44)
    emu.poke(buf, bytes(16))
    rc = emu.call(fn | 1, mp, name, buf, thread=b"ui_s")
    bsize, frsize, blocks, bfree = struct.unpack("<4I", emu.peek(buf, 16))
    return {"rc": rc, "bsize": bsize, "frsize": frsize, "blocks": blocks, "bfree": bfree,
            "total": frsize * blocks, "free": frsize * bfree}


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd, gdb and llvm-nm)")
class Slots16Test(unittest.TestCase):
    def test_sixteen_slots(self):
        target = paths.ROOT / "target"
        lab = package.parse((target / "vibration-lab.wp2app").read_bytes())
        for n in range(HELLO):
            (target / f"lab-{n:x}.wp2app").write_bytes(package.pack(
                lab.image, name=f"lab-{n:x}", version=1, entry=lab.entry, relocs=lab.relocs, abi_minor=lab.abi_minor))
        with harness.installed(["loader"], name="slots16") as (emu, port):
            harness.ready(emu)  # an idle guest after the BLE disconnect stalls emu.call
            report = {"nand_before": statvfs(emu, b"/NAND:/")}
            # one session: a bind per install, 16 in a row, ran the emulated BLE out of bond slots ("no cfg left")
            async def all_apps(session):
                for n in range(HELLO):
                    await install.install(session, (target / f"lab-{n:x}.wp2app").read_bytes(), n)
                await install.install(session, (target / "hello.wp2app").read_bytes(), HELLO)
            asyncio.run(cli.with_watch(type("Args", (), {"hci_port": port, "address": None})(), all_apps))
            listing = json.loads(harness.cli_out(["list", "--hci-port", port]))
            self.assertEqual(listing["stub"]["version"], 11)
            self.assertEqual({k: v["name"] for k, v in listing["slots"].items() if k != "L"},
                             {**{str(n): f"lab-{n:x}" for n in range(HELLO)}, str(HELLO): "hello"})
            harness.ready(emu)
            self.assertEqual(harness.read_file(emu, b"/NAND:/wp2apps/F.pkg", 4), b"WP2A", "slot 15 is F.pkg")
            report["nand_after"] = statvfs(emu, b"/NAND:/")
            report["res_free_menu_closed"] = harness.u32(emu, harness.HEAP + 8)

            harness.open_menu(emu)
            save_png(emu.run / "menu.png", *emu.frame())
            report["res_free_menu_open"] = harness.u32(emu, harness.HEAP + 8)
            report["res_min_free_ever"] = harness.u32(emu, harness.HEAP + 0xC)
            block = harness.u32(emu, harness.STUB_RAM + harness.LOADER_BLOCK_OFF)
            icons = struct.unpack("<16I", emu.peek(block + harness.symbol(LOADER_ELF, "lv::UI") + ICONS_OFF, 64))
            report["icons"] = sum(map(bool, icons))
            (emu.run / "slots16.json").write_text(json.dumps(report, indent=2))
            self.assertEqual(report["icons"], HELLO, "every lab row has its icon (hello has none)")

            harness.cli_out(["cards", "set", f"view:{0xF8 + HELLO:#x}", "--hci-port", port])
            emu.send("c 1", "c 0", pause=3)  # crown: the menu goes, the face shows the new page
            harness.ready(emu)
            mark = len(emu.uart())
            harness.swipe(emu, 420, 233, 40, 233, steps=4)
            save_png(emu.run / "page.png", *emu.frame())
            self.assertIn(f"view {0xF8 + HELLO} focused", emu.uart()[mark:])
            self.assertEqual(emu.peek(RUNNING, 1)[0], HELLO, "hello runs on page 0x107")
            self.assertNotIn("FATAL", emu.uart().upper())


if __name__ == "__main__":
    unittest.main()
