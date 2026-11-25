import contextlib
import io
import json
import struct
from pathlib import Path
import tempfile
import unittest
import zlib

from wp2apps import cli, package


class PackageTest(unittest.TestCase):
    def test_round_trip(self):
        image = bytes(range(256)) * 4
        pkg = package.parse(package.pack(image, name="Vibration Lab", version=3, relocs=[0, 8]))
        self.assertEqual((pkg.name, pkg.version, pkg.kind, pkg.abi, pkg.entry, pkg.image, pkg.relocs),
                         ("Vibration Lab", 3, package.KIND_APP, 1, 0, image, (0, 8)))

    def test_widget_icon_trailer(self):
        image, icons = bytes(64), [bytes([i]) * package.WIDGET_ICON_BYTES for i in range(3)]
        plain = package.pack(image, name="w", version=1, relocs=[4])
        data = package.pack(image, name="w", version=1, relocs=[4], widget_icons=icons)
        self.assertEqual(data[:len(plain)], plain, "body_crc and header do not cover the trailer")
        self.assertEqual(data[len(plain):len(plain) + 8], b"WP2W\x01\x03\x28\x00")
        self.assertEqual(struct.unpack_from("<I", data, len(plain) + 8)[0], zlib.crc32(b"".join(icons)))
        self.assertEqual(len(data), len(plain) + 12 + 3 * 4800)
        self.assertEqual(package.parse(data).widget_icons, tuple(icons))
        self.assertEqual(package.parse(plain).widget_icons, ())
        self.assertEqual(package.strip_trailer(data), plain)
        at = len(plain)
        for off, value in ((0, 0x58), (4, 2), (5, 0), (5, 2), (6, 48), (7, 1), (8, data[at + 8] ^ 1), (12, 9)):
            bad = bytearray(data)
            bad[at + off] = value  # magic, version, count, size, zero, crc, a pixel
            with self.assertRaises(ValueError, msg=f"byte {off}"):
                package.parse(bytes(bad))
        with self.assertRaises(ValueError):
            package.parse(data[:-1])
        with self.assertRaises(ValueError):
            package.pack(image, name="w", version=1, widget_icons=[bytes(4800)] * 17)
        with self.assertRaises(ValueError):
            package.pack(image, name="w", version=1, widget_icons=[bytes(4799)])
        with self.assertRaises(ValueError):
            package.pack(image, name="l", version=1, kind=package.KIND_LOADER, widget_icons=[bytes(4800)])

    def test_set_order(self):
        image = b"\x01" * 8 + package.ORDER_MAGIC + bytes(24 * package.ORDER_MAX) + b"\x02" * 8
        loader = package.pack(image, name="loader", version=3, kind=package.KIND_LOADER, relocs=(0,))
        out = package.parse(package.set_order(loader, ["hello", "api-demo"]))
        self.assertEqual((out.name, out.version, out.relocs), ("loader", 3, (0,)))
        table = out.image[16:16 + 24 * package.ORDER_MAX]
        self.assertEqual(table[:48], b"hello".ljust(24, b"\0") + b"api-demo".ljust(24, b"\0"))
        self.assertFalse(any(table[48:]))
        self.assertEqual(out.image[:16] + out.image[-8:], image[:16] + image[-8:])
        self.assertEqual(package.parse(package.set_order(loader, [])).image, image)
        with self.assertRaises(ValueError):
            package.set_order(loader, ["x"] * 17)
        with self.assertRaises(ValueError):
            package.set_order(package.pack(image, name="a", version=1), ["x"])
        with self.assertRaises(ValueError):  # a relocated word inside the table
            package.set_order(package.pack(image, name="l", version=1, kind=package.KIND_LOADER, relocs=(20,)), [])

    def test_features(self):
        from wp2apps import icon
        self.assertEqual(package.features(b"WP2F\x03\0\0\0" + bytes(8)), ["widget", "page"])
        self.assertEqual(package.features(bytes(16)), [])
        with_icon = icon.encode(96, 96, bytes(96 * 96 * 4))
        self.assertEqual(package.features(with_icon + b"WP2F\x02\0\0\0"), ["page"])
        self.assertEqual(package.features(with_icon), [])

    def test_size_limits(self):
        self.assertEqual((package.APP_SIZE, package.LOADER_SIZE), (0x20000, 0x10000))
        package.pack(b"\0" * 16, name="loader", version=1, kind=package.KIND_LOADER)
        for kind, size in ((package.KIND_APP, 0x20001), (package.KIND_LOADER, 0x10001)):
            with self.assertRaises(ValueError):
                package.pack(b"\0" * size, name="x", version=1, kind=kind)

    def test_relocs_checked(self):
        for relocs in ([8, 0], [0, 0], [13]):
            with self.assertRaises(ValueError):
                package.pack(b"\0" * 16, name="x", version=1, relocs=relocs)

    def test_reloc_table_covered_by_crc(self):
        data = bytearray(package.pack(b"\0" * 16, name="x", version=1, relocs=[4]))
        data[-1] ^= 1
        with self.assertRaises(ValueError):
            package.parse(bytes(data))

    def test_relocations_from_elf(self):
        elf = fake_elf(1, [(0, 0x30)], rels=[(0x10, R_ARM_ABS32, TEXT), (0x04, R_ARM_THM_CALL, TEXT),
                                             (0x20, R_ARM_ABS32, TEXT)])
        self.assertEqual(package.relocations(elf), [0x10, 0x20])

    def test_relocations_reject_movw_and_absolute_symbols(self):
        with self.assertRaisesRegex(ValueError, "no-movt"):
            package.relocations(fake_elf(1, [(0, 0x30)], rels=[(0x10, 47, TEXT)]))
        with self.assertRaisesRegex(ValueError, "absolute"):
            package.relocations(fake_elf(1, [(0, 0x30)], rels=[(0x10, R_ARM_ABS32, SHN_ABS)]))

    def test_utf8_name_fits(self):
        self.assertEqual(package.parse(package.pack(b"\0" * 4, name="Вибро", version=1)).name, "Вибро")

    def test_bad_names_rejected(self):
        for name in ("Я" * 12, ""):
            with self.assertRaises(ValueError):
                package.pack(b"\0" * 4, name=name, version=1)

    def test_corrupt_rejected(self):
        data = package.pack(b"\1" * 64, name="x", version=1)
        flipped = data[:-1] + bytes([data[-1] ^ 1])
        for broken in (b"XXXX" + data[4:], data[:-1], flipped, data[:package.HEADER_SIZE - 1]):
            with self.assertRaises(ValueError):
                package.parse(broken)

    def test_header_bit_flip_rejected(self):
        data = bytearray(package.pack(b"\0" * 64, name="x", version=1))
        data[0x28] ^= 0x01
        with self.assertRaises(ValueError):
            package.parse(bytes(data))

    def test_nonzero_reserved_rejected(self):
        data = bytearray(package.pack(b"\0" * 64, name="x", version=1))
        data[0x3F] = 1
        with self.assertRaises(ValueError):
            package.parse(bytes(data))

    def test_abi_minor_round_trip(self):
        pkg = package.parse(package.pack(b"\0" * 4, name="x", version=1))
        self.assertEqual(pkg.abi_minor, package.ABI_MINOR)
        self.assertEqual(package.HEADER.unpack_from(package.pack(b"\0" * 4, name="x", version=1))[3],
                         package.ABI_MINOR)

    def test_elf_linked_at_zero(self):
        package.check_elf(b"\0" * 0x30, fake_elf(1, [(0, 0x30)]))
        with self.assertRaises(ValueError):
            package.check_elf(b"\0" * 0x30, fake_elf(0x386FF001, [(0x386FF000, 0x30)]))
        with self.assertRaises(ValueError):
            package.check_elf(b"\0" * 0x30, fake_elf(1, [(0, 0x30), (0x30, 0x8)]))

    def test_entry_size_and_address_bounds(self):
        with self.assertRaises(ValueError):
            package.pack(b"\0" * 16, name="x", version=1, entry=16)
        with self.assertRaises(ValueError):
            package.pack(b"\0" * (package.APP_SIZE + 1), name="x", version=1)

    def test_cli_pack_info(self):
        with tempfile.TemporaryDirectory() as d:
            image, out = Path(d) / "hello.bin", Path(d) / "hello.wp2app"
            elf = Path(d) / "hello.elf"
            image.write_bytes(b"\x70\x47" * 20)
            elf.write_bytes(fake_elf(1, [(0, 40)]))
            self.assertEqual(cli.main(["pack", str(image), "--elf", str(elf), "--name", "hello", "--version", "2",
                                       "--output", str(out)]), 0)
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                self.assertEqual(cli.main(["info", str(out)]), 0)
            info = json.loads(stdout.getvalue())
            self.assertEqual((info["name"], info["version"], info["image_size"]), ("hello", 2, 40))
            self.assertEqual(info["relocs"], 0)

    def test_cli_error_exit(self):
        with tempfile.TemporaryDirectory() as d:
            bad = Path(d) / "bad.wp2app"
            bad.write_bytes(b"nope")
            with contextlib.redirect_stderr(io.StringIO()) as stderr:
                self.assertEqual(cli.main(["info", str(bad)]), 1)
            self.assertIn("Error:", stderr.getvalue())


