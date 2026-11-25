from collections.abc import Iterator
from dataclasses import dataclass
import enum
import struct
import zlib

from . import package
from .link import SYSTEM

MAX_DATA = 128
SLOTS = 16  # stub 8; a stub before it takes 0..7 (`slots`)
SLOTS_OLD = 8
LOADER_SLOT = 0xFF
STUB_SLOT = 0xFE  # INFO for it: the stub's version (docs/abi-1.4.md, "Stub version")
INFO, BEGIN, DATA, END, DELETE = 0x90A0, 0x90A1, 0x90A2, 0x90A3, 0x90A4
# the stub's model byte (StubVersion reply byte 6; a Watch Pro 2 stub sends none) and its DATA opcode: the Watch 3
# Pro's stock firmware handles 0x90A2 itself (Do Not Disturb sync), so its stub takes DATA on 0x90A9
MODEL_WP2, MODEL_WP3 = 1, 2
DATA_OPCODES = {MODEL_WP2: DATA, MODEL_WP3: 0x90A9}
MODEL_NAMES = {MODEL_WP2: "wp2", MODEL_WP3: "wp3"}
PUSH, PUSH_WIDGET, PUSH_BLOB = 0x90A8, 0, 1  # stub 8, docs/widget-push.md


class Status(enum.IntEnum):
    OK = 0
    BAD_REQUEST = 1
    IO = 2
    SEQUENCE = 3
    CRC = 4
    BUSY = 5
    INVALID = 6
    NO_APP = 7
    FULL = 8
    PAUSED = 9
    LATER = 10


@dataclass(frozen=True)
class Header:
    name: str
    version: int
    kind: int
    abi_minor: int
    # INFO returns only the header; the SDK puts an icon in front of the code, so the entry moves off 0
    icon: bool = False


def chunks(data: bytes) -> Iterator[tuple[int, bytes]]:
    for offset in range(0, len(data), MAX_DATA):
        yield offset, data[offset:offset + MAX_DATA]


async def _call(s, c2: int, payload: bytes) -> bytes:
    reply = await s.request(SYSTEM, c2, payload, c2 | 0xA000)
    if not reply:
        raise ValueError("empty reply")
    if reply[0] == Status.LATER and len(reply) == 5:
        raise ValueError(f"watch refused command {c2:#06x}: LATER, retry in {struct.unpack_from('<I', reply, 1)[0]} ms")
    if reply[0] != Status.OK:
        try:
            name = Status(reply[0]).name
        except ValueError:
            name = f"status {reply[0]}"
        raise ValueError(f"watch refused command {c2:#06x}: {name}")
    return reply[1:]


async def info(s, slot: int) -> Header | None:
    raw = await _call(s, INFO, bytes([slot]))
    if not raw:
        return None
    if len(raw) != package.HEADER_SIZE:
        raise ValueError(f"header is {len(raw)} bytes")
    _, _, kind, abi_minor, name, version, _, entry, *_ = package.HEADER.unpack(raw)
    return Header(name.rstrip(b"\0").decode("utf-8", "replace"), version, kind, abi_minor,
                  kind == package.KIND_APP and entry != 0)


@dataclass(frozen=True)
class Stub:
    version: int  # 1: a stub from before the version was readable
    abi_minor: int | None  # the highest package abi_minor it takes, None when it doesn't say
    model: int = MODEL_WP2  # MODEL_WP3 when the reply carries that model byte

    @property
    def data_opcode(self) -> int:
        return DATA_OPCODES[self.model]


async def stub(s) -> Stub:
    """Which stub the watch runs. Stock firmware has none and doesn't answer at all (the request times out)."""
    reply = await s.request(SYSTEM, INFO, bytes([STUB_SLOT]), INFO | 0xA000)
    if reply[:1] == bytes([Status.BAD_REQUEST]):
        return Stub(1, None)
    if len(reply) not in (6, 7) or reply[0] != Status.OK or len(reply) == 7 and reply[6] not in DATA_OPCODES:
        raise ValueError(f"unexpected stub version reply {reply.hex()}")
    return Stub(struct.unpack_from("<I", reply, 1)[0], reply[5], reply[6] if len(reply) == 7 else MODEL_WP2)


