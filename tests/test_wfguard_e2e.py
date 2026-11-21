"""Boot watch face guard (docs/recon.md, "Watch face boot guard"): a face that faults the stock at boot costs one
fault, the next boot comes up on another built-in, the stub's flag is gone CONFIRM_MS later, the bad face can be
taken off the list over 0x9055 and the watch reboots cleanly. The bad face is WP2_BAD_FACE (default: the DataPulse
port from cmf-watch-utils runs/bootloop-repro/), imported over local273.wfc, the default face. Needs
target/app-wp2.bin from scripts/build-fw (stub version 9)."""
import asyncio
import os
import shutil
import struct
import subprocess
import unittest
from pathlib import Path
from unittest import skipUnless

from tests.e2e import harness
from wp2apps import cli as wp2
from wp2apps import link, paths

FLAG = b"/NAND:/wp2wfgrd"
WFM = 0x9055
DATA_STORAGE_WRITE = 0x101B5A6F  # (type, -1); WfmPara is in type 1 (wp2-core stock.rs)
BAD_FACE = Path(os.environ.get("WP2_BAD_FACE") or
                (paths.cmf_watch_utils() / "runs/bootloop-repro/datapulse-wp2.bin" if harness.E2E else "missing"))


def bad_fixtures() -> Path:
    """Default fixtures with BAD_FACE as local273.wfc in the NAND (the CLI imports it only for the stock app)."""
    cmf = paths.cmf_watch_utils()
    run = harness.RUNS / "wfguard-face"
    if not (run / "nand.bin").is_file():
        # the stock faults on this face at ~4 s; the import is in nand.bin whatever the outcome
        subprocess.run(["uv", "run", "--locked", "cmf-watch-utils", "emu", "run", "--headless", "--watch-face",
                        str(BAD_FACE), "--fixtures", str(cmf / "fixtures"), "--qemu", str(harness.QEMU),
                        "--rom", str(cmf / "runtime/compat.bin"), "--seconds", "3", "--audio", "none",
                        "--output", str(run)], cwd=paths.ROOT)
    fixtures = harness.RUNS / "fixtures-wfguard"
    fixtures.mkdir(parents=True, exist_ok=True)
    for name, target in (("nor.bin", cmf / "fixtures/nor.bin"), ("nand-id.bin", cmf / "fixtures/nand-id.bin"),
                         ("nand-defaults.bin", run / "nand.bin"), ("wfc-index.txt", run / "wfc-index.txt")):
        (fixtures / name).unlink(missing_ok=True)
        (fixtures / name).symlink_to(target)
    return fixtures


def wfm(port, payload: bytes) -> bytes:
    async def go(s):
        return await s.request(link.SYSTEM, WFM, payload, 0xA055)
    return asyncio.run(wp2.with_watch(type("Args", (), {"hci_port": port, "address": None})(), go))


def face_list(port) -> tuple[int, list[int]]:
    reply = wfm(port, b"\x00")
    idx, cnt = reply[1], reply[2]
    return idx, list(struct.unpack_from(f"<{cnt}I", reply, 4))


def boots(uart: str) -> list[str]:
    return uart.split("*** Booting Zephyr")[1:]


@skipUnless(harness.E2E and BAD_FACE.is_file(), "set WP2_E2E=1 (emulator, netsimd, gdb) and WP2_BAD_FACE")
class WatchFaceGuardTest(unittest.TestCase):
    def shot(self, emu, name):
        from tests.test_demo_e2e import save_png
        harness.ready(emu)  # the "Paired" popup and the screen-off after the BLE session
        w, h, rgb = emu.frame()
        save_png(emu.run / f"{name}.png", w, h, rgb)

    def test_bad_face_falls_back_once(self):
        from tests.e2e.netsim import netsim
        nor = harness.patched_paired_nor()
        with netsim() as port, harness.Emu("wfguard", nor=nor, fixtures=bad_fixtures(), hci_port=port) as emu:
            # boot 1 arms the flag and faults on 273; boot 2 finds it and makes 274 active
            end = harness.time.monotonic() + 240
            while len(b := boots(emu.uart())) < 2 or "launcher enter" not in b[-1]:
                self.assertLess(harness.time.monotonic(), end, "no second boot to the launcher")
                harness.time.sleep(1)
            emu.keep_awake()  # this QEMU cannot enter S3BT (~15 s idle)
            first, second = boots(emu.uart())[:2]
            self.assertIn("ZEPHYR FATAL ERROR", first)
            self.assertNotIn("ZEPHYR FATAL ERROR", second)
            self.assertRegex(second, r"cfgOk:1, count:6.*\n(.*\n){0,6}.*m_u32CurrId:274.*\n.*WfmPara_Save enter")
            self.assertIsNotNone(harness.read_file(emu, FLAG, 1))

            # the confirm timer (ui_service, screen kept on: the screen-off case is not covered here)
            emu.sleep(25)
            self.assertIsNone(harness.read_file(emu, FLAG, 1))
            self.assertEqual(face_list(port), (1, [273, 274, 275, 276, 277, 280]))
            self.shot(emu, "1-fallback")

            # take the bad face off the list (a built-in stays on NAND, a download goes at the next boot)
            self.assertEqual(wfm(port, bytes([1, 0, 5, 0]) + struct.pack("<5I", 274, 275, 276, 277, 280)), b"\x00")
            self.assertEqual(face_list(port), (0, [274, 275, 276, 277, 280]))

            # AT REBOOT skips the stock's storage flush that the menu reboot and power-off run (0x100C6234)
            emu.call(DATA_STORAGE_WRITE, 1, 0xFFFFFFFF, thread=b"ui_s")
            harness.reboot(emu, port)
            emu.sleep(25)
            uart = emu.uart()
            third = boots(uart)[-1]
            self.assertEqual(len(boots(uart)), 3)
            self.assertEqual(uart.count("ZEPHYR FATAL ERROR"), 1)
            # no flag at boot: armed again, no fallback save after the load
            self.assertRegex(third, r"cfgOk:1, count:5")
            self.assertNotRegex(third, r"cfgOk:1, count:5.*\n(.*\n){0,6}.*WfmPara_Save")
            self.assertIsNone(harness.read_file(emu, FLAG, 1))
            emu.keep_awake()
            self.assertEqual(face_list(port), (0, [274, 275, 276, 277, 280]))
            self.shot(emu, "2-after-reboot")
            print("wfguard run:", emu.run, "boots", len(boots(uart)), "faults", uart.count("ZEPHYR FATAL ERROR"))


