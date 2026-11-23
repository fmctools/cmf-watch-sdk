"""ABI 1.1 in the emulator (docs/abi-1.1.md): host channel, storage, time, battery, wake lock, ABI 1.0 apps.
Needs target/{loader,api-demo,hello-abi10}.wp2app; hello-abi10 is `hello` built from the ABI 1.0 SDK (87c73c3) and
packed as `hello10`. Run with QEMU_WP2_HOST_RTC=1 (set below) and WP2_NETSIM_INSTANCE of your own."""
import asyncio
from contextlib import ExitStack, asynccontextmanager
import datetime
import os
import re
import struct
import time
import unittest
from unittest import skipUnless

from tests.e2e import harness
from pathlib import Path

from wp2apps import channel, cli, h4, link, paths

S = harness.STUB_RAM
RUNNING = S + 6
MSGBOX_POPUP, MSGBOX_CLOSE, BTCALL_MSGBOX = 0x100D5E35, 0x100D6461, 0xF6  # incoming-call box, as test_e2e
# stock wake-lock bitmap (0x101B59A8): word 0x10 + id / 32 of 0x38007708, bit id % 32; the loader locks 0xF0
WAKE_WORD, WAKE_BIT = 0x38007708 + (0x10 + 0xF0 // 32) * 4, 1 << (0xF0 % 32)
DEMO_ELF = paths.ROOT / "sdk/target/thumbv7em-none-eabi/release/api-demo"
FILE, NAME = S + 0xE40, S + 0xE20  # past StubRam, as harness.write_file


KEY = {}  # session key of the bind made before the app opens; reused so no "Paired" popup ends the app


@asynccontextmanager
async def session(port):
    async with h4.gatt_over_h4(port) as gatt:
        s = link.Session()
        await s.handshake(gatt, key=KEY.get(port))
        KEY[port] = s.key
        yield s


async def unpaused(s, timeout: float = 20) -> float:
    """Waits for the app to leave a pause (a popup of an earlier step)."""
    t0 = time.monotonic()
    while (await channel.ping(s)).flags & 2:
        if time.monotonic() - t0 > timeout:
            raise TimeoutError("app still paused")
        await asyncio.sleep(0.25)
    return time.monotonic() - t0


async def flush(s):
    """Drops EVENTs queued for this host key before now (a CLI send exits before it collects the app's reply)."""
    await channel.ping(s)
    await asyncio.sleep(0.1)
    while not s.events.empty():
        s.events.get_nowait()


async def ask(s, text: str, timeout: float = 10) -> str:
    await flush(s)
    await channel.send(s, text.encode())
    return (await channel.recv(s, timeout)).data.decode()


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd, gdb and llvm-nm)")
class ApiTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        os.environ["QEMU_WP2_HOST_RTC"] = "1"
        cls.stack = ExitStack()
        cls.emu, cls.port = cls.stack.enter_context(harness.installed(["loader", "api-demo", "hello-abi10"], name="api"))

        async def bind():
            async with session(cls.port):
                pass

        asyncio.run(bind())
        os.environ["WP2APPS_SESSIONS"] = str(cls.emu.run / "sessions.json")
        cli.SESSIONS = Path(os.environ["WP2APPS_SESSIONS"])
        cli.cache_key(f"hci:{cls.port}", KEY[cls.port])
        harness.open_app(cls.emu, row=0)  # ready() closes the "Paired" popup of that bind first

    @classmethod
    def tearDownClass(cls):
        uart = cls.emu.uart()
        cls.stack.close()
        for bad in ("MllcFailedHook", "ZEPHYR FATAL ERROR"):
            assert bad not in uart, bad

    def run_async(self, coro):
        return asyncio.run(coro)

    def running(self) -> int:
        return self.emu.peek(RUNNING, 1)[0]

    def wake_held(self) -> bool:
        return bool(harness.u32(self.emu, WAKE_WORD) & WAKE_BIT)

    def stats(self):
        base = harness.u32(self.emu, S + harness.APP_BLOCK_OFF) + harness.symbol(DEMO_ELF, "STATS")
        return struct.unpack("<5i", self.emu.peek(base, 20))

    def setUp(self):
        if self._testMethodName in ("test_1_channel_and_wake_lock", "test_2_statuses", "test_3_time_and_battery",
                                    "test_4_storage", "test_4b_log", "test_5_screen_lock_effect"):
            self.launch(0)

    def launch(self, slot: int):
        """Starts the app of `slot` (= menu row here): from the menu if it shows, else through harness.open_app."""
        if self.running() == slot:
            return
        if self.emu.peek(harness.POPUP_FLAGS, 1)[0] & 3:
            harness.close_popup(self.emu)
            self.emu.keep_awake()  # closing the popup sets the stock auto-sleep back to 5 s
        if self.running() != 0xFF:
            self.emu.send("c 1", "c 0", pause=2)
        if self.emu.peek(S + 4, 2) == b"\x01\x01":
            y = 180 + 128 * slot
            self.emu.send(f"t 1 200 {y}", f"t 0 200 {y}", pause=2)
        if self.running() != slot:
            harness.open_app(self.emu, row=slot)

    def test_1_channel_and_wake_lock(self):
        async def run():
            async with session(self.port) as s:
                self.assertEqual(await channel.ping(s), channel.State(0, 1))
                self.assertEqual(await ask(s, "!keep 1"), "keep 1")
                rtt = []
                for i in range(20):
                    data = bytes([i]) * 128
                    t0 = time.monotonic()
                    await channel.send(s, data)
                    m = await channel.recv(s, 10)
                    rtt.append(time.monotonic() - t0)
                    self.assertEqual((m.slot, m.data), (0, data))
                n, t0 = 30, time.monotonic()
                for i in range(n):
                    await channel.send(s, b"!x" + bytes(126))  # unknown command: one short reply each
                up = time.monotonic() - t0
                for _ in range(n):
                    self.assertEqual((await channel.recv(s, 10)).data, b"unknown")
                await channel.send(s, b"!spam 40")
                t0 = time.monotonic()
                got = [(await channel.recv(s, 20)) for _ in range(41)]
                down = time.monotonic() - t0
                self.assertEqual([m.data for m in got[:40]], [f"spam {i}".encode() for i in range(40)])
                seqs = [m.seq for m in got]
                self.assertEqual(seqs, [(seqs[0] + i) % 256 for i in range(41)], "EVENT seq gap")
                return sorted(rtt), up / n, down / 40

        rtt, up, down = self.run_async(run())
        print(f"\necho 128 B round trip: median {rtt[10] * 1000:.0f} ms, max {rtt[-1] * 1000:.0f} ms; "
              f"host->app {up * 1000:.0f} ms/msg (acked); app->host {down * 1000:.0f} ms/msg")
        self.assertTrue(self.wake_held())
        # the BLE disconnect blanks the panel ~6 s later without a lock (harness.ready), and that closes view 0xF0
        self.emu.sleep(15)
        self.assertEqual(self.running(), 0, "app stopped although it holds the wake lock")
        out = harness.cli_out(["send", "!count", "--hci-port", self.port])
        self.assertEqual(out.strip(), '{"sent": 6}')

        async def reconnect():
            ch = channel.Channel(hci_port=self.port)
            ch.key = KEY[self.port]
            try:
                await ch.connect()
                await flush(ch.session)
                await ch.send(b"one", timeout=60)
                self.assertEqual((await ch.recv(timeout=10)).data, b"one")
                await ch.drop()
                await ch.send(b"two", timeout=60)
                self.assertEqual((await ch.recv(timeout=10)).data, b"two")
            finally:
                await ch.close()

        self.run_async(reconnect())

    def test_2_statuses(self):
        async def run():
            async with session(self.port) as s:
                await unpaused(s)
                with self.assertRaises(channel.NoApp):
                    await channel.send(s, b"x", slot=3)
                self.emu.call(MSGBOX_POPUP, BTCALL_MSGBOX, 0)
                harness.wait_popup(self.emu, True)
                self.emu.sleep(0.5)
                self.assertFalse(self.wake_held(), "wake lock kept under a popup")
                self.assertEqual((await channel.ping(s)).flags, 3)
                with self.assertRaisesRegex(TimeoutError, "PAUSED"):
                    await channel.send(s, b"x", wait=1)
                self.emu.call(MSGBOX_CLOSE, BTCALL_MSGBOX, 0)
                harness.wait_popup(self.emu, False)
                self.emu.sleep(0.5)
                self.assertTrue(self.wake_held())
                self.assertEqual(await ask(s, "ok"), "ok")

        self.run_async(run())

    def test_3_time_and_battery(self):
        async def run():
            async with session(self.port) as s:
                await unpaused(s)
                t = await ask(s, "!time")
                host = datetime.datetime.now()
                return t, host, await ask(s, "!bat")

        t, host, plugged = self.run_async(run())
        print(f"\n{t} (host {host:%H:%M:%S}); {plugged}")
        m = re.fullmatch(r"time (\d+)-(\d+)-(\d+) (\d+):(\d+):(\d+)\.(\d+) wd(\d) tz(-?\d+)", t)
        self.assertIsNotNone(m, t)
        watch = datetime.datetime(*map(int, m.groups()[:6]))
        self.assertLess(abs((watch - host).total_seconds()), 10)
        self.assertEqual(int(m.group(8)), (host.weekday() + 1) % 7)
        self.check_battery(plugged, "0")

    def check_battery(self, text, plugged):
        b = re.fullmatch(r"bat (\d+) c(\d) f(\d) d(\d)", text)
        self.assertIsNotNone(b, text)
        self.assertTrue(0 < int(b.group(1)) <= 100)
        self.assertEqual((b.group(2), b.group(4)), (plugged, plugged))

    def test_7_charger(self):
        """Last: on the charger the stock charge popup comes back at every screen wake."""
        self.emu.send("p 4000 1 60", pause=0)
        harness.wait_popup(self.emu, True)
        harness.close_popup(self.emu)
        self.emu.keep_awake()  # closing the popup sets the stock auto-sleep back to 5 s
        self.launch(0)

        async def run():
            async with session(self.port) as s:
                await unpaused(s)
                return await ask(s, "!bat")

        text = self.run_async(run())
        print(f"\n{text}")
        self.check_battery(text, "1")

    def test_4_storage(self):
        async def first():
            async with session(self.port) as s:
                await unpaused(s)
                before = int((await ask(s, "!count")).split()[1])
                self.assertEqual(await ask(s, "!inc"), f"inc {before + 1} 0")
                self.assertEqual(await ask(s, "!inc"), f"inc {before + 2} 0")
                self.assertEqual(await ask(s, "!save 4096"), "save 4096 0 4096 1")
                self.assertEqual(await ask(s, "!save 4097"), "save 4097 -1 4 0")
                return before + 2

        count = self.run_async(first())
        self.emu.send("c 1", "c 0", pause=2)  # BUTTON: main returns 0
        self.assertEqual(self.running(), 0xFF)
        self.assertFalse(self.wake_held(), "wake lock kept after exit")
        self.launch(0)
        self.assertEqual(self.stats()[4], 4, "load() at start")

        async def second():
            async with session(self.port) as s:
                await unpaused(s)
                self.assertEqual(await ask(s, "!keep 1"), "keep 1")
                self.assertEqual(await ask(s, "!count"), f"count {count}")

        self.run_async(second())

    def test_4b_log(self):
        """Every message becomes a line of the LVGL log, newest on top; the crown scrolls the page. Frames go to
        runs/api-log/. The 8x8 Cyrillic font of the pixel path is covered by wp2-core's gfx tests."""
        texts = ["привет ПРИВЕТ Ёё ъЩ€", *(f"line {i}" for i in range(8))]

        async def run():
            async with session(self.port) as s:
                for t in texts:
                    self.assertEqual(await ask(s, t), t)

        self.run_async(run())
        time.sleep(1)
        out = paths.ROOT / "runs" / "api-log"
        out.mkdir(parents=True, exist_ok=True)
        from tests.test_demo_e2e import save_png
        logs = []
        for name in ("top", "scrolled"):
            w, h, rgb = self.emu.frame()
            save_png(out / f"{name}.png", w, h, rgb)
            logs.append(rgb[200 * w * 3:])  # under the status line, which ticks every second
            self.emu.send(*["r -80"] * 6, pause=1.5)
        self.assertTrue(any(logs[0]), "empty log")
        self.assertNotEqual(logs[0], logs[1], "the crown did not scroll the log")

    def autosleep(self, ms: int):
        self.emu.gdb(f"set *(unsigned *)*(unsigned *){harness.AWAKE_PTR:#x} = {ms}")

    def blanked_within(self, seconds: float) -> bool:
        mark = len(self.emu.uart())
        self.emu.sleep(seconds)
        return "display blanking on" in self.emu.uart()[mark:]

    def test_5_screen_lock_effect(self):
        """The harness turns auto-sleep off (keep_awake); here it is 3 s, so only the lock keeps the panel on."""
        async def keep(on):
            async with session(self.port) as s:
                await unpaused(s)
                self.assertEqual(await ask(s, f"!keep {on}"), f"keep {on}")

        async def release():
            # the link stays up until the panel is on again: 7 s after a disconnect with the panel off the stock
            # enters deep sleep (S3BT), which stops the emulator
            async with session(self.port) as s:
                await unpaused(s)
                self.assertEqual(await ask(s, "!keep 0"), "keep 0")
                try:
                    self.emu.sleep(0.5)
                    self.assertFalse(self.wake_held())
                    self.assertTrue(self.blanked_within(10), "panel stayed on after the lock was released")
                finally:
                    self.emu.keep_awake()
                    self.emu.send("c 1", "c 0", pause=2)  # wakes the panel

        self.run_async(keep(1))
        self.autosleep(3000)
        try:
            self.assertFalse(self.blanked_within(10), "panel blanked while the app held the lock")
            self.assertEqual(self.running(), 0)
        except BaseException:
            self.emu.keep_awake()
            raise
        self.run_async(release())

    def test_6_abi10_app(self):
        self.launch(1)
        self.assertEqual(self.running(), 1, "ABI 1.0 hello did not start")

        async def run():
            async with session(self.port) as s:
                state = await channel.ping(s)
                self.assertEqual((state.running_slot, state.flags & 1), (1, 0))
                with self.assertRaises(channel.NoApp):
                    await channel.send(s, b"x")

        self.run_async(run())
        self.emu.send("c 1", "c 0", pause=2)
        self.assertEqual(self.running(), 0xFF)

    def test_8_remove_drops_data(self):
        for _ in range(5):  # on the charger the charge popup comes back at each wake and takes the button
            if self.running() == 0xFF:
                break
            if self.emu.peek(harness.POPUP_FLAGS, 1)[0] & 3:
                harness.close_popup(self.emu)
            else:
                self.emu.send("c 1", "c 0", pause=2)
        self.assertEqual(self.running(), 0xFF)
        self.assertTrue(self.exists(b"/NAND:/wp2apps/0.dat"))
        harness.cli_out(["remove", "--slot", "0", "--hci-port", self.port])
        self.assertFalse(self.exists(b"/NAND:/wp2apps/0.dat"))
        self.assertFalse(self.exists(b"/NAND:/wp2apps/0.new"))

    def exists(self, path: bytes) -> bool:
        harness.ready(self.emu)
        self.emu.poke(NAME, path + b"\0")
        self.emu.poke(FILE, bytes(0x2C))
        ui = b"ui_s"
        rc = self.emu.call(harness.FS_OPEN, FILE, NAME, 1, thread=ui)
        if rc == 0:
            self.emu.call(harness.FS_CLOSE, FILE, thread=ui)
        return rc == 0


if __name__ == "__main__":
    unittest.main()
