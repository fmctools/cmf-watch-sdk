import struct
import tempfile
import unittest
import zlib
from pathlib import Path

from wp2apps import bitmaps


def png(w, h, color, samples, palette=None, trns=None):
    """A minimal 8-bit non-interlaced PNG: samples are already in the file's channel layout."""
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[color]
    stride = w * channels
    raw = b"".join(b"\0" + bytes(samples[y * stride:(y + 1) * stride]) for y in range(h))
    chunk = lambda kind, body: struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))
    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, color, 0, 0, 0))
    if palette is not None:
        data += chunk(b"PLTE", bytes(palette))
    if trns is not None:
        data += chunk(b"tRNS", bytes(trns))
    return data + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


class BitmapsTest(unittest.TestCase):
    def test_read_rgba_color_types(self):
        w, h = 2, 2
        rgba = bytes([255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 128])
        self.assertEqual(bitmaps.read_rgba(png(w, h, 6, rgba)), (2, 2, rgba))

        rgb = bytes([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12])
        expect = bytes([1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255])
        self.assertEqual(bitmaps.read_rgba(png(w, h, 2, rgb)), (2, 2, expect))

        got = bitmaps.read_rgba(png(w, h, 0, bytes([0, 9, 9, 0])))[2]
        self.assertEqual(got, bytes([0, 0, 0, 255, 9, 9, 9, 255, 9, 9, 9, 255, 0, 0, 0, 255]))

        pal = png(w, h, 3, bytes([0, 1, 1, 0]), palette=[255, 0, 0, 0, 255, 0], trns=[0, 64])
        self.assertEqual(bitmaps.read_rgba(pal)[2],
                         bytes([255, 0, 0, 0, 0, 255, 0, 64, 0, 255, 0, 64, 255, 0, 0, 0]))

        ga = png(w, h, 4, bytes([7, 255, 8, 0, 9, 128, 10, 255]))
        self.assertEqual(bitmaps.read_rgba(ga)[2],
                         bytes([7, 7, 7, 255, 8, 8, 8, 0, 9, 9, 9, 128, 10, 10, 10, 255]))

    def test_rgb565_and_alpha(self):
        self.assertEqual(bitmaps.rgb565(1, 1, bytes([255, 255, 255, 255])), b"\xff\xff")
        self.assertEqual(bitmaps.rgb565(1, 1, bytes([255, 0, 0, 255])), struct.pack("<H", 0xF800))
        self.assertEqual(bitmaps.rgb565(1, 1, bytes([255, 0, 0, 0])), b"\0\0", "alpha flattens onto black")

    def test_ident(self):
        self.assertEqual(bitmaps.ident("logo"), "LOGO")
        self.assertEqual(bitmaps.ident("my-icon-2"), "MY_ICON_2")
        self.assertEqual(bitmaps.ident("2fast"), "_2FAST")

    def test_generate(self):
        with tempfile.TemporaryDirectory() as tmp:
            crate = Path(tmp)
            (crate / "src").mkdir()
            (crate / "bitmaps").mkdir()
            (crate / "bitmaps" / "dot.png").write_bytes(png(1, 1, 6, bytes([255, 0, 0, 255])))
            out, n = bitmaps.generate(crate)
            self.assertEqual((out.name, n), ("bitmaps.gen.rs", 1))
            text = out.read_text(encoding="utf-8")
            self.assertIn("pub mod bitmaps", text)
            self.assertIn(
                "    pub static DOT: Bitmap = Bitmap { w: 1, h: 1, px: &[\n"
                "                63488\n"
                "    ] };",
                text,
            )
            # unchanged content keeps the file's mtime (cargo skips the rebuild)
            before = out.stat().st_mtime_ns
            bitmaps.generate(crate)
            self.assertEqual(out.stat().st_mtime_ns, before)

    def test_generate_without_a_directory(self):
        with tempfile.TemporaryDirectory() as tmp:
            crate = Path(tmp)
            (crate / "src").mkdir()
            out, n = bitmaps.generate(crate)
            self.assertEqual(n, 0)
            self.assertFalse(out.exists(), "no bitmaps/ and no generated file: leave the tree alone")

    def test_rejects(self):
        with tempfile.TemporaryDirectory() as tmp:
            crate = Path(tmp)
            (crate / "src").mkdir()
            (crate / "bitmaps").mkdir()
            (crate / "bitmaps" / "wide.png").write_bytes(png(467, 1, 6, bytes([0, 0, 0, 255]) * 467))
            with self.assertRaisesRegex(ValueError, "467x1"):
                bitmaps.generate(crate)
            (crate / "bitmaps" / "wide.png").unlink()
            (crate / "bitmaps" / "a-b.png").write_bytes(png(1, 1, 6, bytes([0, 0, 0, 255])))
            (crate / "bitmaps" / "a_b.png").write_bytes(png(1, 1, 6, bytes([0, 0, 0, 255])))
            with self.assertRaisesRegex(ValueError, "both become A_B"):
                bitmaps.generate(crate)


if __name__ == "__main__":
    unittest.main()
