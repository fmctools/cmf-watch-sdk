"""Watch face guard (docs/recon.md, "Watch face boot guard"): a face that faults the stock costs one fault whether
it is booted or picked, a face that dies after its load falls back on the third death, power off, reboot and the OTA
reboot do not count. The bad face is WP2_BAD_FACE (default: the DataPulse port from cmf-watch-utils
runs/bootloop-repro/), imported over local273.wfc, the default face. Needs target/app-wp2.bin and target/app-wp3.bin
from scripts/build-fw. Stub 11 no longer faults on that face, so the Watch Pro 2 runs here get an app.bin whose
procedural type table is the stock's again."""
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
from wp2apps import firmware, link, paths

FLAG = b"/NAND:/wp2wfgrd"
WFM = 0x9055
DATA_STORAGE_WRITE = 0x101B5A6F  # (type, -1); WfmPara is in type 1 (wp2-core wfguard.rs)
CURRENT_ID = 0x3801AE58
PARA_MIRROR = 0x3800BC9D
UX2SYS_REBOOT = 0x100E0DE9  # what the menu's reboot sends to the launcher
OTA_FORCE_REBOOT = 0x101CE674
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


def faulting_nor() -> bytes:
    """The patched paired NOR with the stock's type table back, the bug BAD_FACE faults on (tag 0x82 type 6)."""
    from tests.e2e.netsim import paired_nor
    app = bytearray((paths.ROOT / "target/app-wp2.bin").read_bytes())
    for lit in firmware.PROC_TYPES_LITERALS:
        struct.pack_into("<I", app, lit - firmware.APP_VBASE, firmware.PROC_TYPES)
    firmware.fix_acth(app)
    return firmware.stage_nor(paired_nor(), bytes(app))


def wfm(port, payload: bytes) -> bytes:
    async def go(s):
        return await s.request(link.SYSTEM, WFM, payload, 0xA055)
    return asyncio.run(wp2.with_watch(type("Args", (), {"hci_port": port, "address": None})(), go))


def face_list(port) -> tuple[int, list[int]]:
    reply = wfm(port, b"\x00")
    idx, cnt = reply[1], reply[2]
    return idx, list(struct.unpack_from(f"<{cnt}I", reply, 4))


PICKED, LOADING, LOADED = 0, 1, 2  # wp2-core wfguard.rs Stage


def guard_flag(emu) -> tuple[int, int, int] | None:
    """(id, stage, deaths)"""
    raw = harness.read_file(emu, FLAG, 8)
    if raw is None:
        return None
    _, stage, deaths, _, face = struct.unpack("<4BI", raw)
    return face, stage, deaths


def boots(uart: str) -> list[str]:
    return uart.split("*** Booting Zephyr")[1:]


