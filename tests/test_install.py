import contextlib
import io
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
import zlib

from wp2apps import install, package


class InstallChunksTest(unittest.TestCase):
    def test_chunks_cover_file(self):
        data = bytes(range(256)) * 3
        parts = list(install.chunks(data))
        self.assertEqual(b"".join(p for _, p in parts), data)
        self.assertTrue(all(len(p) <= 128 for _, p in parts))
        self.assertEqual([o for o, _ in parts], list(range(0, len(data), 128)))


class FakeSession:
    def __init__(self, slots=(), stub=6):
        self.calls, self.slots, self.stub = [], dict(slots), stub

    async def request(self, c1, c2, payload, expect, timeout=6.0):
        self.calls.append((c2, payload))
        if c2 == install.INFO and payload[0] == install.STUB_SLOT:
            return b"\0" + struct.pack("<IB", self.stub, 3)
        if c2 == install.INFO:
            return b"\0" + self.slots.get(payload[0], b"")
        if c2 == install.DATA:
            return b"\0" + struct.pack("<I", struct.unpack_from("<I", payload)[0] + len(payload) - 4)
        return b"\0"


def sample(name="hello", kind=package.KIND_APP):
    return package.pack(bytes(64), name=name, version=3, kind=kind)


class InstallFlowTest(unittest.IsolatedAsyncioTestCase):
    async def test_first_free_slot(self):
        s = FakeSession({0: sample("other")[:64]})
        self.assertEqual(await install.install(s, sample()), 1)
        begin = [p for c, p in s.calls if c == install.BEGIN][0]
        self.assertEqual(struct.unpack("<BII", begin), (1, len(sample()), zlib.crc32(sample())))
        self.assertEqual(s.calls[-1][0], install.END)

    async def test_same_name_replaced(self):
        s = FakeSession({0: sample("other")[:64], 3: sample()[:64]})
        self.assertEqual(await install.install(s, sample()), 3)

    async def test_slots_8_to_15_from_stub_8(self):
        full = {n: sample(f"app{n}")[:64] for n in range(8)}
        with self.assertRaisesRegex(ValueError, "no free slot"):
            await install.install(FakeSession(full, stub=7), sample())
        s = FakeSession(full, stub=8)
        self.assertEqual(await install.install(s, sample()), 8)
        self.assertIn((install.INFO, bytes([15])), s.calls)
        self.assertNotIn((install.INFO, bytes([8])), FakeSession(stub=6).calls)

    async def test_loader_goes_to_loader_slot(self):
        self.assertEqual(await install.install(FakeSession(), sample("l", package.KIND_LOADER)), 0xFF)

    async def test_kind_and_slot_checked_before_begin(self):
        for data, slot in ((sample("l", package.KIND_LOADER), 2), (sample(), install.LOADER_SLOT)):
            s = FakeSession()
            with self.assertRaisesRegex(ValueError, "slot L"):
                await install.install(s, data, slot)
            self.assertEqual(s.calls, [])

    async def test_status_error(self):
        class Busy(FakeSession):
            async def request(self, *a, **k):
                return bytes([install.Status.BUSY])
        with self.assertRaisesRegex(ValueError, "BUSY"):
            await install.remove(Busy(), 0)

    async def test_stub_version(self):
        class Stub(FakeSession):
            def __init__(self, reply):
                super().__init__()
                self.reply = reply

            async def request(self, c1, c2, payload, expect, timeout=6.0):
                self.calls.append((c2, payload))
                return self.reply
        new = Stub(b"\0" + struct.pack("<I", 2) + b"\3")
        self.assertEqual(await install.stub(new), install.Stub(2, 3))
        self.assertEqual(new.calls, [(install.INFO, bytes([install.STUB_SLOT]))])
        # a stub from before the version was readable refuses slot 0xFE like any unknown slot
        self.assertEqual(await install.stub(Stub(bytes([install.Status.BAD_REQUEST]))), install.Stub(1, None))
        with self.assertRaisesRegex(ValueError, "unexpected"):
            await install.stub(Stub(b"\0\1"))

    async def test_info_empty_and_full(self):
        s = FakeSession({2: sample()[:64]})
        self.assertIsNone(await install.info(s, 0))
        self.assertEqual(await install.info(s, 2), install.Header("hello", 3, 0, 0))
        s = FakeSession({2: package.pack(bytes(64), name="lab", version=3, entry=16)[:64]})
        self.assertTrue((await install.info(s, 2)).icon)