TEXT = 1
SHN_ABS = 0xFFF1
R_ARM_ABS32 = 2
R_ARM_THM_CALL = 10


def fake_elf(entry, sections, rels=()):
    shdrs = [bytes(40)]
    for addr, size in sections:
        shdrs.append(struct.pack("<IIIIIIIIII", 1, 1, 2, addr, 0, size, 0, 0, 0, 0))
    symtab = bytes(16) + b"".join(struct.pack("<IIIBBH", 0, 0, 0, 0, 0, shndx) for _, _, shndx in rels)
    relsec = b"".join(struct.pack("<II", off, ((i + 1) << 8) | typ) for i, (off, typ, _) in enumerate(rels))
    nsec = len(shdrs) + (2 if rels else 0)
    data_off = 0x34 + 40 * nsec
    if rels:
        shdrs.append(struct.pack("<IIIIIIIIII", 1, 2, 0, 0, data_off, len(symtab), 0, 0, 0, 16))
        shdrs.append(struct.pack("<IIIIIIIIII", 1, 9, 0, 0, data_off + len(symtab), len(relsec),
                                 len(shdrs) - 1, TEXT, 0, 8))
    header = bytearray(0x34)
    header[:4] = b"\x7fELF"
    struct.pack_into("<I", header, 0x18, entry)
    struct.pack_into("<I", header, 0x20, 0x34)
    struct.pack_into("<HH", header, 0x2E, 40, nsec)
    return bytes(header) + b"".join(shdrs) + symtab + relsec


