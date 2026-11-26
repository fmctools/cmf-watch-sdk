import hashlib
import lzma
import re
import struct
import zlib

from . import fmclabel

STOCK_SHA256_PREFIX = "30e7978c39d8fb79"
STUB_OFFSET = 0x25A080
HEAP_LITERALS = (0x188508, 0x188588, 0x1885F4)
HEAP_OLD = 0x676B80
HEAP_BASE = 0x380A9340
HEAP_NEW = 0x675B80
STUB_RAM = HEAP_BASE + HEAP_NEW
STUB_RAM_SIZE = 0x1000
APP_VBASE = 0x10000000
NOR_APP_OFFSET = 0x24000
NOR_APP_SIZE = 0x34B000
STUB_MAX = 0x6000  # own budget, the NOR app partition has ~964 KiB after app.bin (docs/recon.md)
STUB_HEADER = struct.Struct("<4sIIIIIII")
# header version 2 adds the watch face guard's hooks (fw/wp2-stub/src/wfguard.rs)
STUB_HEADER_V2 = struct.Struct("<4sIIIIIII5I")
VIEW_TABLE, VIEW_TABLE_END = 0x100004B8, 0x100013E8
# 0x101461AC and 0x10055458 hold the same values but bound neighbouring tables (docs/recon.md)
VIEW_START_LITERALS = (0x1018BC24, 0x1018DF3C)
VIEW_END_LITERALS = (0x1018BC28, 0x1018DF40)
VIEW_ID = 0xF0
# an app's own page next to the face: the same view proc, the loader starts slot `id - 0xF8` (wp2-core cards.rs)
# (stub 8: 16 slots, 0x100..0x107 past the stock ids, which end at 237)
PAGE_VIEWS = range(0xF8, 0x108)
BOOT_CALL = 0x101884FA
STOCK_HEAP_INIT = 0x10146AA4
BLE_CALL = 0x100D34F0
# the watch face guard's hooks in the order of both stub headers
WF_BOOT, WF_SAVED, POWER_DOWN, FORCED_REBOOT, WF_PARSE = range(5)
# (bl site, its stock target, hook), docs/recon.md "Watch face boot guard"
GUARD_WP2 = (
    (0x100DE098, 0x101B93B6, WF_BOOT),  # WFManager_Init: the list is final, saved or default
    (0x100B2264, 0x100DD16C, WF_SAVED),  # the face list editor closes
    (0x100DD1F6, 0x100DD16C, WF_SAVED),  # WFManager_Load_ByIdx of a picked index
    (0x100DD42E, 0x100DD16C, WF_SAVED),  # WFManager_SetCurrIdxForUi
    (0x100DE1E8, 0x100DD16C, WF_SAVED),  # the phone's set, list or upload (WFM PendHandle)
    (0x10148C9E, 0x1014A8FC, POWER_DOWN),  # system_power_off
    (0x10148D32, 0x1014A8FC, POWER_DOWN),  # system_power_reboot
    (0x101CE678, 0x100C6234, FORCED_REBOOT),  # ota force reboot
    (0x100DD2A6, 0x100DC3C0, WF_PARSE),  # WFManager_Load_ByIdx parses the active face
)
STOCK_BLE_READ = 0x100CC284
# Watch 3 Pro jx402_08 1.0.1.77 (wp2-core wfguard.rs WP3): the stub goes at the end of app.bin
WP3_STOCK_SHA256_PREFIX = "815a21fc80aa3941"
WP3_STUB_OFFSET = 0x2BD280
# stub and the copied view table (fw/wp3-stub/link.ld); the app partition's real size on WP3 is unmeasured
WP3_STUB_MAX = 0x5000
WP3_STUB_HEADER = struct.Struct("<4sI5I")
# header version 2 (fw/wp3-stub): the guard's hooks, the app list's List and Grid update handlers, the view proc
# and the view name of the stub's view WP3_VIEW_ID
WP3_STUB_HEADER_V2 = struct.Struct("<4sI9I")
# version 3 adds the boot hook (StubRam) and the install channel's hook (fw/wp3-stub ble.rs)
WP3_STUB_HEADER_V3 = struct.Struct("<4sI11I")
WP3_HEADERS = {1: WP3_STUB_HEADER, 2: WP3_STUB_HEADER_V2, 3: WP3_STUB_HEADER_V3}
# ResFbHeap size in three literal slots (model.rs heap_literals); the stub's StubRam takes its top 4 KiB
WP3_HEAP_LITERALS = (0x1019C654, 0x1019C700, 0x1019C76C)
WP3_HEAP_OLD = 0x636B00
WP3_HEAP_NEW = WP3_HEAP_OLD - STUB_RAM_SIZE
WP3_BOOT_CALL, WP3_STOCK_HEAP_INIT = 0x1019C646, 0x10152F18
# `bl read_payload` in the BLE dispatcher's default branch; the frame is in r4 there
WP3_BLE_CALL, WP3_STOCK_BLE_READ = 0x100C253A, 0x100C0EEC
# (rodata slot, its stock handler): the update handler (+8 after the 0xC18487B4 marker) of the List and of the Grid
# style's view ops, in the header's order (wp2-core model.rs applist_hook_slot / applist_grid_hook_slot)
WP3_APPLIST = ((0x10215A68, 0x10047351), (0x10215948, 0x10046D99))
# 20-byte records {name, proc (word 1), 0, u16 id, u16 0x102, u16 466, u16 466} (model.rs view_*)
WP3_VIEW_TABLE, WP3_VIEW_TABLE_END = 0x1000050C, 0x1000136C
WP3_VIEW_START_LITERALS = (0x101A1304, 0x101A3194)
WP3_VIEW_END_LITERALS = (0x101A1308, 0x101A3198)
WP3_VIEW_RECORD = struct.Struct("<IIIHHHH")
# model.rs menu_view: a free wake-lock id (0x118..0x146 are msgbox and wake ids), the loader's lock later
WP3_VIEW_ID = 0x147
GUARD_WP3 = (
    (0x100D6E20, 0x101D5A02, WF_BOOT),
    (0x100A5D84, 0x100D577C, WF_SAVED),
    (0x100D5804, 0x100D577C, WF_SAVED),
    (0x100D5A5A, 0x100D577C, WF_SAVED),
    (0x100D6F78, 0x100D577C, WF_SAVED),
    (0x10155226, 0x101570C0, POWER_DOWN),
    (0x101552C8, 0x101570C0, POWER_DOWN),
    (0x1015533A, 0x100BA94C, FORCED_REBOOT),
    (0x100D58B2, 0x100D48B4, WF_PARSE),
)
# list style descriptor +0xC: its layout; waterfall (0x101EB184) is not covered
APPLIST_LAYOUT = 0x101EB160
STOCK_APPLIST_LAYOUT = 0x1005EEE5
# widget registry of the cards left of the watch face: 99 pointers, NULL at 0x40 (docs/recon.md, "Widget cards");
# the stub's entry: {u8 id, u8 size class, u16 state size, u32 scene, u32 tap action, 5 Thumb callbacks}
WGT_REG = 0x101EFC88
WGT_ID = 0x40
WGT_ENTRY = struct.Struct("<BBHII5I")
# the only stock call that switches the motor on (docs/recon.md); 0x100FB168 and 0x100FB2A8 pass pulse 0
MOTOR_CALL = 0x100FB398
STOCK_MOTOR = 0x100FB4B4
# zero gap between the end of the stock image (0x10259E3C) and the ksdfs tail (0x1025A000); stock only reads
# 32 bytes at 0x10259E3B there. Inside app.bin, so a motor-only patch keeps the file size
MOTOR_HOOK = 0x10259F00
MOTOR_HOOK_SIZE = 14
# vibration settings (setvib_view): an LVGL roller, options = "%s\n%s\n%s" of Strong / Standard / Off; the roller
# index is the level (property 0x24), levels index a strength table checked with `cmp #2` (docs/recon.md)
LEVELS = 0x101F4A00
LEVELS_LITERALS = (0x100E17F0, 0x100E1810)
LEVELS_BOUNDS = (0x100E17CE, 0x100E17FA)  # cmp r3, #2
ROLLER_FORMAT = 0x1021D65D
ROLLER_FORMAT_LITERALS = (0x100BBADC, 0x100BBBD0)  # create, language refresh
CROWN_MAX = 0x100BB892  # cmp r1, #1: the crown steps up while index <= 1
WEAK_TABLE = 0x10259E80  # in the zero gap, after the 32 bytes stock reads
WEAK_FORMAT = WEAK_TABLE + 16
# Latin label: the selected row's font48m has full ASCII but only `БИРСавгдзийклнрстуъыь` of Cyrillic ("Слабый" lacks б)
WEAK_LABEL = "Weak"
# Weak is shown between Standard and Off (roller index 2) but stays level 3, so Off keeps level 2 (phone sync):
# index <-> level swaps 2 and 3 where setvib_view crosses between them
STOCK_PREVIEW, STOCK_SET_LEVEL, STOCK_GET_LEVEL = 0x100BB730, 0x100BB6F4, 0x100BB6B8
PREVIEW_CALLS = ((0x100BB8AE, "b.w"), (0x100BB938, "bl"))  # crown, value changed: preview(index)
SET_LEVEL_CALLS = (0x100BBC00,)  # check button: level = index
GET_LEVEL_CALLS = (0x100BB966, 0x100BBA3E)  # create: index = level, roller selected = level
# cmp r0,#2; beq 1f; cmp r0,#3; bne 2f; movs r0,#2; b 2f; 1: movs r0,#3; 2:
SWAP_2_3 = bytes.fromhex("022803d0032802d1022000e00320")
# setlist_view (0x44) row 7 shows the level's name as its subtitle: r3 = view + 0x1AC + level * 8, an array of the 3
# scene texts {char *, u32}; level 3 read past it and showed nothing. The two adds become `bl` to a hook + nop.w
SETLIST_SUBTITLE = 0x100B54B2  # add.w r3, r4, r3, lsl #3
SETLIST_SUBTITLE_ADD = 0x100B54B8  # add.w r3, r3, #0x1ac
STOCK_SUBTITLE = bytes.fromhex("04ebc303"), bytes.fromhex("03f5d673")
NOP_W = bytes.fromhex("aff30080")


