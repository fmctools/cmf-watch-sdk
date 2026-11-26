"""Finds where the stock About page shows the firmware version, for the "<version>-fmc" label (docs/recon.md).

Every site comes from byte signatures, not from a table of addresses, so the label carries to later stock releases;
each step refuses unless it finds exactly one match. Load base 0x10000000, file offset = address - base.

  version     the only NUL-terminated "N.N.N.N" string that literal-pool words point at.
  info site   the device-info fill `movs r2,#0x14; ldr r1,=version; add.w r0,r4,#F; bl strncpy`: F is the version
              field's offset in the info record (WP3 0x49, WP2 0x3F). AT GETVERSION and the BLE version replies read
              that field, so it is located only for F and never patched.
  page site   the About view's own read of the field: `add r1,sp,#B; movs r0,#ID; bl get_info`, then within 0x400
              bytes `add.w rD,sp,#(B+F)` hands the version text on (WP3: snprintf's first %s in r3; WP2: straight to
              the label setter in r1).
  fmt word    WP3 shape only (rD = r3): the pool word of the page's snprintf format ("%s\\n(%s %s)"), loaded by an
              `ldr r2,=fmt` just before the page site, with exactly one pool reference.
"""

from dataclasses import dataclass
import re
import struct

BASE = 0x10000000
SUFFIX = "-fmc"
MAX_LABEL = 19  # a sanity bound: real versions are 8 chars; the page copies the text (WP2) or formats it into 0x80 bytes (WP3)
ABOUT_VIEW = b"about_view"


class LocateError(ValueError):
    pass


@dataclass(frozen=True)
class Sites:
    version: str
    version_off: int  # file offset of the version string
    info_pool_off: int  # file offset of the info fill's pool word (every reader of the field: never patched)
    field: int  # version field offset in the info record
    about_rec: int  # address of about_view's view-table record
    about_proc: int  # its view proc (Thumb bit set)
    getter_call: int  # address of the About page's `bl get_info`
    info_id: int
    page_add: int  # address of `add.w rD, sp, #(B+F)` in the About page
    page_rd: int
    fmt_pool_off: int | None  # WP3 shape: file offset of the page's format pool word
    fmt: bytes | None

    @property
    def label(self) -> str:
        return fmc_label(self.version)


def fmc_label(version: str) -> str:
    label = version + SUFFIX
    if len(label) > MAX_LABEL:
        raise LocateError(f"label {label!r} is {len(label)} chars; the limit is {MAX_LABEL}")
    return label


def _h(app: bytes, at: int) -> int:
    return struct.unpack_from("<H", app, at - BASE)[0]


def _u32(app: bytes, off: int) -> int:
    return struct.unpack_from("<I", app, off)[0]


def _cstr(app: bytes, addr: int) -> bytes | None:
    off = addr - BASE
    end = app.find(b"\0", off) if 0 <= off < len(app) else -1
    return None if end < 0 else app[off:end]


def pool_refs(app: bytes, addr: int) -> list[int]:
    """File offsets of the 4-aligned words equal to `addr`."""
    word = re.escape(struct.pack("<I", addr))
    return [m.start() for m in re.finditer(word, app) if m.start() % 4 == 0]


def _ldr_lit(app: bytes, at: int) -> tuple[int, int] | None:
    """16-bit `ldr rt, [pc, #imm8*4]` at `at` -> (rt, pool address)."""
    hw = _h(app, at)
    if hw & 0xF800 != 0x4800:
        return None
    return (hw >> 8) & 7, ((at + 4) & ~3) + (hw & 0xFF) * 4


def bl_target(app: bytes, at: int) -> int | None:
    h1, h2 = _h(app, at), _h(app, at + 2)
    if h1 & 0xF800 != 0xF000 or h2 & 0xD000 != 0xD000:
        return None
    s = (h1 >> 10) & 1
    i1, i2 = 1 - (((h2 >> 13) & 1) ^ s), 1 - (((h2 >> 11) & 1) ^ s)
    off = (s << 24) | (i1 << 23) | (i2 << 22) | ((h1 & 0x3FF) << 12) | ((h2 & 0x7FF) << 1)
    return at + 4 + off - (s << 25)


def _add_w(app: bytes, at: int, rn: int) -> tuple[int, int] | None:
    """`add.w rd, rn, #imm8` (T3 with i = imm3 = 0) -> (rd, imm8)."""
    h1, h2 = _h(app, at), _h(app, at + 2)
    if h1 != 0xF100 | rn or h2 & 0xF000:
        return None
    return (h2 >> 8) & 0xF, h2 & 0xFF