def symbol_elf(name: bytes, value: int) -> bytes:
    strtab = b"\0" + name + b"\0"
    symtab = bytes(16) + struct.pack("<IIIBBH", 1, value, 2, 0x11, 0, 1)
    off = 0x34 + 40 * 3
    shdrs = [bytes(40), struct.pack("<IIIIIIIIII", 0, 2, 0, 0, off, len(symtab), 2, 0, 0, 16),
             struct.pack("<IIIIIIIIII", 0, 3, 0, 0, off + len(symtab), len(strtab), 0, 0, 0, 0)]
    header = bytearray(0x34)
    header[:4] = b"\x7fELF"
    struct.pack_into("<I", header, 0x20, 0x34)
    struct.pack_into("<HH", header, 0x2E, 40, 3)
    return bytes(header) + b"".join(shdrs) + symtab + strtab


class AbiMinorTest(unittest.TestCase):
    def test_read_from_symbol(self):
        image = bytes(8) + struct.pack("<H", 1) + bytes(6)
        self.assertEqual(package.abi_minor(image, symbol_elf(b"WP2_ABI_MINOR", 8)), 1)
        self.assertEqual(package.abi_minor(image, symbol_elf(b"OTHER", 8)), 0)
        with self.assertRaisesRegex(ValueError, "outside"):
            package.abi_minor(image, symbol_elf(b"WP2_ABI_MINOR", 15))

    def test_header_carries_it(self):
        pkg = package.parse(package.pack(bytes(16), name="x", version=1, abi_minor=1))
        self.assertEqual(pkg.abi_minor, 1)
        with self.assertRaisesRegex(ValueError, "abi_minor"):
            package.pack(bytes(16), name="x", version=1, abi_minor=256)


if __name__ == "__main__":
    unittest.main()


class ModelByteTest(unittest.TestCase):
    def test_loader_model_byte(self):
        wp3 = package.pack(bytes(64), name="loader", version=1, kind=package.KIND_LOADER, model="wp3")
        self.assertEqual(wp3[package.MODEL_OFFSET], 2)
        self.assertEqual(package.parse(wp3).model, "wp3")
        wp2 = package.pack(bytes(64), name="loader", version=1, kind=package.KIND_LOADER)
        self.assertEqual(wp2[package.MODEL_OFFSET:package.HEADER_SIZE], bytes(8))
        self.assertEqual(package.parse(wp2).model, "wp2")
        # the byte is outside the header CRC span, so the WP2 stub's reserved-byte check is what refuses it
        self.assertGreaterEqual(package.MODEL_OFFSET, package.HEADER_CRC_SPAN + 4)

    def test_apps_carry_no_model(self):
        with self.assertRaisesRegex(ValueError, "only a loader"):
            package.pack(bytes(64), name="app", version=1, model="wp3")
        bad = bytearray(package.pack(bytes(64), name="app", version=1))
        bad[package.MODEL_OFFSET] = 2
        with self.assertRaisesRegex(ValueError, "reserved"):
            package.parse(bytes(bad))
        bad = bytearray(package.pack(bytes(64), name="loader", version=1, kind=package.KIND_LOADER))
        bad[package.MODEL_OFFSET] = 9
        with self.assertRaisesRegex(ValueError, "reserved"):
            package.parse(bytes(bad))