def sum32(data: bytes) -> int:
    data = bytes(data) + bytes(-len(data) % 4)
    return sum(struct.unpack(f"<{len(data) // 4}I", data)) & 0xFFFFFFFF


def fix_acth(app: bytearray, size: int | None = None) -> None:
    if len(app) % 4:
        raise ValueError("app length must be a multiple of 4")
    size = len(app) - 0x200 if size is None else size
    struct.pack_into("<I", app, 0x18, size)
    struct.pack_into("<I", app, 0x1C, 0)
    struct.pack_into("<I", app, 0x20, 0)
    struct.pack_into("<I", app, 0x1C, (0xFFFFFFFF - sum32(app[0x200:0x200 + size])) & 0xFFFFFFFF)
    struct.pack_into("<I", app, 0x20, (0xFFFFFFFF - sum32(app[:0x200])) & 0xFFFFFFFF)


def check_acth(app: bytes, max_len: int = NOR_APP_SIZE) -> None:
    # app_image_check() of the Actions SDK bootloader (header at 0, vectors at 0x200)
    size, checksum = struct.unpack_from("<II", app, 0x18)
    hdr_size, flags = struct.unpack_from("<H", app, 0x24)[0], struct.unpack_from("<I", app, 0x2C)[0]
    reset = struct.unpack_from("<I", app, 0x204)[0]
    if (app[:8] != b"ACTHHTCA" or hdr_size != 0x200 or flags or not reset & 1
            or not APP_VBASE <= reset <= APP_VBASE + max_len or not 0 < size <= len(app) - 0x200
            or size % 4 or len(app) > max_len or sum32(app[:0x200]) != 0xFFFFFFFF
            or (sum32(app[0x200:0x200 + size]) + checksum) & 0xFFFFFFFF != 0xFFFFFFFF):
        raise ValueError("ACTH header fails the bootloader image check")


