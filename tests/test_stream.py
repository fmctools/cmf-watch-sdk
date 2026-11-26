import struct
import unittest

from wp2apps import stream


class StreamTest(unittest.TestCase):
    def test_start_and_done(self):
        self.assertEqual(stream.start(64, 32), bytes([0xB5, 1, 64, 0, 32, 0]))
        self.assertEqual(stream.done(), bytes([0xB5, 3]))
        with self.assertRaisesRegex(ValueError, "outside"):
            stream.start(467, 1)

    def test_data_frame(self):
        self.assertEqual(stream.data(2, bytes([7, 0])), bytes([0xB5, 2, 2, 0, 0, 0, 7, 0]))
        for bad in [(1, b"\x07\x00"), (0, b""), (0, b"\x07"), (0, bytes(stream.CHUNK + 1) or b"x")]:
            with self.assertRaises(ValueError):
                stream.data(*bad)

    def test_messages_cover_the_image(self):
        px = bytes(range(32))  # 4 x 4 RGB565
        out = stream.messages(4, 4, px)
        self.assertEqual(out[0], stream.start(4, 4))
        self.assertEqual(out[-1], stream.done())
        self.assertEqual(out[1:-1], [stream.data(0, px)])

        px = bytes(range(128))
        out = stream.messages(8, 8, px)
        self.assertEqual([len(m) for m in out], [6, 6 + stream.CHUNK, 6 + 8, 2])
        offsets = [struct.unpack_from("<I", m, 2)[0] for m in out[1:-1]]
        self.assertEqual(offsets, [0, stream.CHUNK])
        self.assertEqual(b"".join(m[6:] for m in out[1:-1]), px)

        with self.assertRaisesRegex(ValueError, "pixel bytes"):
            stream.messages(4, 4, bytes(31))


if __name__ == "__main__":
    unittest.main()
