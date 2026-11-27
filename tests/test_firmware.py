import lzma
import os
from pathlib import Path
import struct
import tempfile
import unittest
from unittest import skipUnless

from wp2apps import cli, firmware, paths

try:
    STOCK = paths.stock_app()
except (FileNotFoundError, ImportError):
    STOCK = None
HAVE_STOCK = STOCK is not None
try:
    STOCK_WP3 = paths.stock_app_wp3()
except (FileNotFoundError, ImportError):
    STOCK_WP3 = None


BUILT_STUB = paths.ROOT / "target/wp2-stub.bin"


def fake_stub(boot=0x1025A0A1, ble=0x1025A0C1, view_proc=0x1025A0E1, view_name=0x1025A100, applist=0, widget=0,
              entry=None, guard=None, types=None,
              proc_list=None, image_cf=None, image_unpack=None, source=None):
    """entry: a widget registry entry placed at WIDGET_AT; guard: a header version 2 with those five hooks; types: a
    header version 3 with that procedural widget table, placed at TYPES_AT; proc_list: its image list hook;
    image_cf, image_unpack: the image loader's hooks; source: the data source hook."""
    fields = (boot, ble, view_proc, view_name, applist, widget)
    version = 1
    if guard is not None or types is not None:
        fields, version = fields + tuple(guard or (SKIP,) * 5), 2
    if types is not None:
        fields, version = fields + (TYPES_AT, proc_list or 0, image_cf or 0, image_unpack or 0, source or 0), 3
    stub = bytearray(b"WP2S" + struct.pack(f"<I{len(fields)}I", version, *fields) + bytes(0x100 if types is None else 0x200))
    if types is not None:
        at = TYPES_AT - 0x1025A080
        stub[at:at + len(types)] = types
    if entry is not None:
        at = WIDGET_AT - 0x1025A080
        stub[at:at + len(entry)] = entry
    return bytes(stub)


WIDGET_AT = 0x1025A140
CALLBACKS = (0x1025A0A1, 0x1025A0C1, 0x1025A0E1, 0x1025A0A1, 0x1025A0A1)


def widget_entry(wid=0x40, callbacks=CALLBACKS):
    return firmware.WGT_ENTRY.pack(wid, 3, 40, 0, 0xF0, *callbacks)


TYPES_AT = 0x1025A180
SKIP = 0x1025A0E1
# the data source dispatcher's "no such source" branch (0x100DAA90): mvn r0, #0xf
SOURCE_UNKNOWN, MVN_R0_15 = 0x100DAACC, bytes.fromhex("6ff00f00")
# the parser of tag 0x82 looks the widget's first image up here and drops a widget without one
PROC_LIST_CALL, STOCK_IMAGE_LIST = 0x100DB378, 0x101B7810
# the face's image loader maps the header's format and unpacks LZ4; the header reader 0x101B8E28, whose format the
# items are drawn with, maps it again
IMAGE_CF_CALLS, STOCK_IMAGE_CF = (0x100DB7B6, 0x101B8E5E), 0x101B8FD2
IMAGE_UNPACK_CALL, STOCK_IMAGE_UNPACK = 0x100DB9F0, 0x101822C0


def type_table(stock=bytes(0x60), sentinel=(0xFF, SKIP, SKIP, SKIP, SKIP, 0), added=b""):
    return stock + added + struct.pack("<6I", *sentinel)


def lookup(app, table, kind, limit=4096):
    """What the five stock loops do with a face's widget type: the matched entry, None for "skip"."""
    for i in range(limit):
        at = table - 0x10000000 + 0x18 * i
        value = struct.unpack_from("<H", app, at)[0]
        if value == kind:
            return at + 0x10000000
        if value == 0xFF:
            return None
    raise AssertionError(f"the loop for type {kind:#x} does not end")
def wp3_stub(hooks):
    return b"WP3S" + struct.pack("<6I", 1, *hooks) + bytes(0x40)


