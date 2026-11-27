import struct
import unittest
import zlib

from wp2apps import icon, package


def png(w, h, rgba, filt=0):
    raw = b"".join(bytes([filt]) + rgba[y * w * 4:(y + 1) * w * 4] for y in range(h))
    chunk = lambda kind, body: struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


S = icon.SIDE


def good() -> bytes:
    rgba = bytes([255, 255, 255, 255]) * (S * S - 1) + bytes([0, 0, 255, 0])
    return icon.encode(*icon.read_png(png(S, S, rgba)))


class IconTest(unittest.TestCase):
    def test_encode_white_and_transparent(self):
        desc = good()
        self.assertEqual(desc[:16], struct.pack("<4sBBHHHI", b"WP2I", 1, 5, S, S, 0, S * S * 3))
        self.assertEqual(desc[16:19], b"\xff\xff\xff")
        self.assertEqual(desc[-3:], b"\0\0\0")  # alpha 0: no colour
        self.assertEqual(icon.parse(desc + b"code")[2], desc[16:])

    def test_png_filters(self):
        rgba = bytes(range(256)) * (S * S * 4 // 256)
        # Up filter on all-zero deltas after row 0 keeps each column constant
        w, h, out = icon.read_png(png(4, 2, bytes(range(16)) + bytes(16), filt=2))
        self.assertEqual(out, bytes(range(16)) * 2)
        self.assertEqual(icon.read_png(png(S, S, rgba))[2], rgba)

    def test_dither_keeps_mean(self):
        grey = bytes([100, 100, 100, 255]) * (S * S)
        px = icon.encode(S, S, grey)[16:]
        reds = [struct.unpack_from("<H", px, i)[0] >> 11 for i in range(0, len(px), 3)]
        self.assertAlmostEqual(sum(reds) / len(reds) * 255 / 31, 100, delta=3)
        self.assertGreater(len(set(reds)), 1)

    def test_rejects(self):
        desc = good()
        self.assertIsNone(icon.parse(b"\x80\xb5" + desc[2:]))  # bad magic: an image without icon
        with self.assertRaisesRegex(ValueError, "truncated"):
            icon.parse(desc[:10])
        with self.assertRaisesRegex(ValueError, "past the image"):
            icon.parse(desc[:-1])
        big = struct.pack("<4sBBHHHI", b"WP2I", 1, 5, 128, 128, 0, 128 * 128 * 3)
        with self.assertRaisesRegex(ValueError, "96x96"):
            icon.parse(big + bytes(128 * 128 * 3))
        with self.assertRaisesRegex(ValueError, "unsupported"):
            icon.parse(desc[:4] + b"\x02" + desc[5:])
        with self.assertRaisesRegex(ValueError, "96x96"):
            icon.encode(8, 8, bytes(8 * 8 * 4))
        with self.assertRaisesRegex(ValueError, "8-bit"):
            icon.read_png(png(S, S, b"")[:24] + b"\x10\x06" + png(S, S, b"")[26:])

    def test_package_validates_icon(self):
        image = good() + b"\x70\x47"
        pkg = package.parse(package.pack(image, name="x", version=1, entry=len(image) - 2))
        self.assertIsNotNone(icon.parse(pkg.image))
        with self.assertRaisesRegex(ValueError, "past the image"):
            package.pack(image[:100], name="x", version=1)


if __name__ == "__main__":
    unittest.main()