def slots(stub: Stub) -> int:
    """App slots the stub takes: INFO/BEGIN/DELETE of 8..15 answer BAD_REQUEST before stub 8."""
    return SLOTS if stub.version >= 8 else SLOTS_OLD


async def listing(s, count: int | None = None) -> dict[int, Header]:
    if count is None:
        count = slots(await stub(s))
    found = {}
    for slot in (*range(count), LOADER_SLOT):
        header = await info(s, slot)
        if header:
            found[slot] = header
    return found


async def install(s, data: bytes, slot: int | None = None) -> int:
    pkg = package.parse(data)
    if pkg.kind == package.KIND_LOADER and slot not in (None, LOADER_SLOT):
        raise ValueError("a loader package goes to slot L")
    if pkg.kind != package.KIND_LOADER and slot == LOADER_SLOT:
        raise ValueError("slot L takes only the loader package")
    st = await stub(s)
    if pkg.widget_icons and st.version < package.STUB_TRAILER:
        data = package.strip_trailer(data)  # an older stub refuses the extra bytes; it draws no icons anyway
    if pkg.kind == package.KIND_LOADER:
        slot = LOADER_SLOT
    elif slot is None:
        count = slots(st)
        used = await listing(s, count)
        same = [n for n, h in used.items() if n < count and h.name == pkg.name]
        free = [n for n in range(count) if n not in used]
        if not same and not free:
            raise ValueError("no free slot")
        slot = (same or free)[0]
    await upload(s, slot, data, st.data_opcode)
    return slot


async def upload(s, slot: int, data: bytes, data_opcode: int = DATA) -> None:
    """data_opcode: the stub's (Stub.data_opcode); DATA for a Watch Pro 2 stub."""
    await _call(s, BEGIN, struct.pack("<BII", slot, len(data), zlib.crc32(data)))
    for offset, part in chunks(data):
        reply = await _call(s, data_opcode, struct.pack("<I", offset) + part)
        if struct.unpack("<I", reply)[0] != offset + len(part):
            raise ValueError(f"watch expects offset {struct.unpack('<I', reply)[0]}, sent {offset + len(part)}")
    await _call(s, END, b"\0")


async def remove(s, slot: int) -> None:
    await _call(s, DELETE, bytes([slot]))


def widget_record(title: str, value: str) -> bytes:
    """The 64-byte widget file, as the SDK's widget_record: title (<= 23 bytes) at 0, value (<= 39) at 24, NUL-padded,
    each cut at a UTF-8 character boundary."""
    return _field(title, 24) + _field(value, 40)


V2, T_TEXT, T_COLUMNS = 0xFF, 0, 1
NO_ICON = 0xFF
STUB_V2 = 10  # the first stub that draws v2 records (wp2_core::widget::first_stub); 9 is the watch face guard


def columns_record(columns) -> bytes:
    """A v2 `columns` widget record (stub 10, docs/widget-push.md), the same bytes as the SDK's
    `Api::widget_columns`: FF 01 n, then per column caption[10] icon:u8 value[10], text UTF-8 cut at a character
    boundary to 9 bytes and NUL-padded. `columns`: 1..4 (caption, icon index or None, value)."""
    if not 1 <= len(columns) <= 4:
        raise ValueError(f"1..4 columns, got {len(columns)}")
    out = bytes([V2, T_COLUMNS, len(columns)])
    for caption, icon_index, value in columns:
        index = NO_ICON if icon_index is None else icon_index
        if not 0 <= index < 16 and index != NO_ICON:
            raise ValueError(f"icon index {icon_index} is not 0..15")
        out += _field(caption, 10) + bytes([index]) + _field(value, 10)
    return out


def _field(text: str, size: int) -> bytes:
    raw = text.encode("utf-8")[:size - 1]
    return raw.decode("utf-8", "ignore").encode("utf-8").ljust(size, b"\0")


async def push(s, slot: int, kind: int, data: bytes) -> None:
    """PUSH (stub 8): the app's widget record (v1 64 bytes; stub 10 also v2, up to 128) or start blob (<= 128 bytes);
    empty data removes it. Send v2 only to stub 10 or later (`stub`): an older one answers BAD_REQUEST."""
    await _call(s, PUSH, bytes([slot, kind]) + data)
