"""The "<version>-fmc" About label (wp2apps.fmclabel, firmware.add_fmc_label).

Synthetic images cover both shapes (WP3: format word, WP2: thunk) and every refusal. The stock checks run only with
extracted stock app.bin files (never committed), as tests/test_model_stock.py takes them:

    WP2_STOCK_APP=/path/wp2-app.bin WP3_STOCK_APP=/path/wp3-app.bin \\
        uv run --locked python -m unittest tests.test_fmclabel -v
"""

import json
import os
from pathlib import Path
import struct
import tempfile
import unittest
from unittest import mock, skipUnless

from wp2apps import cli, firmware, fmclabel

BASE = 0x10000000
MODEL_JSON = Path(__file__).resolve().parent.parent / "fw" / "wp2-core" / "model.json"
MODELS = {name: {k: v["value"] for k, v in fields.items()} for name, fields in json.loads(MODEL_JSON.read_text()).items()}


def stock(env):
    path = os.environ.get(env)
    return Path(path).read_bytes() if path else None


WP2_STOCK, WP3_STOCK = stock("WP2_STOCK_APP"), stock("WP3_STOCK_APP")
# hand-checked disassembly of the two stock builds (docs/recon.md "About page label"); valid for these builds only
KNOWN = {
    "wp2": dict(version="1.0.0.73", info_pool_off=0xE1B2C, field=0x3F, about_proc=0x100BF9F9, getter_call=0x100BFCB8,
                info_id=0x11, page_add=0x100BFF60, page_rd=1, fmt_pool_off=None),
    "wp3": dict(version="1.0.1.77", info_pool_off=0xDB7DC, field=0x49, about_proc=0x100B3185, getter_call=0x100B2BFC,
                info_id=0x11, page_add=0x100B2E00, page_rd=3, fmt_pool_off=0xB2F38, fmt=b"%s\n(%s %s)"),
}

# synthetic layout (file offsets)
VERSION_AT, NAME_AT, FMT_AT = 0x3000, 0x3100, 0x3200
INFO_AT, INFO_POOL = 0x1000, 0x1100
REC_AT, PROC, FMT_POOL = 0x200, 0x1800, 0x1900
B = 0x88


def h(app, off, *hws):
    struct.pack_into(f"<{len(hws)}H", app, off, *hws)


def ldr(rt, at, pool):
    """16-bit `ldr rt, [pc, #..]` at file offset `at` loading the word at file offset `pool`."""
    imm = (pool - ((at + 4) & ~3)) // 4
    assert 0 <= imm < 256
    return 0x4800 | rt << 8 | imm


