"""Stream a PNG to the bitmap-stream app over the host channel (temporary prototype, docs/bitmap-assets.md).

Messages, little endian, at most MSG_MAX 128 bytes each:
    START [B5 01 w u16 h u16]
    DATA  [B5 02 offset u32][RGB565 payload, even length, at most 120 bytes]
    DONE  [B5 03]
The offset is a byte offset into the row-major image, always even, so payloads read as RGB565 words on the watch;
they may land anywhere (duplicates and gaps are harmless: every chunk redraws itself).
"""
import argparse
import asyncio
import struct
import sys
from pathlib import Path

from . import bitmaps
from .channel import ANY, Channel, SimChannel

MAGIC = 0xB5
START = 1
DATA = 2
DONE = 3
# 6-byte header + payload <= 128; 120 keeps every payload even
CHUNK = 120


def start(w: int, h: int) -> bytes:
    if not (1 <= w <= 466 and 1 <= h <= 466):
        raise ValueError(f"image {w}x{h} outside 1..=466")
    return struct.pack("<BBHH", MAGIC, START, w, h)


def data(offset: int, px: bytes) -> bytes:
    if offset % 2 or not px or len(px) % 2 or len(px) > CHUNK:
        raise ValueError(f"DATA needs an even nonzero payload up to {CHUNK} bytes at an even offset")
    return struct.pack("<BBI", MAGIC, DATA, offset) + px


def done() -> bytes:
    return bytes([MAGIC, DONE])


def messages(w: int, h: int, px: bytes) -> list[bytes]:
    """START, one or more DATA, DONE for w*h RGB565 pixels as bytes."""
    if len(px) != w * h * 2:
        raise ValueError(f"{w}x{h} needs {w * h * 2} pixel bytes, got {len(px)}")
    return [start(w, h), *(data(off, px[off:off + CHUNK]) for off in range(0, len(px), CHUNK)), done()]


async def stream(channel, png: Path, pace: float = 0.01) -> int:
    """Sends one image; returns the message count. pace keeps the 4-deep watch inbox from overflowing."""
    w, h, rgba = bitmaps.read_rgba(png.read_bytes())
    out = messages(w, h, bitmaps.rgb565(w, h, rgba))
    for i, m in enumerate(out):
        await channel.send(m)
        if pace and i + 1 < len(out):
            await asyncio.sleep(pace)
    return len(out)


def _channel(a):
    if a.tcp:
        return SimChannel(a.tcp, slot=a.slot)
    return Channel(address=a.address, hci_port=a.hci_port, slot=a.slot, channel_key=a.channel_key)


def main(argv=None) -> int:
    p = argparse.ArgumentParser(prog="python -m wp2apps.stream",
                                description="stream a PNG to the bitmap-stream app")
    p.add_argument("png", type=Path)
    link = p.add_mutually_exclusive_group(required=True)
    link.add_argument("--address", help="BLE address of the watch")
    link.add_argument("--hci-port", type=int, help="HCI port (wp2apps list prints one)")
    link.add_argument("--tcp", type=int, help="port of scripts/sim <app> --tcp PORT")
    p.add_argument("--slot", type=lambda s: int(s, 0), default=ANY, help="the app's slot (default: any)")
    p.add_argument("--channel-key", action="store_true", help="use the K host channel (docs/abi-1.4.md)")
    p.add_argument("--pace", type=float, default=0.01, help="seconds between messages (default 0.01)")
    a = p.parse_args(argv)

    async def run():
        async with _channel(a) as channel:
            return await stream(channel, a.png, a.pace)

    try:
        n = asyncio.run(run())
    except (OSError, ValueError, TimeoutError) as error:
        print(f"Error: {error or type(error).__name__}", file=sys.stderr)
        return 1
    print(f"{a.png.name}: {n} messages sent")
    return 0


if __name__ == "__main__":
    sys.exit(main())