@skipUnless(harness.E2E and BAD_FACE.is_file(), "set WP2_E2E=1 (emulator, netsimd, gdb) and WP2_BAD_FACE")
class WatchFaceGuardTest(unittest.TestCase):
    def shot(self, emu, name):
        from tests.test_demo_e2e import save_png
        harness.ready(emu)  # the "Paired" popup and the screen-off after the BLE session
        w, h, rgb = emu.frame()
        save_png(emu.run / f"{name}.png", w, h, rgb)

    def wait_boot(self, emu, count: int):
        end = harness.time.monotonic() + 240
        while len(b := boots(emu.uart())) < count or "launcher enter" not in b[-1]:
            self.assertLess(harness.time.monotonic(), end, f"no boot {count} to the launcher")
            harness.time.sleep(0.25)
        self.assertEqual(len(b), count)

    def faults(self, emu) -> int:
        return emu.uart().count("ZEPHYR FATAL ERROR")

    def test_bad_face_falls_back_once(self):
        from tests.e2e.netsim import netsim
        nor = faulting_nor()
        faces = [273, 274, 275, 276, 277, 280]
        with netsim() as port, harness.Emu("wfguard", nor=nor, fixtures=bad_fixtures(), hci_port=port,
                                           seconds=1500) as emu:
            # boot 1 arms the flag and faults on 273; boot 2 finds it and makes 274 active
            self.wait_boot(emu, 2)
            emu.keep_awake()  # this QEMU cannot enter S3BT (~15 s idle)
            self.assertEqual(self.faults(emu), 1)
            self.assertRegex(boots(emu.uart())[1], r"m_u32CurrId:274")
            emu.sleep(5)
            self.assertEqual(guard_flag(emu), (274, LOADED, 0))
            emu.sleep(70)
            self.assertIsNone(guard_flag(emu))
            self.assertEqual(face_list(port), (1, faces))
            self.shot(emu, "1-fallback")

            # the bad face picked again from the phone: the screen goes off before it loads, longer than LOADED_MS
            # passes, it faults at the wake, and that one fault is enough
            darks = emu.uart().count("launcher early-suspend")
            self.assertEqual(wfm(port, bytes([1, 0, 6, 0]) + struct.pack("<6I", *faces)), b"\x00")
            end = harness.time.monotonic() + 120
            while emu.uart().count("launcher early-suspend") == darks:
                self.assertLess(harness.time.monotonic(), end, "the screen stayed on")
                harness.time.sleep(0.1)
            emu.sleep(3.5)
            emu.send("c 1", "c 0", pause=1)  # wakes on the "Paired" popup of that connection, over the face
            if emu.peek(harness.POPUP_FLAGS, 1)[0] & 3:
                emu.send("c 1", "c 0")  # closes it: now the face view loads the pick
            self.wait_boot(emu, 3)
            emu.keep_awake()
            emu.sleep(5)
            self.assertEqual(self.faults(emu), 2)
            self.assertEqual(harness.u32(emu, CURRENT_ID), 274)
            self.assertEqual(guard_flag(emu), (274, LOADED, 0))

            # take the bad face off the list (a built-in stays on NAND, a download goes at the next boot)
            self.assertEqual(wfm(port, bytes([1, 0, 5, 0]) + struct.pack("<5I", *faces[1:])), b"\x00")
            self.assertEqual(face_list(port), (0, faces[1:]))
            # an edit that keeps the active face does not put it back to "loading"
            self.assertEqual(guard_flag(emu), (274, LOADED, 0))
            # AT REBOOT skips the stock's storage flush that the menu reboot and power-off run (0x100C6234)
            emu.call(DATA_STORAGE_WRITE, 1, 0xFFFFFFFF, thread=b"ui_s")
            harness.ready(emu)  # the screen went off with the BLE session, a dark guest drops the next one
            harness.reboot(emu, port)
            emu.sleep(5)
            # a reset after the load counts a death and keeps the face
            self.assertEqual(guard_flag(emu), (274, LOADED, 1))
            self.assertEqual(face_list(port), (0, faces[1:]))

            # the menu's reboot drops the flag
            emu.call(UX2SYS_REBOOT, thread=b"ui_s")
            self.wait_boot(emu, 5)
            emu.keep_awake()
            emu.sleep(5)
            self.assertEqual(guard_flag(emu), (274, LOADED, 0))

            # so does the OTA's forced reboot (noreturn, so a jump rather than a call; ui_service has the stack for fs)
            emu.qmp.request("system_reset")
            self.wait_boot(emu, 6)
            emu.keep_awake()
            emu.sleep(5)
            self.assertEqual(guard_flag(emu), (274, LOADED, 1))
            ui = int.from_bytes(b"ui_s", "little")
            emu.gdb(f"tbreak *{harness.OS_MUTEX_LOCK & ~1:#x} if *(unsigned *)(*(unsigned *)"
                    f"{harness.CURRENT_THREAD:#x} + 0x78) == {ui:#x}", "continue",
                    "set $r0 = 2", f"set $pc = {OTA_FORCE_REBOOT:#x}")
            self.wait_boot(emu, 7)
            emu.keep_awake()
            emu.sleep(5)
            self.assertEqual(guard_flag(emu), (274, LOADED, 0))
            self.assertEqual(harness.u32(emu, CURRENT_ID), 274)
            self.assertEqual(self.faults(emu), 2)
            self.shot(emu, "2-after-reboot")
            print("wfguard run:", emu.run, "boots", len(boots(emu.uart())), "faults", self.faults(emu))

    def test_dark_screen_and_lost_list(self):
        """No keep_awake: the screen goes off 5 s into each boot and the LVGL timers stop. A reset there costs the
        face only the third time, also when that boot lost the saved list and built the default one."""
        with harness.Emu("wfguard-dark", nor=harness.patched_paired_nor()) as emu:
            for boot in range(1, 4):
                self.wait_boot(emu, boot)
                end = harness.time.monotonic() + 120
                while "launcher early-suspend" not in boots(emu.uart())[-1]:
                    self.assertLess(harness.time.monotonic(), end, "the screen stayed on")
                    harness.time.sleep(0.1)
                self.assertEqual(harness.u32(emu, CURRENT_ID), 273, f"boot {boot}")
                if boot < 3:
                    emu.qmp.request("system_reset")
                else:
                    # the default list path: the old hook ran before this list existed
                    emu.gdb("tbreak *0x100de068", "monitor system_reset", "continue",
                            f"set *(unsigned *){PARA_MIRROR:#x} = 0")
            self.wait_boot(emu, 4)
            emu.keep_awake()
            fourth = boots(emu.uart())[-1]
            self.assertRegex(fourth, r"cfgOk:0")
            self.assertEqual(harness.u32(emu, CURRENT_ID), 274)
            self.assertEqual(self.faults(emu), 0)
            print("wfguard dark run:", emu.run, "boots", 4, "faults", 0)


