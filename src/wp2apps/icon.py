"""App icon: a descriptor at image offset 0, in front of the code (docs/app-icons.md).

    0x00 b"WP2I", u8 version 1, u8 cf 5 (LV_IMG_CF_TRUE_COLOR_ALPHA), u16 w, u16 h, u16 0, u32 size = w*h*3
    0x10 pixels: RGB565 LE + A8, row-major
"""
import argparse
from pathlib import Path
import struct
import sys
import zlib

DESC = struct.Struct("<4sBBHHHI")
MAGIC = b"WP2I"
VERSION = 1
CF_TRUE_COLOR_ALPHA = 5
SIDE = 96
# 4x4 Bayer thresholds: gradients survive the drop to RGB565 without bands
BAYER = (0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5)


def read_png(data: bytes) -> tuple[int, int, bytes]:
    """8-bit gray, RGB, palette, gray+alpha or RGBA, non-interlaced -> (w, h, rgba)."""
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    pos, idat = 8, b""
    w = h = 0
    color = plte = trns = None
    while pos < len(data):
        n, kind = struct.unpack_from(">I4s", data, pos)
        body = data[pos + 8:pos + 8 + n]
        if kind == b"IHDR":
            w, h, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", body)
            if depth != 8 or interlace != 0:
                raise ValueError("PNG must be 8-bit, not interlaced")
        elif kind == b"PLTE":
            plte = body
        elif kind == b"tRNS":
            trns = body
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    if color is None or color not in (0, 2, 3, 4, 6):
        raise ValueError(f"unsupported PNG color type {color}")
    if not w:
        raise ValueError("PNG has no IHDR")
    if color == 3 and (plte is None or len(plte) % 3):
        raise ValueError("palette PNG without a usable PLTE")

    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[color]
    raw = zlib.decompress(idat)
    stride = w * channels
    out, prev = bytearray(), bytearray(stride)
    for y in range(h):
        f, line = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - channels] if i >= channels else 0
            b, c = prev[i], prev[i - channels] if i >= channels else 0
            if f == 1:
                line[i] = (line[i] + a) & 0xFF
            elif f == 2:
                line[i] = (line[i] + b) & 0xFF
            elif f == 3:
                line[i] = (line[i] + (a + b) // 2) & 0xFF
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 0xFF
            elif f:
                raise ValueError(f"bad PNG filter {f}")
        out += line
        prev = line

    rgba = bytearray(w * h * 4)
    if color == 6:
        rgba[:] = out
    elif color == 2:
        for i in range(w * h):
            rgba[4 * i:4 * i + 3] = out[3 * i:3 * i + 3]
            rgba[4 * i + 3] = 0xFF
    elif color == 0:
        transparent = struct.unpack(">H", trns[:2])[0] if trns and len(trns) >= 2 else None
        for i, v in enumerate(out):
            rgba[4 * i:4 * i + 3] = bytes([v]) * 3
            rgba[4 * i + 3] = 0 if v == transparent else 0xFF
    elif color == 4:
        for i in range(w * h):
            g, a = out[2 * i], out[2 * i + 1]
            rgba[4 * i:4 * i + 3] = bytes([g]) * 3
            rgba[4 * i + 3] = a
    else:  # palette
        for i, p in enumerate(out):
            if p * 3 + 3 > len(plte):
                raise ValueError(f"palette index {p} has no entry")
            rgba[4 * i:4 * i + 3] = plte[3 * p:3 * p + 3]
            rgba[4 * i + 3] = trns[p] if trns and p < len(trns) else 0xFF
    return w, h, bytes(rgba)


def encode(w: int, h: int, rgba: bytes) -> bytes:
    if (w, h) != (SIDE, SIDE):
        raise ValueError(f"icon must be {SIDE}x{SIDE}, got {w}x{h}")
    px = pixels(w, h, rgba)
    return DESC.pack(MAGIC, VERSION, CF_TRUE_COLOR_ALPHA, w, h, 0, len(px)) + px


def pixels(w: int, h: int, rgba: bytes) -> bytes:
    """RGBA -> CF 5 pixels (RGB565 LE + A8), straight alpha, 4x4 ordered dither."""
    px = bytearray()
    for y in range(h):
        for x in range(w):
            r, g, b, a = rgba[4 * (y * w + x):4 * (y * w + x) + 4]
            t = (BAYER[(y & 3) * 4 + (x & 3)] + 0.5) / 16
            c = (min(31, int(r * 31 / 255 + t)) << 11 | min(63, int(g * 63 / 255 + t)) << 5
                 | min(31, int(b * 31 / 255 + t))) if a else 0
            px += struct.pack("<HB", c, a)
    return bytes(px)


def parse(image: bytes) -> tuple[int, int, bytes] | None:
    """The icon at the start of an app image: None without one, ValueError for a broken one."""
    if image[:4] != MAGIC:
        return None
    if len(image) < DESC.size:
        raise ValueError("icon descriptor is truncated")
    _, version, cf, w, h, reserved, size = DESC.unpack_from(image)
    if (version, cf, reserved) != (VERSION, CF_TRUE_COLOR_ALPHA, 0):
        raise ValueError(f"unsupported icon (version {version}, cf {cf})")
    if (w, h) != (SIDE, SIDE) or size != w * h * 3:
        raise ValueError(f"icon must be {SIDE}x{SIDE} with {SIDE * SIDE * 3} pixel bytes")
    if DESC.size + size > len(image):
        raise ValueError("icon pixels run past the image")
    return w, h, image[DESC.size:DESC.size + size]


def main(argv=None) -> int:
    args = argparse.ArgumentParser(prog="python -m wp2apps.icon", description="96x96 RGBA PNG -> icon descriptor")
    args.add_argument("png", type=Path)
    args.add_argument("output", type=Path)
    a = args.parse_args(argv)
    try:
        a.output.write_bytes(encode(*read_png(a.png.read_bytes())))
    except ValueError as error:
        print(f"Error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