def synthetic(wp3=True, version=b"1.2.3.45", field=0x49, proc_word=2):
    """Zeros with one of each signature: the version string and its info fill, an about_view record and, near its
    proc, the getter call and the page's add.w (WP3 shape: r3 after an `ldr r2,=fmt`; WP2 shape: r1)."""
    app = bytearray(0x4000)
    app[VERSION_AT:VERSION_AT + len(version) + 1] = version + b"\0"
    struct.pack_into("<I", app, INFO_POOL, BASE + VERSION_AT)
    h(app, INFO_AT, 0x2214, ldr(1, INFO_AT + 2, INFO_POOL), 0xF104, field)  # movs r2,#0x14; ldr r1; add.w r0,r4,#F
    app[INFO_AT + 8:INFO_AT + 12] = firmware.thumb_bl(BASE + INFO_AT + 8, BASE + 0x2000)
    app[NAME_AT:NAME_AT + 11] = b"about_view\0"
    struct.pack_into("<I", app, REC_AT, BASE + NAME_AT)
    struct.pack_into("<I", app, REC_AT + 4 * proc_word, BASE + PROC | 1)
    h(app, PROC, 0xA900 | B // 4, 0x2011)  # add r1, sp, #B; movs r0, #0x11
    app[PROC + 4:PROC + 8] = firmware.thumb_bl(BASE + PROC + 4, BASE + 0x2100)
    page = PROC + 0x40
    if wp3:
        app[FMT_AT:FMT_AT + 12] = b"%s\n(%s %s)\0\0"
        struct.pack_into("<I", app, FMT_POOL, BASE + FMT_AT)
        h(app, page - 2, ldr(2, page - 2, FMT_POOL))
    h(app, page, 0xF10D, (3 if wp3 else 1) << 8 | B + field)  # add.w rD, sp, #(B+F)
    return app


def diff(a, b):
    return {i for i in range(min(len(a), len(b))) if a[i] != b[i]}


class SyntheticTest(unittest.TestCase):
    def test_locates_wp3_shape(self):
        s = fmclabel.locate(bytes(synthetic()))
        self.assertEqual((s.version, s.label, s.version_off), ("1.2.3.45", "1.2.3.45-fmc", VERSION_AT))
        self.assertEqual((s.info_pool_off, s.field), (INFO_POOL, 0x49))
        self.assertEqual((s.about_rec, s.about_proc), (BASE + REC_AT, BASE + PROC | 1))
        self.assertEqual((s.getter_call, s.info_id, s.page_add, s.page_rd), (BASE + PROC + 4, 0x11, BASE + PROC + 0x40, 3))
        self.assertEqual((s.fmt_pool_off, s.fmt), (FMT_POOL, b"%s\n(%s %s)"))

    def test_locates_wp2_shape(self):
        s = fmclabel.locate(bytes(synthetic(wp3=False, field=0x3F, proc_word=1)))
        self.assertEqual((s.field, s.page_rd, s.fmt_pool_off, s.fmt), (0x3F, 1, None, None))

    def test_format_word_patch(self):
        app = synthetic()
        out = bytearray(app)
        self.assertEqual(firmware.add_fmc_label(out, bytes(app)), "1.2.3.45-fmc")
        new = struct.unpack_from("<I", out, FMT_POOL)[0]
        self.assertEqual(new, BASE + len(app))
        self.assertEqual(out[len(app):], b"%s-fmc\n(%s %s)\0\0")
        self.assertLessEqual(diff(app, out), set(range(FMT_POOL, FMT_POOL + 4)))
        self.assertEqual(out[INFO_POOL:INFO_POOL + 4], app[INFO_POOL:INFO_POOL + 4])  # the info field stays stock

    def test_thunk_patch(self):
        app = synthetic(wp3=False, field=0x3F)
        out = bytearray(app)
        firmware.add_fmc_label(out, bytes(app))
        page = PROC + 0x40
        self.assertLessEqual(diff(app, out), set(range(page, page + 4)))
        thunk = fmclabel.bl_target(bytes(out), BASE + page)
        self.assertEqual(thunk, BASE + len(app))
        ldr_r1, bx_lr, label = struct.unpack_from("<HHI", out, len(app))
        self.assertEqual((ldr_r1, bx_lr, label), (0x4900, 0x4770, thunk + 8))  # ldr r1, [pc, #0]; bx lr
        self.assertEqual(out[label - BASE:].rstrip(b"\0"), b"1.2.3.45-fmc")
        self.assertEqual(len(out) % 4, 0)

    def test_offset_after_unaligned_end(self):
        app = synthetic()
        out = bytearray(app) + b"\1\2"
        firmware.add_fmc_label(out, bytes(app))
        self.assertEqual(struct.unpack_from("<I", out, FMT_POOL)[0], BASE + len(app) + 4)

    def test_label_length(self):
        self.assertEqual(fmclabel.fmc_label("1.0.1.77"), "1.0.1.77-fmc")
        self.assertEqual(len(fmclabel.fmc_label("123.456.789.123")), 19)
        with self.assertRaisesRegex(fmclabel.LocateError, "limit"):
            fmclabel.locate(bytes(synthetic(version=b"123.456.789.1234")))

    def test_refuses_without_unique_matches(self):
        cases = []
        app = synthetic(); app[VERSION_AT:VERSION_AT + 8] = b"x" * 8; cases.append((app, "version"))
        app = synthetic(); app[0x3400:0x3409] = b"9.9.9.9\0\0"; struct.pack_into("<I", app, 0x3500, BASE + 0x3400)
        cases.append((app, "version"))
        app = synthetic(); h(app, INFO_AT, 0x2213); cases.append((app, "info site"))
        app = synthetic(); app[NAME_AT:NAME_AT + 5] = b"xxxxx"; cases.append((app, "about_view"))
        app = synthetic(); struct.pack_into("<I", app, REC_AT + 0x14, BASE + NAME_AT)
        struct.pack_into("<I", app, REC_AT + 0x18, BASE + PROC + 0x101); cases.append((app, "about_view"))
        app = synthetic(); h(app, PROC + 0x40, 0xF10D, 0x0300 | B + 0x48); cases.append((app, "page site"))
        app = synthetic(); h(app, PROC + 0x60, 0xF10D, 0x0100 | B + 0x49); cases.append((app, "page site"))
        for app, what in cases:
            with self.subTest(what), self.assertRaisesRegex(fmclabel.LocateError, what):
                fmclabel.locate(bytes(app))

    def test_format_needs_a_single_reference(self):
        app = synthetic()
        struct.pack_into("<I", app, 0x1A00, BASE + FMT_AT)
        s = fmclabel.locate(bytes(app))
        self.assertIsNone(s.fmt_pool_off)  # then the r3 thunk instead
        out = bytearray(app)
        firmware.add_fmc_label(out, bytes(app))
        self.assertEqual(struct.unpack_from("<H", out, len(app))[0], 0x4B00)  # ldr r3, [pc, #0]

    def test_identical_view_table_copy_counts_once(self):
        app = synthetic()
        app[0x2800:0x2810] = app[REC_AT:REC_AT + 16]
        self.assertEqual(fmclabel.locate(bytes(app)).about_rec, BASE + REC_AT)


class CliTest(unittest.TestCase):
    def run_firmware(self, *argv):
        calls = []
        with tempfile.TemporaryDirectory() as tmp,                 mock.patch.object(cli.paths, "stock_app", return_value=b"wp2"),                 mock.patch.object(cli.paths, "stock_app_wp3", return_value=b"wp3"),                 mock.patch.object(firmware, "patch", side_effect=lambda *a, **k: calls.append(("wp2", k)) or b"x"),                 mock.patch.object(firmware, "patch_wp3", side_effect=lambda *a, **k: calls.append(("wp3", k)) or b"x"):
            stub = Path(tmp) / "stub.bin"
            stub.write_bytes(b"s")
            args = cli.parser().parse_args(["firmware", "--stub", str(stub), "--output", str(Path(tmp) / "o"), *argv])
            args.handler(args)
        return calls[0]

    def test_label_on_by_default(self):
        self.assertTrue(self.run_firmware()[1]["label"])
        self.assertTrue(self.run_firmware("--model", "wp3")[1]["label"])

    def test_opt_out(self):
        self.assertFalse(self.run_firmware("--no-fmc-label")[1]["label"])
        self.assertFalse(self.run_firmware("--model", "wp3", "--no-fmc-label")[1]["label"])


class StockTest(unittest.TestCase):
    def check_stock(self, name, img):
        s = fmclabel.locate(img)
        for k, v in KNOWN[name].items():
            self.assertEqual(getattr(s, k), v, k)
        # cross-check with the model table (fw/wp2-core model.rs): the record is in the stock view table, on a record
        # boundary, and its proc word (word 1 or 2) is the proc the locator used
        m = MODELS[name]
        start, end, size = (int(m[k], 16) for k in ("view_table", "view_table_end", "view_record"))
        self.assertTrue(start <= s.about_rec < end and (s.about_rec - start) % size == 0, hex(s.about_rec))
        words = struct.unpack_from("<3I", img, s.about_rec - BASE)
        self.assertEqual(words[0], BASE + img.index(b"\0about_view\0") + 1)
        self.assertIn(s.about_proc, words[1:])
        # the version field is the one AT GETVERSION prints: the info fill reads the version string
        self.assertEqual(struct.unpack_from("<I", img, s.info_pool_off)[0], BASE + s.version_off)
        return s

    def check_patch(self, img, out, s):
        firmware.check_acth(out)
        allowed = set(range(0x18, 0x24))
        allowed |= set(range(s.fmt_pool_off, s.fmt_pool_off + 4)) if s.fmt_pool_off else set(
            range(s.page_add - BASE, s.page_add - BASE + 4))
        self.assertLessEqual(diff(img, out), allowed)
        self.assertEqual(out[s.info_pool_off:s.info_pool_off + 4], img[s.info_pool_off:s.info_pool_off + 4])

    @skipUnless(WP3_STOCK, "needs WP3_STOCK_APP")
    def test_wp3(self):
        s = self.check_stock("wp3", WP3_STOCK)
        stub = b"WP3S" + struct.pack("<12I", 3, 0x102BD29D, 0x102BD2A1, 0x102BD2A9, 0x102BD2B1, 0x102BD2B9,
                                     0x102BD2C1, 0x102BD2C9, 0x102BD2D1, 0x102BD2E0, 0x102BD2E5, 0x102BD2F1) + bytes(0x100)
        plain = firmware.patch_wp3(WP3_STOCK, stub)
        out = firmware.patch_wp3(WP3_STOCK, stub, label=True)
        self.check_patch(plain, out, s)
        self.assertEqual(out[len(plain):].rstrip(b"\0"), b"%s-fmc\n(%s %s)")
        self.assertEqual(struct.unpack_from("<I", out, s.fmt_pool_off)[0], BASE + len(plain))
        # a loader build (stub + view table copy) still locates the same sites
        self.assertEqual(fmclabel.locate(plain), s)

    @skipUnless(WP2_STOCK, "needs WP2_STOCK_APP")
    def test_wp2(self):
        s = self.check_stock("wp2", WP2_STOCK)
        stub = b"WP2S" + struct.pack("<7I", 1, 0x1025A0A1, 0x1025A0C1, 0x1025A0E1, 0x1025A100, 0, 0) + bytes(0x100)
        plain = firmware.patch(WP2_STOCK, stub)
        out = firmware.patch(WP2_STOCK, stub, label=True)
        self.check_patch(plain, out, s)
        thunk = fmclabel.bl_target(out, s.page_add)
        self.assertEqual(thunk, BASE + len(plain))
        self.assertEqual(struct.unpack_from("<HHI", out, len(plain)), (0x4900, 0x4770, thunk + 8))
        self.assertEqual(out[thunk + 8 - BASE:].rstrip(b"\0"), b"1.0.0.73-fmc")
        # the page site is `add.w r1, sp, #0x4b` in stock
        self.assertEqual(WP2_STOCK[s.page_add - BASE:s.page_add - BASE + 4], bytes.fromhex("0df14b01"))
        # without a stub the label still gets covered by the ACTH size
        firmware.check_acth(firmware.patch(WP2_STOCK, weak=350, label=True))


if __name__ == "__main__":
    unittest.main()