WP3_STUB = paths.ROOT / "target/wp3-stub.bin"
WP3_ELF = paths.ROOT / "fw/target/thumbv7em-none-eabi/release/wp3-stub"
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
    models one and walks every hook (tests/e2e/wp3_badface.gdb)."""

    def test_every_hook(self):
        cmf = paths.cmf_watch_utils()
        run = harness.RUNS / f"wfguard-wp3-{harness.time.strftime('%H%M%S')}"
        run.mkdir(parents=True)
        app, rom = wp3_preview(run)
        stub = WP3_STUB.read_bytes()
        udf = 0x102BD280 + stub.index(b"\x00\xde")
        loaded, survived = (harness.symbol(WP3_ELF, f"wp3_stub::{name}") & ~1 for name in ("loaded", "survived"))
        port = harness.free_port()
        out = run / "emu"
        emu = subprocess.Popen(["uv", "run", "--locked", "cmf-watch-utils", "emu", "run", "--profile", "wp3-stock",
                                "--headless", "--allow-reboot", "--app", str(app), "--rom", str(rom),
                                "--gdb-port", str(port), "--fixtures", str(cmf / "fixtures/wp3"),
                                "--qemu", str(cmf / "runtime/p10/qemu-system-arm"), "--seconds", "300",
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
                                  "-ex", f"set $loaded = {loaded:#x}",
                                  "-ex", f"set $survived = {survived:#x}", "-x", str(paths.ROOT / "tests/e2e/wp3_badface.gdb")],
                                 capture_output=True, text=True, timeout=1500).stdout
        finally:
            emu.wait(timeout=1500)
        (run / "gdb.log").write_text(gdb)
        uart = (out / "uart0.log").read_text(errors="replace")
        events = [line for line in gdb.splitlines()
                  if line.startswith(("INIT", "BADFACE", "FLAG", "SURVIVED", "PICKED", "REBOOT", "RESET", "LIST"))]
        picked = lambda face: f"FLAG id={face} stage={PICKED} deaths=0"
        armed = lambda face, deaths=0: f"FLAG id={face} stage={LOADING} deaths={deaths}"
        ran = lambda face, deaths=0: f"FLAG id={face} stage={LOADED} deaths={deaths}"
        self.assertEqual(events, [
            armed(276), "INIT id=276", "BADFACE fault id=276",  # boot 2 (boot 1 reboots out of the setup state)
            armed(275), "INIT id=275", ran(275), "SURVIVED, pick 276",  # 3: fell back at once, to the next built-in
            picked(276), "PICKED saved id=276", armed(276), "PICKED fault id=276",  # armed as it is parsed
            armed(275), "INIT id=275", ran(275), "REBOOT menu",  # 4: one fault for the pick
            armed(275), "INIT id=275", ran(275), "REBOOT ota",  # 5: the menu reboot dropped the flag
            armed(275), "INIT id=275", ran(275), "RESET",  # 6: so did the OTA reboot
            armed(275, 1), "INIT id=275", ran(275, 1), "RESET",  # 7
            armed(275, 2), "INIT id=275", ran(275, 2), "RESET",  # 8
            "LIST lost", armed(274), "INIT id=274", ran(274), "RESET",  # 9: the default list, the flag is stale
            "LIST lost", armed(274, 1), "INIT id=274", ran(274, 1), "RESET",  # 10
            "LIST lost", armed(274, 2), "INIT id=274", ran(274, 2), "RESET",  # 11
            "LIST lost", armed(276), "INIT id=276", "BADFACE fault id=276",  # 12: the default face's third death
            armed(275), "INIT id=275", ran(275),  # 13: two bad built-ins in a row, on to the third
        ])
        self.assertEqual((len(boots(uart)), uart.count("ZEPHYR FATAL ERROR")), (13, 3))
        print("wfguard wp3 run:", run, "boots", len(boots(uart)), "faults", uart.count("ZEPHYR FATAL ERROR"))


if __name__ == "__main__":
    unittest.main()