def synthetic_app():
    """Zeros of the stock app's length with the stock words add_stub checks before it rewrites them."""
    app = bytearray(firmware.STUB_OFFSET)
    for lits, value in ((firmware.VIEW_START_LITERALS, firmware.VIEW_TABLE),
                        (firmware.VIEW_END_LITERALS, firmware.VIEW_TABLE_END)):
        for lit in lits:
            struct.pack_into("<I", app, lit - 0x10000000, value)
    for lit in firmware.PROC_TYPES_LITERALS:
        struct.pack_into("<I", app, lit - 0x10000000, firmware.PROC_TYPES)
    app[SOURCE_UNKNOWN - 0x10000000:SOURCE_UNKNOWN - 0x10000000 + 4] = MVN_R0_15
    struct.pack_into("<I", app, firmware.APPLIST_LAYOUT - 0x10000000, firmware.STOCK_APPLIST_LAYOUT)
    for at, target in ((firmware.BOOT_CALL, firmware.STOCK_HEAP_INIT), (firmware.BLE_CALL, firmware.STOCK_BLE_READ),
                       *((at, target) for at, target, _ in firmware.GUARD_WP2),
                       (PROC_LIST_CALL, STOCK_IMAGE_LIST), *((c, STOCK_IMAGE_CF) for c in IMAGE_CF_CALLS),
                       (IMAGE_UNPACK_CALL, STOCK_IMAGE_UNPACK)):
        app[at - 0x10000000:at - 0x10000000 + 4] = firmware.thumb_bl(at, target)
    return app