class TrailerInstallTest(unittest.IsolatedAsyncioTestCase):
    async def test_trailer_stripped_before_stub_10(self):
        data = package.pack(bytes(64), name="w", version=1, widget_icons=[bytes(package.WIDGET_ICON_BYTES)])
        plain = package.strip_trailer(data)
        # 9 is the dangerous one: new enough to look right, but it is the watch face guard, without templates
        for stub, size in ((8, len(plain)), (9, len(plain)), (10, len(data)), (11, len(data))):
            s = FakeSession(stub=stub)
            await install.install(s, data)
            begin = [p for c, p in s.calls if c == install.BEGIN][0]
            self.assertEqual(struct.unpack("<BII", begin)[1], size, f"stub {stub}")


# Golden v2 record, also in sdk/cmf-wp2-app (columns_record_golden): other repos build test vectors with
# install.columns_record.
COLUMNS = bytes.fromhex("ff01034e6f7700000000000000002d33c2b000000000000032333a303000000000000134c2b00000000000000030"
                        "303a30300000000000ff2d2d0000000000000000")


class WidgetSetTest(unittest.TestCase):
    def test_no_v2_record_to_stub_9(self):
        from unittest import mock
        from wp2apps import cli
        for stub, sent in ((8, False), (9, False), (10, True)):
            s = FakeSession(stub=stub)

            async def with_watch(args, action):
                return await action(s)
            with mock.patch.object(cli, "with_watch", with_watch), contextlib.redirect_stdout(io.StringIO()), \
                    contextlib.redirect_stderr(io.StringIO()):
                code = cli.main(["widget", "set", "0", "--columns", "Now:0:1°", "--hci-port", "1"])
            self.assertEqual((code == 0, any(c == install.PUSH for c, _ in s.calls)), (sent, sent), f"stub {stub}")


class PushTest(unittest.IsolatedAsyncioTestCase):
    def test_columns_record(self):
        self.assertEqual(install.columns_record([("Now", 0, "-3°"), ("23:00", 1, "4°"), ("00:00", None, "--")]),
                         COLUMNS)
        self.assertEqual(len(install.columns_record([("a", None, "b")] * 4)), 87)
        cut = install.columns_record([("ééééé", 15, "x" * 20)])
        self.assertEqual(cut[3:13], "éééé".encode() + bytes(2), "cut at a char boundary, NUL kept")
        self.assertEqual(cut[14:24], b"x" * 9 + b"\0")
        for bad in ([], [("a", None, "b")] * 5, [("a", 16, "b")]):
            with self.assertRaises(ValueError):
                install.columns_record(bad)

    def test_column_arg(self):
        from wp2apps import cli
        self.assertEqual(cli.column_arg("23:00:1:-4°"), ("23:00", 1, "-4°"))
        self.assertEqual(cli.column_arg("Now::5"), ("Now", None, "5"))
        with self.assertRaises(Exception):
            cli.column_arg("Now:x:5")

    def test_widget_record_as_the_sdk(self):
        rec = install.widget_record("Weather", "21°\nSunny")
        self.assertEqual(len(rec), 64)
        self.assertEqual(rec[:8], b"Weather\0")
        self.assertEqual(rec[24:35], "21°\nSunny".encode() + b"\0")
        long = install.widget_record("é" * 20, "x" * 50)
        self.assertEqual(long[:24], "é".encode() * 11 + bytes(2), "cut at a char boundary, NUL kept")
        self.assertEqual(long[24:], b"x" * 39 + b"\0")

    async def test_push_frame_and_later(self):
        s = FakeSession()
        await install.push(s, 9, install.PUSH_BLOB, b"\x01\x02")
        self.assertEqual(s.calls, [(install.PUSH, bytes([9, 1, 1, 2]))])

        class Later(FakeSession):
            async def request(self, *a, **k):
                return bytes([install.Status.LATER]) + (1234).to_bytes(4, "little")
        with self.assertRaisesRegex(ValueError, "LATER, retry in 1234 ms"):
            await install.push(Later(), 0, install.PUSH_WIDGET, b"")


class CliTest(unittest.TestCase):
    def test_slot_arg(self):
        from wp2apps import cli
        self.assertEqual(cli.slot_arg("L"), 0xFF)
        self.assertEqual(cli.slot_arg("15"), 15)
        with self.assertRaises(Exception):
            cli.slot_arg("16")

    def test_stock_app_failure_is_error_with_stderr(self):
        from unittest import mock
        from wp2apps import cli, paths
        failed = subprocess.CalledProcessError(2, "fwkit", stderr="no such file\n")
        with mock.patch.object(paths, "firmware", return_value=Path("x")), \
                mock.patch.object(paths.subprocess, "run", side_effect=failed), \
                tempfile.TemporaryDirectory() as tmp, mock.patch("sys.stderr", new=io.StringIO()) as err:
            self.assertEqual(cli.main(["firmware", "--output", f"{tmp}/out.bin"]), 1)
        self.assertIn("no such file", err.getvalue())

    def test_connection_refused_is_error_not_traceback(self):
        from wp2apps import cli
        self.assertEqual(cli.main(["list", "--hci-port", "1"]), 1)