def find_version(app: bytes) -> tuple[str, int, list[int]]:
    """(version, file offset, pool refs). Left edge 'not a digit or dot': WP2's string follows a non-NUL byte."""
    hits = []
    for m in re.finditer(rb"(?<![0-9.])(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,4})\0", app):
        refs = pool_refs(app, BASE + m.start(1))
        if refs:
            hits.append((m.group(1).decode(), m.start(1), refs))
    if len(hits) != 1:
        raise LocateError(f"version: expected one referenced N.N.N.N string, found {[(v, hex(o)) for v, o, _ in hits]}")
    return hits[0]


def find_info_site(app: bytes, refs: list[int]) -> tuple[int, int]:
    """(pool word file offset, field offset)."""
    out = []
    for at in range(BASE + 2, BASE + len(app) - 10, 2):
        d = _ldr_lit(app, at)
        if not d or d[0] != 1 or d[1] - BASE not in refs or _h(app, at - 2) != 0x2214:  # movs r2, #0x14
            continue
        a = _add_w(app, at + 2, 4)
        if a and a[0] == 0 and bl_target(app, at + 6) is not None:
            out.append((d[1] - BASE, a[1]))
    if len(out) != 1:
        raise LocateError(f"info site: expected one match, found {len(out)}")
    return out[0]


def find_view(app: bytes, name: bytes = ABOUT_VIEW) -> tuple[int, int]:
    """(record address, view proc) of the view-table record named `name`. The proc is the first non-zero of words 1
    and 2 (stock uses either). A loader build keeps a copy of the table after its stub: identical records count once."""
    names = [m.start() + 1 for m in re.finditer(b"\0" + re.escape(name) + b"\0", app)]
    if len(names) != 1:
        raise LocateError(f"{name.decode()}: expected one name string, found {len(names)}")
    found = {}
    for r in pool_refs(app, BASE + names[0]):
        if r + 12 > len(app):
            continue
        proc = _u32(app, r + 4) or _u32(app, r + 8)
        if proc & 1 and 0 <= proc - BASE < len(app):
            found.setdefault(proc, BASE + r)
    if len(found) != 1:
        raise LocateError(f"{name.decode()}: expected one view record, found {len(found)}")
    (proc, rec), = found.items()
    return rec, proc


def find_page_site(app: bytes, proc: int, field: int, window: int = 0x1000) -> tuple[int, int, int, int]:
    """(getter call, info id, page add, rD): exactly one `add r1,sp,#B; movs r0,#ID; bl` within `window` of the
    proc with an `add.w rD,sp,#(B+field)` in the 0x400 bytes after it."""
    pairs = []
    for at in range(max(BASE, (proc & ~1) - window), min(BASE + len(app) - 8, proc + window), 2):
        h, n = _h(app, at), _h(app, at + 2)
        if h & 0xFF00 != 0xA900 or n & 0xFF00 != 0x2000 or bl_target(app, at + 4) is None:
            continue
        b = (h & 0xFF) * 4
        for at2 in range(at + 8, min(at + 0x400, BASE + len(app) - 4), 2):
            a = _add_w(app, at2, 13)
            if a and a[1] == b + field:
                pairs.append((at + 4, n & 0xFF, at2, a[0]))
    if len(pairs) != 1:
        raise LocateError(f"page site: expected one match, found {[(hex(p[0]), hex(p[2])) for p in pairs]}")
    return pairs[0]


def find_fmt_word(app: bytes, page_add: int, page_rd: int) -> tuple[int | None, bytes | None]:
    """WP3 shape: rD = r3 and one `ldr r2,=fmt` in the 16 bytes before the page site, the format's first conversion
    is %s and its pool word is its only reference. Otherwise (None, None): the page gets the thunk."""
    if page_rd != 3:
        return None, None
    found = []
    for at in range(page_add - 16, page_add, 2):
        d = _ldr_lit(app, at)
        if d and d[0] == 2:
            fmt = _cstr(app, _u32(app, d[1] - BASE))
            if fmt and re.match(rb"[^%]*%s", fmt):
                found.append((d[1] - BASE, fmt))
    if len(found) != 1 or len(pool_refs(app, _u32(app, found[0][0]))) != 1:
        return None, None
    return found[0]


def locate(app: bytes) -> Sites:
    version, version_off, refs = find_version(app)
    fmc_label(version)
    info_pool, field = find_info_site(app, refs)
    rec, proc = find_view(app)
    call, info_id, add, rd = find_page_site(app, proc, field)
    fmt_off, fmt = find_fmt_word(app, add, rd)
    return Sites(version, version_off, info_pool, field, rec, proc, call, info_id, add, rd, fmt_off, fmt)
