"""Bitmap assets: bitmaps/*.png -> src/bitmaps.gen.rs (docs/bitmap-assets.md).

Each PNG becomes a cmf_wp2_app::Bitmap of RGB565 pixels (2 bytes per pixel): alpha flattened onto black, a 4x4
ordered dither like the app icon. scripts/build-app and scripts/sim run this before cargo; the app includes the
result with cmf_wp2_app::bitmaps!().
"""
import argparse
import re
import struct
import sys
import zlib
from pathlib import Path

from . import icon

MAX_SIDE = 466
# Warn above this many pixel bytes: the package image (icon + code + data + bss + assets) is at most 128 KiB
SOFT_BYTES = 48 * 1024


def read_rgba(data: bytes) -> tuple[int, int, bytes]:
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


def rgb565(w: int, h: int, rgba: bytes) -> bytes:
    """RGBA -> 2 bytes per pixel RGB565 LE: alpha flattened onto black, 4x4 ordered dither (icon.BAYER)."""
    px = bytearray()
    for y in range(h):
        for x in range(w):
            r, g, b, a = rgba[4 * (y * w + x):4 * (y * w + x) + 4]
            if a != 255:
                r, g, b = r * a // 255, g * a // 255, b * a // 255
            t = (icon.BAYER[(y & 3) * 4 + (x & 3)] + 0.5) / 16
            c = (min(31, int(r * 31 / 255 + t)) << 11 | min(63, int(g * 63 / 255 + t)) << 5
                 | min(31, int(b * 31 / 255 + t)))
            px += struct.pack("<H", c)
    return bytes(px)


def ident(stem: str) -> str:
    """my-icon -> MY_ICON, a valid SCREAMING_SNAKE static name."""
    name = re.sub(r"[^0-9A-Za-z_]", "_", stem).upper()
    if not name or name[0].isdigit():
        name = "_" + name
    return name


def _pixels(px: bytes) -> str:
    words = struct.unpack(f"<{len(px) // 2}H", px)
    rows = [", ".join(str(v) for v in words[i:i + 16]) for i in range(0, len(words), 16)]
    return ",\n".join(" " * 16 + row for row in rows)


def generate(crate: Path) -> tuple[Path, int]:
    """Writes src/bitmaps.gen.rs for bitmaps/*.png (an empty module when there is no directory)."""
    source = crate / "bitmaps"
    out = crate / "src" / "bitmaps.gen.rs"
    files = sorted(source.glob("*.png")) if source.is_dir() else []
    if not files and not source.is_dir() and not out.exists():
        # a crate with no bitmap assets and no generated file: leave its tree alone
        return out, 0
    lines = ["// Generated by python -m wp2apps.bitmaps from bitmaps/*.png; do not edit.", "pub mod bitmaps {"]
    if files:
        lines.append("    use cmf_wp2_app::Bitmap;")
        lines.append("")
    seen: dict[str, str] = {}
    for f in files:
        w, h, rgba = read_rgba(f.read_bytes())
        if not (1 <= w <= MAX_SIDE and 1 <= h <= MAX_SIDE):
            raise ValueError(f"{f.name}: {w}x{h} outside 1..={MAX_SIDE}")
        name = ident(f.stem)
        if name in seen:
            raise ValueError(f"{f.name} and {seen[name]} both become {name}")
        seen[name] = f.name
        px = rgb565(w, h, rgba)
        if len(px) > SOFT_BYTES:
            print(f"warning: {f.name} is {len(px)} bytes of the 131072-byte package image", file=sys.stderr)
        lines += [f"    /// {f.name}: {w}x{h} RGB565, {len(px)} bytes in the package image.",
                  f"    pub static {name}: Bitmap = Bitmap {{ w: {w}, h: {h}, px: &[",
                  _pixels(px),
                  "    ] };"]
    lines += ["}", ""]
    text = "\n".join(lines)
    out.parent.mkdir(parents=True, exist_ok=True)
    if not out.exists() or out.read_text(encoding="utf-8") != text:  # keep cargo's mtime when nothing changed
        out.write_text(text, encoding="utf-8")
    return out, len(files)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(prog="python -m wp2apps.bitmaps",
                                     description="bitmaps/*.png -> src/bitmaps.gen.rs")
    parser.add_argument("crate", nargs="?", type=Path, default=Path("."),
                        help="the app crate directory (default: current directory)")
    a = parser.parse_args(argv)
    try:
        out, n = generate(a.crate)
    except (OSError, ValueError) as error:
        print(f"Error: {error}", file=sys.stderr)
        return 1
    print(f"{out}: {n} bitmap(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
