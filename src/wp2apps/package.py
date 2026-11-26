from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path
import struct
import zlib

from . import icon

HEADER = struct.Struct("<4sHBB24sIIIIII8s")
HEADER_CRC_SPAN = 0x34
HEADER_SIZE = HEADER.size
MAGIC = b"WP2A"
ABI = 1
ABI_MINOR = 0
KIND_APP = 0
KIND_LOADER = 1
APP_SIZE = 0x20000
LOADER_SIZE = 0x10000
NAME_BYTES = 23
# header byte 0x38 (the first reserved byte): the model a loader is built for, 0 for a Watch Pro 2 loader and for
# every app (apps are model-free). A WP2 stub rejects any non-zero reserved byte, so it refuses a Watch 3 Pro
# loader; the Watch 3 Pro stub requires its model there (wp2-core port.rs).
MODEL_OFFSET = 0x38
LOADER_MODELS = {"wp2": 0, "wp3": 2}


WIDGET, PAGE = 1, 2  # app_features! flags (sdk cmf-wp2-app, wp2-core pkg.rs)


def features(image: bytes) -> list[str]:
    """What the app declared with app_features!: the record after the icon, or at offset 0 without one."""
    at = 0
    if icon.parse(image) is not None:
        at = icon.DESC.size + icon.SIDE * icon.SIDE * 3
    flags = image[at + 4] if image[at:at + 4] == b"WP2F" and len(image) > at + 4 else 0
    return [name for bit, name in ((WIDGET, "widget"), (PAGE, "page")) if flags & bit]


# Widget icons (docs/app-icons.md, "Widget icons"): a trailer after the relocation table, outside body_crc and the
# loaded image. "WP2W", version 1, count 1..16, size 40, 0, crc32 of the pixels, count x 40x40 CF 5 pixels.
TRAILER = struct.Struct("<4sBBBBI")
TRAILER_MAGIC = b"WP2W"
WIDGET_ICON_SIDE = 40
WIDGET_ICON_BYTES = WIDGET_ICON_SIDE * WIDGET_ICON_SIDE * 3
WIDGET_ICONS_MAX = 16
STUB_TRAILER = 10  # the first stub that takes a package with a trailer (older ones, 9 too, check the exact size)


def widget_trailer(icons: Sequence[bytes]) -> bytes:
    if not 1 <= len(icons) <= WIDGET_ICONS_MAX:
        raise ValueError(f"1..{WIDGET_ICONS_MAX} widget icons, got {len(icons)}")
    if any(len(i) != WIDGET_ICON_BYTES for i in icons):
        raise ValueError(f"widget icons are {WIDGET_ICON_SIDE}x{WIDGET_ICON_SIDE} CF 5 ({WIDGET_ICON_BYTES} bytes)")
    px = b"".join(icons)
    return TRAILER.pack(TRAILER_MAGIC, 1, len(icons), WIDGET_ICON_SIDE, 0, zlib.crc32(px)) + px


def parse_trailer(data: bytes) -> tuple[bytes, ...]:
    """The widget icons of a trailer; ValueError for a broken one."""
    if len(data) < TRAILER.size:
        raise ValueError("widget icon trailer is truncated")
    magic, version, count, side, zero, crc = TRAILER.unpack_from(data)
    if (magic, version, side, zero) != (TRAILER_MAGIC, 1, WIDGET_ICON_SIDE, 0) or not 1 <= count <= WIDGET_ICONS_MAX:
        raise ValueError("bad widget icon trailer head")
    px = data[TRAILER.size:]
    if len(px) != count * WIDGET_ICON_BYTES:
        raise ValueError(f"widget icon trailer has {len(px)} pixel bytes for {count} icons")
    if zlib.crc32(px) != crc:
        raise ValueError("widget icon trailer CRC mismatch")
    return tuple(px[i:i + WIDGET_ICON_BYTES] for i in range(0, len(px), WIDGET_ICON_BYTES))


def widget_icons_dir(directory: Path) -> list[bytes]:
    """The 40x40 RGBA PNGs of `directory`, in file name order (scripts/build-app renders widget-icons/NN-name.svg)."""
    from . import icon
    icons = []
    for png in sorted(directory.glob("*.png")):
        w, h, rgba = icon.read_png(png.read_bytes())
        if (w, h) != (WIDGET_ICON_SIDE, WIDGET_ICON_SIDE):
            raise ValueError(f"{png.name} is {w}x{h}; widget icons are {WIDGET_ICON_SIDE}x{WIDGET_ICON_SIDE}")
        icons.append(icon.pixels(w, h, rgba))
    return icons