def _thumb_branch(at: int, target: int, second: int) -> bytes:
    off = target - (at + 4)
    if off % 2 or not -(1 << 24) <= off < 1 << 24:
        raise ValueError(f"branch {at:#x} -> {target:#x} is out of range")
    s = (off >> 24) & 1
    i1, i2 = (off >> 23) & 1, (off >> 22) & 1
    j1, j2 = (i1 ^ 1) ^ s, (i2 ^ 1) ^ s
    hw1 = 0xF000 | s << 10 | (off >> 12) & 0x3FF
    hw2 = second | j1 << 13 | j2 << 11 | (off >> 1) & 0x7FF
    return struct.pack("<HH", hw1, hw2)


def thumb_bl(at: int, target: int) -> bytes:
    return _thumb_branch(at, target, 0xD000)


def thumb_b_w(at: int, target: int) -> bytes:
    return _thumb_branch(at, target, 0x9000)


def put(app: bytearray, addr: int, old: bytes, new: bytes) -> None:
    off = addr - APP_VBASE
    if bytes(app[off:off + len(old)]) != old:
        raise ValueError(f"unexpected bytes at {addr:#x}")
    app[off:off + len(new)] = new


def motor_q10(permille: int) -> int:
    if not 0 < permille < 1000:
        raise ValueError(f"motor strength {permille} is not in 1..999 permille")
    return (permille * 1024 + 500) // 1000