WP3_STUB = paths.ROOT / "target/wp3-stub.bin"
WP3_APP = paths.ROOT / "target/app-wp3.bin"


def wp3_preview(run: Path) -> tuple[Path, Path]:
    """app-wp3.bin and the compatibility ROM with cmf-watch-utils' watch face preview (skips the setup screens)."""
    app, rom = run / "app-preview.bin", run / "compat-preview.bin"
    subprocess.run(["uv", "run", "--locked", "python", "-c",
                    "import sys; from cmf_watch_utils.emu.preview import watch_face_preview as p; "
                    "rom = bytearray(open('runtime/p10/compat.bin', 'rb').read()); "
                    "open(sys.argv[2], 'wb').write(p(open(sys.argv[1], 'rb').read(), rom)); "
                    "open(sys.argv[3], 'wb').write(rom)", str(WP3_APP), str(app), str(rom)],
                   cwd=paths.cmf_watch_utils(), check=True)
    return app, rom


@skipUnless(harness.E2E and WP3_APP.is_file() and shutil.which("gdb"),
            "set WP2_E2E=1, gdb on PATH and build target/app-wp3.bin (scripts/build-fw)")
class Wp3WatchFaceGuardTest(unittest.TestCase):
    """Watch 3 Pro (fw/wp3-stub): no face is known to fault 1.0.1.77 (the DataPulse port renders), so the debugger
    models one: built-in 276 made active, a fault once WFManager_Init picked it (tests/e2e/wp3_badface.gdb)."""

    def test_bad_face_falls_back_once(self):
        cmf = paths.cmf_watch_utils()
        run = harness.RUNS / f"wfguard-wp3-{harness.time.strftime('%H%M%S')}"
        run.mkdir(parents=True)
        app, rom = wp3_preview(run)
        stub = WP3_STUB.read_bytes()
        udf, flag = (0x102BD280 + stub.index(pattern) for pattern in (b"\x00\xde", b"/NAND:/wp2wfgrd\0"))
        port = harness.free_port()
        out = run / "emu"
        emu = subprocess.Popen(["uv", "run", "--locked", "cmf-watch-utils", "emu", "run", "--profile", "wp3-stock",
                                "--headless", "--allow-reboot", "--app", str(app), "--rom", str(rom),
                                "--gdb-port", str(port), "--fixtures", str(cmf / "fixtures/wp3"),
                                "--qemu", str(cmf / "runtime/p10/qemu-system-arm"), "--seconds", "100",
                                "--audio", "none", "--output", str(out)], cwd=cmf)
        try:
            end = harness.time.monotonic() + 30
            while True:
                try:
                    harness.socket.create_connection(("127.0.0.1", port)).close()
                    break
                except OSError:
                    self.assertLess(harness.time.monotonic(), end, "no gdb port")
                    harness.time.sleep(0.05)
            gdb = subprocess.run(["gdb", "-batch", "-ex", "set architecture armv7e-m", "-ex", "set pagination off",
                                  "-ex", f"target remote :{port}", "-ex", f"set $udf = {udf:#x}",
                                  "-ex", f"set $flag = {flag:#x}", "-x", str(paths.ROOT / "tests/e2e/wp3_badface.gdb")],
                                 capture_output=True, text=True, timeout=600).stdout
        finally:
            emu.wait(timeout=600)
        (run / "gdb.log").write_text(gdb)
        uart = (out / "uart0.log").read_text(errors="replace")
        # boot 1 always reboots out of the setup state; boot 2 builds the list with 276 active and faults;
        # boot 3 falls back to 274 and confirms; the reset makes boot 4, which loads 274 from NOR with no fallback
        events = [line for line in gdb.splitlines() if line.startswith(("BADFACE", "INIT", "CONFIRM"))]
        self.assertEqual(events, ["BADFACE fault id=276", "INIT id=274", "CONFIRM unlink", "CONFIRMED reset",
                                  "INIT id=274"])
        self.assertEqual((len(boots(uart)), uart.count("ZEPHYR FATAL ERROR")), (4, 1))
        third, fourth = boots(uart)[2:]
        self.assertRegex(third, r"cfgOk:1, count:7.*\n(.*\n){0,3}.*WfmPara_Save enter.*\n.*\n.*data_storage_write type:1 ")
        self.assertRegex(fourth, r"cfgOk:1, count:7")
        self.assertNotRegex(fourth, r"cfgOk:1, count:7.*\n(.*\n){0,3}.*WfmPara_Save")
        print("wfguard wp3 run:", run, "boots", len(boots(uart)), "faults", uart.count("ZEPHYR FATAL ERROR"))


if __name__ == "__main__":
    unittest.main()