def strip_trailer(data: bytes) -> bytes:
    """The package without its widget icon trailer, for a stub before STUB_TRAILER: the same bytes as a package
    packed without icons (body_crc never covers the trailer)."""
    _, _, _, _, _, _, size, _, count, *_ = HEADER.unpack_from(data)
    return data[:HEADER_SIZE + size + 4 * count]


@dataclass(frozen=True)
class Package:
    kind: int
    abi: int
    abi_minor: int
    name: str
    version: int
    entry: int
    image: bytes
    relocs: tuple[int, ...]
    widget_icons: tuple[bytes, ...] = ()
    model: str = "wp2"  # a loader's model (LOADER_MODELS); "wp2" for every app


SHN_ABS = 0xFFF1
R_ARM_ABS32 = 2
PC_RELATIVE = {0, 3, 10, 11, 30, 42, 51, 53, 54, 102, 103}


def _check(image: bytes, name: bytes, kind: int, entry: int, relocs: Sequence[int]) -> None:
    if not 1 <= len(name) <= NAME_BYTES:
        raise ValueError(f"name must be 1..{NAME_BYTES} UTF-8 bytes, got {len(name)}")
    if kind not in (KIND_APP, KIND_LOADER):
        raise ValueError(f"unknown kind {kind}")
    if not 0 <= entry < len(image):
        raise ValueError(f"entry {entry:#x} is outside the {len(image)}-byte image")
    limit = APP_SIZE if kind == KIND_APP else LOADER_SIZE
    if len(image) > limit:
        raise ValueError(f"image is {len(image)} bytes; the region holds {limit}")
    previous = -1
    for off in relocs:
        if off <= previous or off + 4 > len(image):
            raise ValueError(f"relocation {off:#x} is out of order or outside the image")
        previous = off
    if kind == KIND_APP:
        icon.parse(image)


def pack(image: bytes, *, name: str, version: int, entry: int = 0, kind: int = KIND_APP,
         relocs: Sequence[int] = (), abi_minor: int = ABI_MINOR, widget_icons: Sequence[bytes] = (),
         model: str = "wp2") -> bytes:
    encoded = name.encode("utf-8")
    _check(image, encoded, kind, entry, relocs)
    if not 0 <= abi_minor <= 0xFF:
        raise ValueError(f"abi_minor {abi_minor} does not fit the header")
    table = struct.pack(f"<{len(relocs)}I", *relocs)
    fields = [MAGIC, ABI, kind, abi_minor, encoded, version, len(image), entry, len(relocs),
              zlib.crc32(image + table), 0, b""]
    fields[10] = zlib.crc32(HEADER.pack(*fields)[:HEADER_CRC_SPAN])
    if widget_icons and kind != KIND_APP:
        raise ValueError("only an app carries widget icons")
    if model not in LOADER_MODELS:
        raise ValueError(f"unknown model {model!r}")
    if LOADER_MODELS[model] and kind != KIND_LOADER:
        raise ValueError("only a loader carries a model; apps run on every model")
    fields[11] = bytes([LOADER_MODELS[model]]) + bytes(7)
    return HEADER.pack(*fields) + image + table + (widget_trailer(widget_icons) if widget_icons else b"")


def parse(data: bytes) -> Package:
    if len(data) < HEADER_SIZE:
        raise ValueError("package is shorter than its header")
    magic, abi, kind, abi_minor, raw_name, version, size, entry, count, crc, header_crc, reserved = \
        HEADER.unpack_from(data)
    if magic != MAGIC:
        raise ValueError("not a WP2 app package")
    if zlib.crc32(data[:HEADER_CRC_SPAN]) != header_crc:
        raise ValueError("header CRC mismatch")
    if any(reserved[1:]) or reserved[0] not in LOADER_MODELS.values() or reserved[0] and kind != KIND_LOADER:
        raise ValueError("reserved header bytes are not zero")
    if abi != ABI:
        raise ValueError(f"unsupported ABI {abi}")
    end = HEADER_SIZE + size + 4 * count
    if len(data) < end or (len(data) > end and kind != KIND_APP):
        raise ValueError(f"header says {size} image bytes and {count} relocations, "
                         f"package has {len(data) - HEADER_SIZE} bytes")
    widget_icons = parse_trailer(data[end:]) if len(data) > end else ()
    body = data[HEADER_SIZE:end]
    if zlib.crc32(body) != crc:
        raise ValueError("image CRC mismatch")
    image = body[:size]
    relocs = struct.unpack(f"<{count}I", body[size:])
    encoded = raw_name.rstrip(b"\0")
    try:
        name = encoded.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ValueError("name is not valid UTF-8") from error
    _check(image, encoded, kind, entry, relocs)
    model = next(m for m, b in LOADER_MODELS.items() if b == reserved[0])
    return Package(kind, abi, abi_minor, name, version, entry, image, relocs, widget_icons, model)