def motor_hook(permille: int) -> bytes:
    """Replaces `bl STOCK_MOTOR` (r3 = pulse = stock strength, 0 = off): r3 = r3 * q10 >> 10, then stock."""
    k = motor_q10(permille)
    movw_r12 = struct.pack("<HH", 0xF240 | (k >> 11 & 1) << 10 | k >> 12, (k >> 8 & 7) << 12 | 12 << 8 | k & 0xFF)
    mul_r3_r3_r12 = bytes.fromhex("03fb0cf3")
    lsrs_r3_10 = bytes.fromhex("9b0a")
    code = movw_r12 + mul_r3_r3_r12 + lsrs_r3_10
    return code + thumb_b_w(MOTOR_HOOK + len(code), STOCK_MOTOR)


def weak_row(out: bytearray, permille: int) -> int:
    """A fourth vibration level, shown before Off, at `permille` of full strength. Returns the end of the added data."""
    if not 0 < permille < 1000:
        raise ValueError("weak row strength is 1..999 permille")
    fmt = b"%s\n%s\n" + WEAK_LABEL.encode() + b"\n%s\0"
    put(out, WEAK_TABLE, bytes(16 + len(fmt)), struct.pack("<4I", 255, 250, 0, 255 * permille // 1000) + fmt)
    for lit in LEVELS_LITERALS:
        put(out, lit, struct.pack("<I", LEVELS), struct.pack("<I", WEAK_TABLE))
    for at in LEVELS_BOUNDS:
        put(out, at, bytes.fromhex("022b"), bytes.fromhex("032b"))
    for lit in ROLLER_FORMAT_LITERALS:
        put(out, lit, struct.pack("<I", ROLLER_FORMAT), struct.pack("<I", WEAK_FORMAT))
    put(out, CROWN_MAX, bytes.fromhex("0129"), bytes.fromhex("0229"))
    at = (WEAK_FORMAT + len(fmt) + 3) & ~3
    preview, at = at, _swap_hook(out, at, STOCK_PREVIEW)
    set_level, at = at, _swap_hook(out, at, STOCK_SET_LEVEL)
    get_level = at
    code = b"\x08\xb5" + thumb_bl(at + 2, STOCK_GET_LEVEL) + SWAP_2_3 + b"\x08\xbd"  # push {r3,lr} .. pop {r3,pc}
    put(out, at, bytes(len(code)), code)
    for call, kind in PREVIEW_CALLS:
        enc = thumb_bl if kind == "bl" else thumb_b_w
        put(out, call, enc(call, STOCK_PREVIEW), enc(call, preview))
    for call in SET_LEVEL_CALLS:
        put(out, call, thumb_bl(call, STOCK_SET_LEVEL), thumb_bl(call, set_level))
    for call in GET_LEVEL_CALLS:
        put(out, call, thumb_bl(call, STOCK_GET_LEVEL), thumb_bl(call, get_level))
    at = (at + len(code) + 3) & ~3
    end = _subtitle_hook(out, at)
    if end > MOTOR_HOOK:
        raise ValueError("weak row code runs into the motor hook")
    return end


def _subtitle_hook(out: bytearray, at: int) -> int:
    """cmp r3,#3; <stock adds>; it eq; adreq r3, text; bx lr; text = {"Weak", 0}. Returns the end."""
    code = b"\x03\x2b" + b"".join(STOCK_SUBTITLE) + b"\x08\xbf\x00\xa3\x70\x47"  # adr: Align(PC, 4) = at + 16
    text = at + len(code)
    data = code + struct.pack("<II", text + 8, 0) + WEAK_LABEL.encode() + b"\0"
    put(out, at, bytes(len(data)), data)
    put(out, SETLIST_SUBTITLE, STOCK_SUBTITLE[0], thumb_bl(SETLIST_SUBTITLE, at))
    put(out, SETLIST_SUBTITLE_ADD, STOCK_SUBTITLE[1], NOP_W)
    return at + len(data)


def _swap_hook(out: bytearray, at: int, target: int) -> int:
    """SWAP_2_3 on r0, then tail-call target. Returns the next 4-aligned address."""
    code = SWAP_2_3 + thumb_b_w(at + len(SWAP_2_3), target)
    put(out, at, bytes(len(code)), code)
    return (at + len(code) + 3) & ~3


def patch(app: bytes, stub: bytes | None = None, *, motor: int | None = None, reserve: bool = True,
          weak: int | None = None, label: bool = False, _verify: bool = True) -> bytes:
    """motor: stock vibrations at this many permille of their strength; reserve: the loader's heap reservation;
    weak: a fourth vibration level at this many permille; label: "<version>-fmc" on the About page (add_fmc_label)."""
    if _verify and not hashlib.sha256(app).hexdigest().startswith(STOCK_SHA256_PREFIX):
        raise ValueError("not the stock JX402 1.0.0.73 app.bin (sha256 mismatch)")
    if stub is not None and not reserve:
        raise ValueError("the stub needs the heap reservation")
    out = bytearray(app)
    size = struct.unpack_from("<I", out, 0x18)[0]
    if reserve:
        for off in HEAP_LITERALS:
            put(out, APP_VBASE + off, struct.pack("<I", HEAP_OLD), struct.pack("<I", HEAP_NEW))
    if motor is not None:
        put(out, MOTOR_HOOK, bytes(MOTOR_HOOK_SIZE), motor_hook(motor))
        put(out, MOTOR_CALL, thumb_bl(MOTOR_CALL, STOCK_MOTOR), thumb_bl(MOTOR_CALL, MOTOR_HOOK))
        size = max(size, MOTOR_HOOK + MOTOR_HOOK_SIZE + 2 - APP_VBASE - 0x200)
    if weak is not None:
        end = weak_row(out, weak)
        size = max(size, (end - APP_VBASE - 0x200 + 3) & ~3)
    if stub is not None:
        add_stub(out, stub)
    out += bytes(-len(out) % 4)
    if label:
        add_fmc_label(out, app)
    # stock size stops before the appended ksdfs tail; appended code must be covered
    fix_acth(out, None if stub is not None or label else size)
    check_acth(out)
    return bytes(out)


def add_stub(out: bytearray, stub: bytes) -> None:
    version = struct.unpack_from("<I", stub, 4)[0]
    header = STUB_HEADER_V2 if version == 2 else STUB_HEADER
    magic, version, boot, ble, view_proc, view_name, applist, widget, *guard = header.unpack_from(stub)
    base = APP_VBASE + STUB_OFFSET
    code = (boot, view_proc) + tuple(p for p in (ble, applist) if p) + tuple(guard)
    if (magic != b"WP2S" or version not in (1, 2) or not all(p & 1 for p in code)
            or not all(base <= p & ~1 < base + len(stub) for p in (*code, view_name))):
        raise ValueError("bad stub header")
    if widget:
        if widget % 4 or not base <= widget <= base + len(stub) - WGT_ENTRY.size:
            raise ValueError("bad stub widget entry")
        entry = WGT_ENTRY.unpack_from(stub, widget - base)
        wid, callbacks = entry[0], entry[5:]
        if wid != WGT_ID or not all(p & 1 and base <= p & ~1 < base + len(stub) for p in callbacks):
            raise ValueError("bad stub widget entry")
    if len(out) != STUB_OFFSET:
        raise ValueError(f"app.bin is {len(out):#x} bytes; the stub goes at {STUB_OFFSET:#x}")
    out += stub
    out += bytes(-len(out) % 8)
    table = APP_VBASE + len(out)
    out += out[VIEW_TABLE - APP_VBASE:VIEW_TABLE_END - APP_VBASE]
    for view_id in (VIEW_ID, *PAGE_VIEWS):
        out += struct.pack("<IIIIHHHH", view_name, view_proc, 0, 0, view_id, 0x102, 466, 466)
    if len(out) - STUB_OFFSET > STUB_MAX:
        raise ValueError(f"stub and view table take {len(out) - STUB_OFFSET:#x} bytes; the limit is {STUB_MAX:#x}")
    for lit in VIEW_START_LITERALS:
        put(out, lit, struct.pack("<I", VIEW_TABLE), struct.pack("<I", table))
    for lit in VIEW_END_LITERALS:
        put(out, lit, struct.pack("<I", VIEW_TABLE_END), struct.pack("<I", APP_VBASE + len(out)))
    put(out, BOOT_CALL, thumb_bl(BOOT_CALL, STOCK_HEAP_INIT), thumb_bl(BOOT_CALL, boot & ~1))
    if ble:
        put(out, BLE_CALL, thumb_bl(BLE_CALL, STOCK_BLE_READ), thumb_bl(BLE_CALL, ble & ~1))
    if applist:
        put(out, APPLIST_LAYOUT, struct.pack("<I", STOCK_APPLIST_LAYOUT), struct.pack("<I", applist))
    if widget:
        put(out, WGT_REG + 4 * WGT_ID, bytes(4), struct.pack("<I", widget))
    if guard:
        put_guard(out, GUARD_WP2, guard)


def put_guard(out: bytearray, sites: tuple, hooks: list[int]) -> None:
    for at, stock, hook in sites:
        put(out, at, thumb_bl(at, stock), thumb_bl(at, hooks[hook] & ~1))


def patch_wp3(app: bytes, stub: bytes, *, label: bool = False) -> bytes:
    """Watch 3 Pro: appends fw/wp3-stub and points the GUARD_WP3 calls at it; a version 2 stub also gets the
    WP3_APPLIST slots and a view-table record for WP3_VIEW_ID (add_stub_wp3). label: add_fmc_label after the stub."""
    if not hashlib.sha256(app).hexdigest().startswith(WP3_STOCK_SHA256_PREFIX):
        raise ValueError("not the stock jx402_08 1.0.1.77 app.bin (sha256 mismatch)")
    out = bytearray(app)
    add_stub_wp3(out, stub)
    if label:
        add_fmc_label(out, app)
    fix_acth(out)
    check_acth(out)
    return bytes(out)


def add_stub_wp3(out: bytearray, stub: bytes) -> None:
    """Version 1: the guard only. Version 2: also the app-list update slots and the stub's view, whose record goes
    into a copy of the view table after the stub (the stock literals are repointed at the copy, as on WP2).
    Version 3: also the boot hook with the ResFbHeap shrunk by StubRam, and the BLE default-branch hook."""
    if len(stub) < WP3_STUB_HEADER.size:
        raise ValueError("bad stub header")
    magic, version = struct.unpack_from("<4sI", stub)
    if magic != b"WP3S" or version not in WP3_HEADERS or len(stub) < WP3_HEADERS[version].size:
        raise ValueError("bad stub header")
    fields = WP3_HEADERS[version].unpack_from(stub)[2:]
    if version == 1:
        code, view_name, hooks = fields, None, ()
    else:
        code, view_name, hooks = fields[:8], fields[8], fields[9:]
        code += hooks
    base = APP_VBASE + WP3_STUB_OFFSET
    if not all(p & 1 and base <= p & ~1 < base + len(stub) for p in code):
        raise ValueError("bad stub header")
    if view_name is not None and not base <= view_name < base + len(stub):
        raise ValueError("bad stub header")
    if len(stub) > WP3_STUB_MAX:
        raise ValueError(f"stub takes {len(stub):#x} bytes; the limit is {WP3_STUB_MAX:#x}")
    if len(out) != WP3_STUB_OFFSET:
        raise ValueError(f"app.bin is {len(out):#x} bytes; the stub goes at {WP3_STUB_OFFSET:#x}")
    guard = code[:5]
    out += stub
    if version == 1:
        out += bytes(-len(out) % 4)
        put_guard(out, GUARD_WP3, guard)
        return
    applist, view_proc = code[5:7], code[7]
    out += bytes(-len(out) % 8)
    table = APP_VBASE + len(out)
    out += out[WP3_VIEW_TABLE - APP_VBASE:WP3_VIEW_TABLE_END - APP_VBASE]
    out += WP3_VIEW_RECORD.pack(view_name, view_proc, 0, WP3_VIEW_ID, 0x102, 466, 466)
    if len(out) - WP3_STUB_OFFSET > WP3_STUB_MAX:
        raise ValueError(f"stub and view table take {len(out) - WP3_STUB_OFFSET:#x} bytes; "
                         f"the limit is {WP3_STUB_MAX:#x}")
    for lit in WP3_VIEW_START_LITERALS:
        put(out, lit, struct.pack("<I", WP3_VIEW_TABLE), struct.pack("<I", table))
    for lit in WP3_VIEW_END_LITERALS:
        put(out, lit, struct.pack("<I", WP3_VIEW_TABLE_END), struct.pack("<I", APP_VBASE + len(out)))
    for (slot, stock), hook in zip(WP3_APPLIST, applist):
        put(out, slot, struct.pack("<I", stock), struct.pack("<I", hook))
    put_guard(out, GUARD_WP3, guard)
    if hooks:
        boot, ble = hooks
        for lit in WP3_HEAP_LITERALS:
            put(out, lit, struct.pack("<I", WP3_HEAP_OLD), struct.pack("<I", WP3_HEAP_NEW))
        put(out, WP3_BOOT_CALL, thumb_bl(WP3_BOOT_CALL, WP3_STOCK_HEAP_INIT), thumb_bl(WP3_BOOT_CALL, boot & ~1))
        put(out, WP3_BLE_CALL, thumb_bl(WP3_BLE_CALL, WP3_STOCK_BLE_READ), thumb_bl(WP3_BLE_CALL, ble & ~1))


def add_fmc_label(out: bytearray, stock: bytes) -> str:
    """Settings > About shows "<version>-fmc" (fmclabel, docs/recon.md "About page label"); every machine-readable
    version (AT GETVERSION, the BLE replies, the OTA version code) stays stock. Sites are located in the `stock`
    image; the new bytes are appended to `out` (4-aligned), so call it after the stub and fix the ACTH after it.
    WP3 shape: the page's format pool word points at "%s-fmc" + the rest of the stock format (data only). Otherwise
    the page's `add.w rD, sp, #..` becomes `bl thunk`, thunk = `ldr rD, [pc, #0]; bx lr; .word label`: the page
    already called the info getter, so its lr is saved. Returns the label."""
    sites = fmclabel.locate(stock)
    label = sites.label
    out += bytes(-len(out) % 4)
    at = APP_VBASE + len(out)
    if sites.fmt_pool_off is not None:
        word = sites.fmt_pool_off
        old = struct.unpack_from("<I", stock, word)[0]
        put(out, APP_VBASE + word, struct.pack("<I", old), struct.pack("<I", at))
        out += sites.fmt.replace(b"%s", b"%s" + fmclabel.SUFFIX.encode(), 1) + b"\0"
    else:
        if sites.page_rd > 7:
            raise ValueError(f"About page site uses r{sites.page_rd}; the thunk needs r0..r7")
        add = sites.page_add
        old = stock[add - APP_VBASE:add - APP_VBASE + 4]
        out += struct.pack("<HHI", 0x4800 | sites.page_rd << 8, 0x4770, at + 8) + label.encode() + b"\0"
        put(out, add, old, thumb_bl(add, at))
    out += bytes(-len(out) % 4)
    return label


def changed_ranges(old: bytes, new: bytes, gap: int = 16) -> list[tuple[int, int]]:
    """[start, end) offsets where new differs from old; runs closer than `gap` bytes are merged."""
    if len(old) != len(new):
        raise ValueError("ranges need equal lengths")
    out: list[list[int]] = []
    for i in (i for i in range(len(old)) if old[i] != new[i]):  # ponytail: byte scan, 2.5 MB takes ~0.3 s
        if out and i - out[-1][1] < gap:
            out[-1][1] = i + 1
        else:
            out.append([i, i + 1])
    return [(a, b) for a, b in out]


def stage_nor(nor: bytes, app: bytes) -> bytes:
    if len(app) > NOR_APP_SIZE:
        raise ValueError(f"app is {len(app)} bytes; the partition holds {NOR_APP_SIZE}")
    out = bytearray(nor)
    out[NOR_APP_OFFSET:NOR_APP_OFFSET + NOR_APP_SIZE] = app + b"\xff" * (NOR_APP_SIZE - len(app))
    return bytes(out)


def xz_chunks(data: bytes, chunk: int = 0x200000) -> bytes:
    """AOTA partition blob: per chunk b"LZMA", u32 16, u32 packed size, u32 size, then an XZ stream (stock: preset 6)."""
    out = bytearray()
    for i in range(0, len(data), chunk):
        piece = lzma.compress(data[i:i + chunk], format=lzma.FORMAT_XZ, preset=6)
        out += b"LZMA" + struct.pack("<III", 16, len(piece), len(data[i:i + chunk])) + piece
    return bytes(out)


def ota_xml_set(xml: str, file_name: str | None = None, **fields: int) -> str:
    """Sets `<tag>0x..</tag>` fields of one partition (by file_name) or, with no file_name, of the whole manifest."""
    if file_name is not None:
        blocks = [m for m in re.finditer(r"<partition>.*?</partition>", xml, re.S)
                  if f"<file_name>{file_name}</file_name>" in m.group()]
        if len(blocks) != 1:
            raise ValueError(f"ota.xml has {len(blocks)} partition(s) {file_name}")
        a, b = blocks[0].span()
        return xml[:a] + ota_xml_set(xml[a:b], **fields) + xml[b:]
    for tag, value in fields.items():
        xml, n = re.subn(rf"<{tag}>[^<]*</{tag}>", f"<{tag}>{value:#x}</{tag}>", xml)
        if n != 1:
            raise ValueError(f"ota.xml has {n} <{tag}>")
    return xml


def fix_aota_crc(image: bytes) -> bytes:
    """AOTA header: data CRC32 at +0x18 over [u16 +0x12, u32 +0x14), header CRC32 at +0x04 over +0x08..0x400.
    `fwkit pack` and `edit-app` leave the data CRC stale; the stock OTA client in the emulator checks it."""
    out = bytearray(image)
    start, end = struct.unpack_from("<H", out, 0x12)[0], struct.unpack_from("<I", out, 0x14)[0]
    if not (out[:4] == b"AOTA" and 0x400 <= start <= end <= len(out)):
        raise ValueError("not an AOTA image")
    struct.pack_into("<I", out, 0x18, zlib.crc32(out[start:end]))
    struct.pack_into("<I", out, 0x04, zlib.crc32(out[0x08:0x400]))
    return bytes(out)


def ota_xml_only(xml: str, file_name: str) -> str:
    """The manifest with every partition but `file_name` dropped."""
    blocks = list(re.finditer(r"\s*<partition>.*?</partition>", xml, re.S))
    keep = [m for m in blocks if f"<file_name>{file_name}</file_name>" in m.group()]
    if len(keep) != 1:
        raise ValueError(f"ota.xml has {len(keep)} partition(s) {file_name}")
    xml = xml[:blocks[0].start()] + keep[0].group() + xml[blocks[-1].end():]
    return re.sub(r"<partitionsNum>\d+</partitionsNum>", "<partitionsNum>1</partitionsNum>", xml)