class FirmwareTest(unittest.TestCase):
    def test_thumb_bl_known_encoding(self):
        self.assertEqual(firmware.thumb_bl(0x100D34F0, 0x100CC284), bytes.fromhex("f8f7c8fe"))

    def test_thumb_range_checked(self):
        with self.assertRaises(ValueError):
            firmware.thumb_bl(0x10000000, 0x12000000)

    def test_thumb_b_w_roundtrip(self):
        self.assertEqual(firmware.thumb_b_w(0x10000000, 0x10000004), bytes.fromhex("00f000b8"))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_thumb_bl_matches_stock_encoding(self):
        self.assertEqual(firmware.thumb_bl(0x101884FA, 0x10146AA4), STOCK[0x1884FA:0x1884FE])

    def test_fix_acth_rule(self):
        app = bytearray(b"ACTHHTCA" + bytes(0x1F8) + os.urandom(4096))
        firmware.fix_acth(app)
        size, checksum = struct.unpack_from("<II", app, 0x18)
        self.assertEqual(size, len(app) - 0x200)
        self.assertEqual(firmware.sum32(app[:0x200]), 0xFFFFFFFF)
        self.assertEqual((firmware.sum32(app[0x200:0x200 + size]) + checksum) & 0xFFFFFFFF, 0xFFFFFFFF)

    def test_fix_acth_needs_alignment(self):
        with self.assertRaises(ValueError):
            firmware.fix_acth(bytearray(0x203))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_fix_acth_reproduces_stock(self):
        size = struct.unpack_from("<I", STOCK, 0x18)[0]
        app = bytearray(STOCK[:0x200 + size])
        firmware.fix_acth(app)
        self.assertEqual(bytes(app), STOCK[:0x200 + size])

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_patch_keeps_stock_acth_semantics(self):
        firmware.check_acth(STOCK)
        plain = firmware.patch(STOCK)
        self.assertEqual(plain[:0x1C], STOCK[:0x1C])
        self.assertEqual(plain[0x24:0x200], STOCK[0x24:0x200])
        with_stub = firmware.patch(STOCK, fake_stub())
        firmware.check_acth(with_stub)
        self.assertEqual(struct.unpack_from("<I", with_stub, 0x18)[0], len(with_stub) - 0x200)
        broken = bytearray(with_stub)
        broken[-1] ^= 1
        with self.assertRaises(ValueError):
            firmware.check_acth(broken)

    def test_put_checks_old_bytes(self):
        app = bytearray(b"abcd")
        firmware.put(app, 0x10000000, b"ab", b"xy")
        self.assertEqual(app, b"xycd")
        with self.assertRaisesRegex(ValueError, "0x10000002"):
            firmware.put(app, 0x10000002, b"zz", b"..")

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_heap_literals_patched(self):
        out = firmware.patch(STOCK)
        for off in firmware.HEAP_LITERALS:
            self.assertEqual(struct.unpack_from("<I", out, off)[0], firmware.HEAP_NEW)
        diff = [i for i in range(0x200, len(STOCK)) if out[i] != STOCK[i]]
        self.assertEqual({i & ~3 for i in diff}, set(firmware.HEAP_LITERALS))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_patch_refuses_other_bytes(self):
        bad = bytearray(STOCK)
        bad[0x188508] ^= 1
        with self.assertRaisesRegex(ValueError, "0x10188508"):
            firmware.patch(bytes(bad), _verify=False)
        with self.assertRaisesRegex(ValueError, "sha256"):
            firmware.patch(bytes(bad))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_stub_patch_points(self):
        stub = fake_stub(boot=0x1025A0A1, ble=0x1025A0C1, view_proc=0x1025A0E1, view_name=0x1025A100)
        out = firmware.patch(STOCK, stub)
        self.assertEqual(out[0x25A080:0x25A080 + len(stub)], stub)
        self.assertEqual(out[0x1884FA:0x1884FE], firmware.thumb_bl(0x101884FA, 0x1025A0A0))
        self.assertEqual(out[0xD34F0:0xD34F4], firmware.thumb_bl(0x100D34F0, 0x1025A0C0))
        table = struct.unpack_from("<I", out, 0x1018BC24 - 0x10000000)[0]
        end = struct.unpack_from("<I", out, 0x1018BC28 - 0x10000000)[0]
        self.assertEqual(table % 8, 0)
        self.assertEqual((end - table) // 0x18, 162 + 17)  # stock, 0xF0, the pages 0xF8..0x107
        ours = end - 17 * 0x18
        self.assertEqual(out[table - 0x10000000:ours - 0x10000000], STOCK[0x4B8:0x13E8])
        for i, view_id in enumerate((0xF0, *range(0xF8, 0x108))):
            entry = out[ours + i * 0x18 - 0x10000000:ours + (i + 1) * 0x18 - 0x10000000]
            self.assertEqual(struct.unpack_from("<IIIIHHHH", entry), (0x1025A100, 0x1025A0E1, 0, 0, view_id, 0x102, 0x1D2, 0x1D2))
        self.assertEqual(struct.unpack_from("<I", out, 0x1018DF3C - 0x10000000)[0], table)
        self.assertEqual(struct.unpack_from("<I", out, 0x1018DF40 - 0x10000000)[0], end)
        # neighbouring tables that merely share a bound with the view table (docs/recon.md)
        for lit in (0x101461AC, 0x10055458):
            self.assertEqual(out[lit - 0x10000000:lit - 0x10000000 + 4], STOCK[lit - 0x10000000:lit - 0x10000000 + 4])

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_applist_patch(self):
        out = firmware.patch(STOCK, fake_stub(applist=0x1025A121))
        desc = 0x101EB154 - 0x10000000
        self.assertEqual(struct.unpack_from("<I", out, desc + 0xC)[0], 0x1025A121)
        self.assertEqual(out[desc:desc + 0xC] + out[desc + 0x10:desc + 0x30],
                         STOCK[desc:desc + 0xC] + STOCK[desc + 0x10:desc + 0x30])
        base = firmware.patch(STOCK, fake_stub())
        self.assertEqual(base[desc:desc + 0x30], STOCK[desc:desc + 0x30])
        diff = {i & ~3 for i in range(0x200, 0x25A080) if out[i] != base[i]}
        self.assertEqual(diff, {desc + 0xC})

    def test_motor_hook_encoding(self):
        # llvm-objdump of movw r12, #358; mul r3, r3, r12; lsrs r3, r3, #10; b.w 0x100FB4B4 at 0x10259F00
        hook = firmware.motor_hook(350)
        self.assertEqual(hook[:10], bytes.fromhex("40f2661c03fb0cf39b0a"))
        self.assertEqual(hook[10:], firmware.thumb_b_w(0x10259F0A, 0x100FB4B4))
        self.assertEqual(firmware.motor_hook(999)[:4], bytes.fromhex("40f2ff3c"))  # movw r12, #1023
        for permille in (1, 350, 999):
            k = firmware.motor_q10(permille)
            for p in range(256):
                self.assertLessEqual(abs((p * k >> 10) - p * permille // 1000), 1)
        self.assertEqual(255 * firmware.motor_q10(350) >> 10, 89)
        for bad in (0, 1000):
            with self.assertRaises(ValueError):
                firmware.motor_q10(bad)

    def test_changed_ranges(self):
        old = bytes(100)
        new = bytearray(old)
        new[3], new[10], new[60], new[61] = 1, 1, 1, 1
        self.assertEqual(firmware.changed_ranges(old, bytes(new)), [(3, 11), (60, 62)])
        self.assertEqual(firmware.changed_ranges(old, old), [])

    def test_ota_xml_set(self):
        xml = ("<v><version_code>0x10000</version_code></v><partition><file_name>a.bin</file_name><file_size>0x1</file_size>"
               "</partition><partition><file_name>b.bin</file_name><file_size>0x2</file_size></partition>")
        out = firmware.ota_xml_set(xml, "b.bin", file_size=0x30)
        self.assertIn("<file_name>a.bin</file_name><file_size>0x1</file_size>", out)
        self.assertIn("<file_name>b.bin</file_name><file_size>0x30</file_size>", out)
        self.assertIn("<version_code>0x10002</version_code>", firmware.ota_xml_set(xml, version_code=0x10002))
        with self.assertRaisesRegex(ValueError, "c.bin"):
            firmware.ota_xml_set(xml, "c.bin", file_size=1)
        with self.assertRaisesRegex(ValueError, "file_size"):
            firmware.ota_xml_set(xml, file_size=1)  # two of them: refuse

    def test_ota_xml_only(self):
        xml = ("<p><partitionsNum>3</partitionsNum>\n\t<partition><file_name>a.bin</file_name></partition>\n\t<partition>"
               "<file_name>TEMP.bin</file_name></partition>\n\t<partition><file_name>c.bin</file_name></partition>\n\t</p>")
        self.assertEqual(firmware.ota_xml_only(xml, "TEMP.bin"),
                         "<p><partitionsNum>1</partitionsNum>\n\t<partition><file_name>TEMP.bin</file_name></partition>\n\t</p>")

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_xz_chunks_match_stock_temp(self):
        with tempfile.TemporaryDirectory() as tmp:
            paths.fwkit("unpack", "--no-raw", paths.firmware(), tmp)
            stored = (Path(tmp) / "parts/TEMP.bin.part").read_bytes()
        raw, pos = bytearray(), 0
        while pos < len(stored):
            _, packed, _ = struct.unpack_from("<III", stored, pos + 4)
            raw += lzma.decompress(stored[pos + 16:pos + 16 + packed])
            pos += 16 + packed
        self.assertEqual(firmware.xz_chunks(bytes(raw)), stored)
        self.assertEqual(firmware.fix_aota_crc(bytes(raw)), bytes(raw))  # stock TEMP's CRCs are right
        stale = bytearray(raw)
        stale[0x1000] ^= 1
        self.assertNotEqual(firmware.fix_aota_crc(bytes(stale))[0x18:0x1C], raw[0x18:0x1C])
        with self.assertRaisesRegex(ValueError, "AOTA"):
            firmware.fix_aota_crc(bytes(0x1000))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_motor_only_patch(self):
        self.assertEqual(STOCK[0x259F00:0x25A000], bytes(0x100))
        out = firmware.patch(STOCK, motor=350, reserve=False)
        self.assertEqual(len(out), len(STOCK))
        firmware.check_acth(out)
        self.assertEqual(out[0xFB398:0xFB39C], firmware.thumb_bl(0x100FB398, 0x10259F00))
        self.assertEqual(out[0x259F00:0x259F0E], firmware.motor_hook(350))
        self.assertEqual(struct.unpack_from("<I", out, 0x18)[0], 0x259F10 - 0x200)
        self.assertEqual([(a, b) for a, b in firmware.changed_ranges(STOCK, out)],
                         [(0x18, 0x24), (0xFB398, 0xFB39C), (0x259F00, 0x259F0E)])
        with_stub = firmware.patch(STOCK, fake_stub(), motor=350)
        self.assertEqual(with_stub[0x259F00:0x259F0E], firmware.motor_hook(350))
        with self.assertRaisesRegex(ValueError, "stub"):
            firmware.patch(STOCK, fake_stub(), motor=350, reserve=False)

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_weak_row_patch(self):
        out = firmware.patch(STOCK, fake_stub(), weak=350)
        u32 = lambda a: struct.unpack_from("<I", out, a - 0x10000000)[0]
        self.assertEqual(struct.unpack_from("<4I", out, 0x259E80), (255, 250, 0, 89))
        self.assertEqual(out[0x259E90:0x259E90 + 14], b"%s\n%s\nWeak\n%s\0")
        # the roller shows Weak at index 2, Off at 3: hooks swap 2 <-> 3 between index and level
        hooks = {firmware.STOCK_PREVIEW: 0x10259EA0, firmware.STOCK_SET_LEVEL: 0x10259EB4,
                 firmware.STOCK_GET_LEVEL: 0x10259EC8}
        calls = [(a, k, firmware.STOCK_PREVIEW) for a, k in firmware.PREVIEW_CALLS]
        calls += [(a, "bl", firmware.STOCK_SET_LEVEL) for a in firmware.SET_LEVEL_CALLS]
        calls += [(a, "bl", firmware.STOCK_GET_LEVEL) for a in firmware.GET_LEVEL_CALLS]
        for at, kind, stock in calls:
            enc = firmware.thumb_bl if kind == "bl" else firmware.thumb_b_w
            self.assertEqual(out[at - 0x10000000:at - 0x10000000 + 4], enc(at, hooks[stock]), hex(at))
        self.assertEqual(out[0x259EA0:0x259EAE], firmware.SWAP_2_3)
        self.assertEqual(out[0x259EAE:0x259EB2], firmware.thumb_b_w(0x10259EAE, firmware.STOCK_PREVIEW))
        self.assertEqual(out[0x259EC8:0x259ECA] + out[0x259EDC:0x259EDE], b"\x08\xb5\x08\xbd")
        # setlist_view's subtitle: level 3 -> {"Weak", 0} at 0x10259EF0, others stay view + 0x1AC + level * 8
        self.assertEqual(out[0xB54B2:0xB54B6], firmware.thumb_bl(firmware.SETLIST_SUBTITLE, 0x10259EE0))
        self.assertEqual(out[0xB54B8:0xB54BC], firmware.NOP_W)
        self.assertEqual(out[0x259EF0:0x259EFD], struct.pack("<II", 0x10259EF8, 0) + b"Weak\0")
        self.assertEqual([u32(a) for a in firmware.LEVELS_LITERALS], [0x10259E80] * 2)
        self.assertEqual([u32(a) for a in firmware.ROLLER_FORMAT_LITERALS], [0x10259E90] * 2)
        self.assertEqual([out[a - 0x10000000:a - 0x10000000 + 2] for a in firmware.LEVELS_BOUNDS], [b"\x03\x2b"] * 2)
        self.assertEqual(out[0xBB892:0xBB894], b"\x02\x29")
        base = firmware.patch(STOCK, fake_stub())
        self.assertEqual({i & ~3 for i in range(0x200, 0x259E80) if out[i] != base[i]},
                         {0xE17CC, 0xE17F0, 0xE17F8, 0xE1810, 0xBBADC, 0xBBBD0, 0xBB890}
                         | {(at - 0x10000000 + i) & ~3 for at, _, _ in calls for i in range(4)}
                         | {0xB54B0, 0xB54B4, 0xB54B8})
        plain = firmware.patch(STOCK, weak=350, reserve=False)
        firmware.check_acth(plain)
        self.assertGreaterEqual(struct.unpack_from("<I", plain, 0x18)[0] + 0x200, 0x259EFD)
        with self.assertRaises(ValueError):
            firmware.patch(STOCK, weak=1000, reserve=False)

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_applist_needs_thumb(self):
        with self.assertRaisesRegex(ValueError, "stub"):
            firmware.patch(STOCK, fake_stub(applist=0x1025A120))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_stub_too_big_rejected(self):
        with self.assertRaises(ValueError):
            firmware.patch(STOCK, fake_stub() + bytes(firmware.STUB_MAX))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_stub_header_checked(self):
        with self.assertRaisesRegex(ValueError, "stub"):
            firmware.patch(STOCK, b"XXXX" + fake_stub()[4:])
        with self.assertRaisesRegex(ValueError, "stub"):
            firmware.patch(STOCK, fake_stub(boot=0x1025A0A0))
        for bad in (dict(ble=0x1025A0C0), dict(view_proc=0x10000001), dict(view_name=0x1025A080 + len(fake_stub())),
                    dict(applist=0x1025A081 + 0x4000)):
            with self.subTest(**bad), self.assertRaisesRegex(ValueError, "stub"):
                firmware.patch(STOCK, fake_stub(**bad))

    def test_proc_types_patched(self):
        out = synthetic_app()
        firmware.add_stub(out, fake_stub(types=type_table()))
        for lit in firmware.PROC_TYPES_LITERALS:
            self.assertEqual(struct.unpack_from("<I", out, lit - 0x10000000)[0], TYPES_AT)
        older = synthetic_app()
        firmware.add_stub(older, fake_stub(guard=(SKIP,) * 5))
        for lit in firmware.PROC_TYPES_LITERALS:
            self.assertEqual(struct.unpack_from("<I", older, lit - 0x10000000)[0], firmware.PROC_TYPES)

    def test_proc_list_hook_patched(self):
        at = PROC_LIST_CALL - 0x10000000
        out = synthetic_app()
        firmware.add_stub(out, fake_stub(types=type_table(), proc_list=0x1025A0C1))
        self.assertEqual(out[at:at + 4], firmware.thumb_bl(PROC_LIST_CALL, 0x1025A0C0))
        plain = synthetic_app()
        firmware.add_stub(plain, fake_stub(types=type_table()))
        self.assertEqual(plain[at:at + 4], firmware.thumb_bl(PROC_LIST_CALL, STOCK_IMAGE_LIST))
        with self.assertRaisesRegex(ValueError, "stub"):
            firmware.add_stub(synthetic_app(), fake_stub(types=type_table(), proc_list=0x1025A0C0))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_proc_list_call_is_stock(self):
        at = PROC_LIST_CALL - 0x10000000
        self.assertEqual(STOCK[at:at + 4], firmware.thumb_bl(PROC_LIST_CALL, STOCK_IMAGE_LIST))

    def test_image_hooks_patched(self):
        out = synthetic_app()
        firmware.add_stub(out, fake_stub(types=type_table(), image_cf=0x1025A0C1, image_unpack=0x1025A0E1))
        for call, hook in (*((c, 0x1025A0C0) for c in IMAGE_CF_CALLS), (IMAGE_UNPACK_CALL, 0x1025A0E0)):
            at = call - 0x10000000
            self.assertEqual(out[at:at + 4], firmware.thumb_bl(call, hook))
        for bad in (dict(image_cf=0x1025A0C0, image_unpack=0x1025A0E1), dict(image_cf=0x1025A0C1, image_unpack=0x1025A0E0)):
            with self.subTest(**bad), self.assertRaisesRegex(ValueError, "stub"):
                firmware.add_stub(synthetic_app(), fake_stub(types=type_table(), **bad))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_image_calls_are_stock(self):
        for call, target in (*((c, STOCK_IMAGE_CF) for c in IMAGE_CF_CALLS), (IMAGE_UNPACK_CALL, STOCK_IMAGE_UNPACK)):
            at = call - 0x10000000
            self.assertEqual(STOCK[at:at + 4], firmware.thumb_bl(call, target))

    def test_source_hook_patched(self):
        at = SOURCE_UNKNOWN - 0x10000000
        out = synthetic_app()
        firmware.add_stub(out, fake_stub(types=type_table(), source=0x1025A0C1))
        self.assertEqual(out[at:at + 4], firmware.thumb_b_w(SOURCE_UNKNOWN, 0x1025A0C0))
        with self.assertRaisesRegex(ValueError, "stub"):
            firmware.add_stub(synthetic_app(), fake_stub(types=type_table(), source=0x1025A0C0))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_source_branch_is_stock(self):
        at = SOURCE_UNKNOWN - 0x10000000
        self.assertEqual(STOCK[at:at + 4], MVN_R0_15)

    def test_proc_types_checked(self):
        own = lambda *fields: struct.pack("<6I", *fields)
        for name, table in (("stock types changed", type_table(stock=b"\1" + bytes(0x5F))),
                            ("no 0xff end", type_table(sentinel=(0xFFFF, SKIP, SKIP, SKIP, SKIP, 0))),
                            ("0xff matches and calls 0", type_table(sentinel=(0xFF, 0, 0, 0, 0, 0))),
                            ("handler outside the stub", type_table(added=own(6, 0x100E2639, SKIP, SKIP, SKIP, 0))),
                            ("handler without Thumb", type_table(added=own(6, SKIP & ~1, SKIP, SKIP, SKIP, 0)))):
            with self.subTest(name), self.assertRaisesRegex(ValueError, "type table"):
                firmware.add_stub(synthetic_app(), fake_stub(types=table))

    @skipUnless(HAVE_STOCK and BUILT_STUB.is_file(), "needs JX402_V1.0.0.73.bin and scripts/build-fw")
    def test_unknown_proc_type_is_skipped(self):
        stock = firmware.PROC_TYPES
        # the stock ends its table with 0xFFFF but stops at 0xFF: type 4 would match a property row 55 entries past it
        self.assertEqual(lookup(STOCK, stock, 4), 0x101F4294)
        out = firmware.patch(STOCK, BUILT_STUB.read_bytes())
        table = struct.unpack_from("<I", out, firmware.PROC_TYPES_LITERALS[0] - 0x10000000)[0]
        self.assertNotEqual(table, stock)
        for kind in range(0x10000):
            at = lookup(out, table, kind, limit=16)
            if kind < 4:
                self.assertEqual(out[at - 0x10000000:at - 0x10000000 + 0x18],
                                 STOCK[stock - 0x10000000 + 0x18 * kind:stock - 0x10000000 + 0x18 * (kind + 1)])
            elif kind in (4, 5, 6, 7, 0xFF):  # the stub's types; the end, matched by a face of that type
                self.assertTrue(0x1025A080 <= at < 0x1025A080 + len(BUILT_STUB.read_bytes()))
                self.assertEqual(struct.unpack_from("<H", out, at - 0x10000000)[0], kind)
            else:
                self.assertIsNone(at)

    def test_wf_guard_calls_patched(self):
        hooks = (0x1025A0A1, 0x1025A0C1, 0x1025A0E1, 0x1025A0F1, 0x1025A111)
        out = synthetic_app()
        firmware.add_stub(out, fake_stub(guard=hooks))
        v1 = synthetic_app()
        firmware.add_stub(v1, fake_stub())
        for at, stock, hook in firmware.GUARD_WP2:
            o = at - 0x10000000
            self.assertEqual(out[o:o + 4], firmware.thumb_bl(at, hooks[hook] & ~1), hex(at))
            self.assertEqual(v1[o:o + 4], firmware.thumb_bl(at, stock), hex(at))
        for i in range(5):
            bad = list(hooks)
            bad[i] &= ~1
            with self.assertRaisesRegex(ValueError, "stub"):
                firmware.add_stub(synthetic_app(), fake_stub(guard=tuple(bad)))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_wf_guard_calls_are_stock(self):
        for at, stock, _ in firmware.GUARD_WP2:
            self.assertEqual(STOCK[at - 0x10000000:at - 0x10000000 + 4], firmware.thumb_bl(at, stock), hex(at))

    def test_wp3_guard_calls_patched(self):
        hooks = [0x102BD29D, 0x102BD2A1, 0x102BD2A9, 0x102BD2B1, 0x102BD2B9]
        app = bytearray(firmware.WP3_STUB_OFFSET)
        for at, stock, _ in firmware.GUARD_WP3:
            app[at - 0x10000000:at - 0x10000000 + 4] = firmware.thumb_bl(at, stock)
        out = bytearray(app)
        firmware.add_stub_wp3(out, wp3_stub(hooks))
        changed = set()
        for at, _, hook in firmware.GUARD_WP3:
            o = at - 0x10000000
            self.assertEqual(out[o:o + 4], firmware.thumb_bl(at, hooks[hook] & ~1), hex(at))
            changed.update(range(o, o + 4))
        self.assertLessEqual({i for i in range(len(app)) if out[i] != app[i]}, changed)
        for bad in (b"WP2S" + wp3_stub(hooks)[4:], wp3_stub([hooks[0] & ~1, *hooks[1:]]),
                    wp3_stub([*hooks[:4], 0x1025A0C1])):
            with self.assertRaisesRegex(ValueError, "stub"):
                firmware.add_stub_wp3(bytearray(app), bad)

    @skipUnless(STOCK_WP3 is not None, "needs JX402_w3pro_V1.0.1.77.bin")
    def test_wp3_stock_patch(self):
        self.assertEqual(len(STOCK_WP3), firmware.WP3_STUB_OFFSET)
        for at, stock, _ in firmware.GUARD_WP3:
            self.assertEqual(STOCK_WP3[at - 0x10000000:at - 0x10000000 + 4], firmware.thumb_bl(at, stock), hex(at))
        out = firmware.patch_wp3(STOCK_WP3, wp3_stub([0x102BD29D] * 5))
        firmware.check_acth(out)
        with self.assertRaisesRegex(ValueError, "sha256"):
            firmware.patch_wp3(bytes(firmware.WP3_STUB_OFFSET), b"WP3S")

    @skipUnless(STOCK_WP3 is not None, "needs JX402_w3pro_V1.0.1.77.bin")
    def test_wp3_ota_image_round_trip(self):
        app = firmware.patch_wp3(STOCK_WP3, wp3_stub([0x102BD29D] * 5))
        with tempfile.TemporaryDirectory() as tmp:
            image = cli.ota_image(paths.firmware(paths.FIRMWARE_WP3), app, Path(tmp))
            self.assertEqual(paths.app_from(image), app)
            paths.fwkit("verify", image)

    def test_widget_slot_patched(self):
        slot = firmware.WGT_REG + 4 * firmware.WGT_ID - 0x10000000
        out = synthetic_app()
        firmware.add_stub(out, fake_stub(widget=WIDGET_AT, entry=widget_entry()))
        self.assertEqual(struct.unpack_from("<I", out, slot)[0], WIDGET_AT)
        without = synthetic_app()
        firmware.add_stub(without, fake_stub())
        self.assertEqual(without[slot:slot + 4], bytes(4))
        self.assertEqual({i for i in range(firmware.STUB_OFFSET) if out[i] != without[i]}, set(range(slot, slot + 4)))

    def test_widget_slot_must_be_free(self):
        out = synthetic_app()
        struct.pack_into("<I", out, firmware.WGT_REG + 4 * firmware.WGT_ID - 0x10000000, 0x101F0784)
        with self.assertRaises(ValueError):
            firmware.add_stub(out, fake_stub(widget=WIDGET_AT, entry=widget_entry()))

    def test_widget_entry_checked(self):
        for name, stub in (("misaligned", fake_stub(widget=WIDGET_AT + 2, entry=widget_entry())),
                           ("outside", fake_stub(widget=0x1025A080 + 0x120, entry=widget_entry())),
                           ("past the end", fake_stub(widget=0x1025A080 + 0x120 - firmware.WGT_ENTRY.size + 4)),
                           ("before the stub", fake_stub(widget=0x1025A07C)),
                           ("callback at the end", fake_stub(widget=WIDGET_AT, entry=widget_entry(
                               callbacks=CALLBACKS[:4] + (0x1025A080 + 0x120 + 1,)))),
                           ("other id", fake_stub(widget=WIDGET_AT, entry=widget_entry(wid=0x41))),
                           ("arm callback", fake_stub(widget=WIDGET_AT, entry=widget_entry(callbacks=(0x1025A0A0,) * 5))),
                           ("callback out of the stub", fake_stub(widget=WIDGET_AT,
                                                                  entry=widget_entry(callbacks=(0x101B1A05,) * 5)))):
            with self.subTest(name), self.assertRaisesRegex(ValueError, "widget"):
                firmware.add_stub(synthetic_app(), stub)

    def test_heap_shrinks_by_stub_ram_only(self):
        self.assertEqual(firmware.HEAP_OLD - firmware.HEAP_NEW, 0x1000)
        self.assertEqual(firmware.STUB_RAM, 0x3871EEC0)
        self.assertEqual(firmware.STUB_RAM + firmware.STUB_RAM_SIZE, 0x380A9340 + 0x676B80)

    def test_stage_nor(self):
        nor = firmware.stage_nor(b"\0" * 0x400000, b"\1" * 16)
        self.assertEqual(nor[0x24000:0x24010], b"\1" * 16)
        self.assertEqual(nor[0x24010:0x24000 + 0x34B000], b"\xff" * (0x34B000 - 16))
        self.assertEqual(len(nor), 0x400000)

    def test_stage_nor_too_big(self):
        with self.assertRaises(ValueError):
            firmware.stage_nor(b"\0" * 0x400000, b"\1" * 0x34B001)


if __name__ == "__main__":
    unittest.main()