def _sections(elf: bytes):
    if elf[:4] != b"\x7fELF":
        raise ValueError("not an ELF file")
    shoff, = struct.unpack_from("<I", elf, 0x20)
    shentsize, shnum = struct.unpack_from("<HH", elf, 0x2E)
    return [struct.unpack_from("<10I", elf, shoff + i * shentsize) for i in range(shnum)]


def relocations(elf: bytes) -> list[int]:
    sections = _sections(elf)
    offsets = []
    for _, kind, _, _, off, size, link, info, _, _ in sections:
        if kind == 4:
            raise ValueError("SHT_RELA sections are not supported")
        if kind != 9 or not sections[info][2] & 2:
            continue
        symtab = sections[link][4]
        for rel in range(off, off + size, 8):
            r_offset, r_info = struct.unpack_from("<II", elf, rel)
            typ, sym = r_info & 0xFF, r_info >> 8
            if typ not in PC_RELATIVE and typ != R_ARM_ABS32:
                raise ValueError(f"unsupported relocation type {typ} at {r_offset:#x}; "
                                 "build with -C target-feature=+no-movt")
            shndx, = struct.unpack_from("<H", elf, symtab + 16 * sym + 14)
            if shndx == SHN_ABS and typ != 0:
                raise ValueError(f"relocation at {r_offset:#x} targets an absolute symbol")
            if typ == R_ARM_ABS32:
                offsets.append(r_offset)
    return sorted(offsets)


def check_elf(image: bytes, elf: bytes) -> int:
    """The entry offset; the SDK puts the app's icon in front of the code, so it need not be 0."""
    sections = _sections(elf)
    entry, = struct.unpack_from("<I", elf, 0x18)
    end = max((addr + size for _, _, flags, addr, _, size, *_ in sections if flags & 2), default=0)
    if not entry & 1 or entry > len(image):
        raise ValueError(f"ELF entry is {entry:#x}; images must be linked at 0 with a Thumb entry (use app.ld)")
    if len(image) != end:
        raise ValueError(f"image is {len(image)} bytes but the ELF occupies {end}; .bss missing?")
    return entry - 1


def abi_minor(image: bytes, elf: bytes) -> int:
    """The SDK's WP2_ABI_MINOR (u16 in the image at its symbol), 0 for an image without it (loader, old SDK)."""
    sections = _sections(elf)
    for _, kind, _, _, off, size, link, _, _, entsize in sections:
        if kind != 2:
            continue
        strtab = sections[link][4]
        for sym in range(off, off + size, entsize or 16):
            name_off, value = struct.unpack_from("<II", elf, sym)
            end = elf.index(b"\0", strtab + name_off)
            if elf[strtab + name_off:end] == b"WP2_ABI_MINOR":
                if value + 2 > len(image):
                    raise ValueError("WP2_ABI_MINOR lies outside the image")
                return struct.unpack_from("<H", image, value)[0]
    return 0


ORDER_MAGIC = b"WP2ORDER"
ORDER_MAX = 16


def set_order(data: bytes, names: Sequence[str]) -> bytes:
    """The loader package with the menu order `names` in its WP2ORDER table (docs/app-order.md)."""
    pkg = parse(data)
    if pkg.kind != KIND_LOADER:
        raise ValueError("the app order goes into the loader package")
    if len(names) > ORDER_MAX:
        raise ValueError(f"at most {ORDER_MAX} names, got {len(names)}")
    at = pkg.image.find(ORDER_MAGIC) + len(ORDER_MAGIC)
    if at < len(ORDER_MAGIC) or pkg.image.count(ORDER_MAGIC) != 1:
        raise ValueError("loader has no single WP2ORDER table (loader older than the app order?)")
    end = at + 24 * ORDER_MAX
    if end > len(pkg.image) or any(at - 3 <= r < end for r in pkg.relocs):
        raise ValueError("WP2ORDER table is cut short or relocated")
    table = b""
    for name in names:
        encoded = name.encode("utf-8")
        if not 1 <= len(encoded) <= NAME_BYTES:
            raise ValueError(f"name {name!r} must be 1..{NAME_BYTES} UTF-8 bytes")
        table += encoded.ljust(24, b"\0")
    image = pkg.image[:at] + table.ljust(24 * ORDER_MAX, b"\0") + pkg.image[end:]
    return pack(image, name=pkg.name, version=pkg.version, entry=pkg.entry, kind=pkg.kind, relocs=pkg.relocs,
                abi_minor=pkg.abi_minor)
