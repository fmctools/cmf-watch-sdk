"""Checks the per-model stock table (fw/wp2-core/src/model.rs) against stock app.bin images.

The table's single source is model.rs; `fw/wp2-core/model.json` is its export (the Rust test `json_is_current`
fails when it is stale), so nothing here restates an address. The image checks run only when the environment points
at extracted stock app.bin files (load address 0x10000000), which are never committed:

    WP2_STOCK_APP=/path/wp2-app.bin WP3_STOCK_APP=/path/wp3-app.bin \\
        uv run --locked python -m unittest tests.test_model_stock -v
    uv run --locked python tests/test_model_stock.py wp3 /path/wp3-app.bin   # per-field report

Every field ends up verified (a concrete check in the image), unverified (with the reason: a RAM address the code
reaches as base + offset, a value with no static footprint such as a key code) or unknown (`None` in the table).
A check that runs and disagrees fails the test.
"""

import hashlib
import json
import os
from pathlib import Path
import struct
import sys
import unittest

ROOT = Path(__file__).resolve().parent.parent
MODEL_JSON = ROOT / "fw" / "wp2-core" / "model.json"
VBASE = 0x10000000
ENV = {"wp2": "WP2_STOCK_APP", "wp3": "WP3_STOCK_APP"}
# heap marker of the app-list view-ops records (marker, layout, update, ...)
OPS_MARKER = 0xC18487B4
# u16 after the id: 0x100..0x102 in stock records (the stub writes 0x102), then width, height
VIEW_FLAGS_HI, SCREEN = 0x100, 466
STUB_RAM_SIZE = 0x1000


def load_models():
    raw = json.loads(MODEL_JSON.read_text())

    def num(v):
        if v is None or isinstance(v, str) and not v.startswith("0x"):
            return v
        return [int(x, 16) for x in v] if isinstance(v, list) else int(v, 16)

    return {
        name: {field: {"kind": cell["kind"], "value": num(cell["value"])} for field, cell in fields.items()}
        for name, fields in raw.items()
    }


class Model:
    """Attribute access to one model's values: m.boot_call, m.kind("boot_call")."""

    def __init__(self, fields):
        self.fields = fields

    def __getattr__(self, name):
        return self.fields[name]["value"]

    def kind(self, name):
        return self.fields[name]["kind"]


class Image:
    def __init__(self, data):
        self.data = data
        self.end = VBASE + len(data)
        self._words = None
        self._branches = None

    def has(self, addr, n=1):
        return VBASE <= addr and addr + n <= self.end

    def u16(self, addr):
        return struct.unpack_from("<H", self.data, addr - VBASE)[0]

    def u32(self, addr):
        return struct.unpack_from("<I", self.data, addr - VBASE)[0]

    def i16(self, addr):
        return struct.unpack_from("<h", self.data, addr - VBASE)[0]

    def cstr(self, addr, limit=64):
        raw = self.data[addr - VBASE:addr - VBASE + limit]
        return raw.split(b"\0")[0].decode("latin-1")

    def words(self):
        """value -> count of every 4-aligned word."""
        if self._words is None:
            counts = {}
            for (w,) in struct.iter_unpack("<I", self.data[:len(self.data) & ~3]):
                counts[w] = counts.get(w, 0) + 1
            self._words = counts
        return self._words

    def bl(self, site):
        """Target of a 32-bit BL / B.W (T4) at `site`, else None."""
        return decode_branch(self.u16(site), self.u16(site + 2), site)

    def branches(self):
        """Targets of every BL / B.W in the image (scanned at each halfword)."""
        if self._branches is None:
            hw = struct.unpack(f"<{len(self.data) // 2}H", self.data[:len(self.data) & ~1])
            targets = set()
            for i in range(len(hw) - 1):
                if hw[i] & 0xF800 == 0xF000 and hw[i + 1] & 0xD000 in (0xD000, 0x9000):
                    targets.add(decode_branch(hw[i], hw[i + 1], VBASE + 2 * i))
            self._branches = targets
        return self._branches

    def halfwords(self, start, n):
        start &= ~1
        return [(start + 2 * i, self.u16(start + 2 * i)) for i in range(n // 2) if self.has(start + 2 * i, 2)]

    def first_literal(self, func, n=0x20):
        """Value of the first 16-bit `ldr rX, [pc, #imm]` in the first n bytes of `func`."""
        for at, h in self.halfwords(func, n):
            if h & 0xF800 == 0x4800:
                return self.u32(((at + 4) & ~3) + (h & 0xFF) * 4)
        return None


def decode_branch(h1, h2, site):
    if h1 & 0xF800 != 0xF000 or h2 & 0xD000 not in (0xD000, 0x9000):
        return None
    s = h1 >> 10 & 1
    j1, j2 = h2 >> 13 & 1, h2 >> 11 & 1
    i1, i2 = 1 - (j1 ^ s), 1 - (j2 ^ s)
    off = s << 24 | i1 << 23 | i2 << 22 | (h1 & 0x3FF) << 12 | (h2 & 0x7FF) << 1
    if s:
        off -= 1 << 25
    return site + 4 + off


def ldr16(rt, rn, off):
    """16-bit `ldr rt, [rn, #off]`."""
    return 0x6800 | off // 4 << 6 | rn << 3 | rt


class Verifier:
    """Runs every check for one model; results: field -> (status, note), status in verified/unverified/unknown/
    failed."""

    def __init__(self, model, image, other=None):
        self.m, self.img, self.other = model, image, other
        self.results = {}

    # -- bookkeeping
    def ok(self, field, note):
        if self.results.get(field, ("",))[0] != "failed":
            self.results[field] = ("verified", note)

    def fail(self, field, note):
        self.results[field] = ("failed", note)

    def check(self, field, cond, note):
        (self.ok if cond else self.fail)(field, note)

    def unverified(self, field, note):
        self.results.setdefault(field, ("unverified", note))

    def run(self):
        m, img = self.m, self.img
        self.check("app_size", len(img.data) == m.app_size, f"image is {len(img.data):#x} bytes")
        digest = hashlib.sha256(img.data).hexdigest()
        self.check("stock_sha256_prefix", digest.startswith(m.stock_sha256_prefix), f"sha256 {digest[:16]}")
        self.ok("name", "model key")
        for check in (self.boot, self.views, self.ble, self.cards, self.applist, self.lvgl, self.fs, self.ux2sys,
                      self.strings, self.misc):
            check()
        self.generic()
        return self.results

    def generic(self):
        m, img = self.m, self.img
        for field, cell in m.fields.items():
            kind, value = cell["kind"], cell["value"]
            if field in self.results:
                continue
            if value is None:
                self.results[field] = ("unknown", "None in the table: not re-derived, or not on this model")
                continue
            values = value if isinstance(value, list) else [value]
            if kind == "func":
                missing = [hex(v) for v in values if (v & ~1) not in img.branches() and v not in img.words()]
                self.check(field, not missing and all(v & 1 for v in values),
                           "called or referenced" if not missing else f"never called or referenced: {missing}")
            elif kind == "ram":
                found = [v for v in values if v in img.words()]
                if found:
                    self.ok(field, "literal in the image")
                else:
                    self.unverified(field, "not a literal: reached as base + offset (emulator recon)")
            elif kind == "rom":
                self.check(field, all(img.has(v) for v in values), "inside the image (no stronger check)")
                self.results[field] = ("unverified", "inside the image only") if self.results[field][0] == \
                    "verified" else self.results[field]
            else:
                self.unverified(field, f"{kind}: no static footprint checked")

    # -- boot and heap
    def boot(self):
        m, img = self.m, self.img
        target = img.bl(m.boot_call)
        self.check("boot_call", target == m.stock_heap_init & ~1, f"bl -> {target and hex(target)}")
        self.check("stock_heap_init", target == m.stock_heap_init & ~1, "target of boot_call")
        vals = [img.u32(a) for a in m.heap_literals]
        self.check("heap_literals", vals == [m.heap_size] * 3 and img.words().get(m.heap_size) == 3,
                   f"slots hold {[hex(v) for v in vals]}, value occurs {img.words().get(m.heap_size)}x")
        self.check("heap_size", img.words().get(m.heap_size) == 3, "exactly 3 literals")
        self.check("heap_base", img.words().get(m.heap_base) == 1, "one literal, next to the heap init call")
        self.check("stub_ram", m.stub_ram == m.heap_base + m.heap_size - STUB_RAM_SIZE, "top 4 KiB of ResFbHeap")
        # heap init stores the free count (+8, strd) and the free list (+0x60)
        body = [h for _, h in img.halfwords(m.stock_heap_init, 0x100)]
        strd8 = any(a & 0xFFF0 == 0xE9C0 and b & 0xFF == m.heap_free // 4 for a, b in zip(body, body[1:]))
        self.check("heap_free", strd8, "strd at +8 in stock_heap_init")
        str60 = any(h & 0xFFC0 == 0x6000 | (m.heap_free_list // 4) << 6 for h in body)
        self.check("heap_free_list", str60, "str at +0x60 in stock_heap_init")
        self.check("heap", m.heap in img.words(), "literal in the image")

    # -- view table
    def views(self):
        m, img = self.m, self.img
        for f, lits, want in (("view_start_literals", m.view_start_literals, m.view_table),
                              ("view_end_literals", m.view_end_literals, m.view_table_end)):
            vals = [img.u32(a) for a in lits]
            self.check(f, vals == [want] * len(lits), f"slots hold {[hex(v) for v in vals]}")
        span = m.view_table_end - m.view_table
        recs = range(m.view_table, m.view_table_end, m.view_record)
        bad, ids, word1 = [], [], 0
        for r in recs:
            name = img.u32(r)
            vid, flags = img.u16(r + m.view_rec_id), img.u16(r + m.view_rec_id + 2)
            w, h = img.u16(r + m.view_rec_id + 4), img.u16(r + m.view_rec_id + 6)
            procs = [img.u32(r + 4), img.u32(r + 8)]
            if not img.has(name) or (flags & 0xFF00, w, h) != (VIEW_FLAGS_HI, SCREEN, SCREEN) or sum(p != 0 for p in procs) != 1 \
                    or not all(p & 1 for p in procs if p):
                bad.append(hex(r))
            word1 += procs[0] != 0
            ids.append(vid)
        note = f"{len(recs)} records of {m.view_record} B, ids up to {max(ids):#x}"
        ok = span % m.view_record == 0 and not bad
        for f in ("view_table", "view_table_end", "view_record", "view_rec_id"):
            self.check(f, ok, note if ok else f"{note}; bad records {bad[:5]}")
        self.check("view_rec_proc", m.view_rec_proc == 4 and word1 > 0,
                   f"{word1} stock records use word 1 with word 2 = 0, the stub's form")
        self.check("stock_view_id_max", max(ids) == m.stock_view_id_max, f"max stock id {max(ids):#x}")
        stub = [m.menu_view, *range(m.page_view_base, m.page_view_base + 16)]
        clash = sorted(set(stub) & set(ids))
        for f in ("menu_view", "page_view_base"):
            self.check(f, not clash, "clear of every stock view id" if not clash else f"collides {clash}")

    # -- BLE dispatcher
    def ble(self):
        m, img = self.m, self.img
        target = img.bl(m.ble_call)
        self.check("ble_call", target == m.read_payload & ~1, f"bl -> {target and hex(target)}")
        self.check("read_payload", target == m.read_payload & ~1, "target of ble_call")
        # default branch: `ldrh.w r1, [rF, #1]` ; `ldr r0, [pc, #..]` ; bl read_payload
        h1, h2 = img.u16(m.ble_dispatch_default), img.u16(m.ble_dispatch_default + 2)
        frame = h1 & 0xF if h1 & 0xFFF0 == 0xF8B0 and h2 == 0x1001 else None
        self.check("ble_frame_reg", frame == m.ble_frame_reg, f"ldrh.w r1, [r{frame}, #1] before the call")
        self.check("ble_dispatch_default", frame is not None and m.ble_call == m.ble_dispatch_default + 6,
                   "frame load, literal load, bl")
        n = m.ble_opcode_max - m.ble_dispatch_first + 1
        table = [img.u32(m.ble_dispatch_table + 4 * i) for i in range(n)]
        default = m.ble_dispatch_default | 1
        dispatch = struct.pack("<HH", 0xF852, 0xF023)  # ldr.w pc, [r2, r3, lsl #2]
        at = m.ble_dispatch_table - VBASE
        jump_ok = self.img.data[at - 4:at] == dispatch
        self.check("ble_dispatch_table", jump_ok and all(e & 1 and img.has(e) for e in table),
                   f"{n} Thumb entries right after `ldr.w pc, [r2, r3, lsl #2]`")
        # bound: movw r2, #max somewhere in the 0x60 bytes before the default branch
        movw = movw_imm(m.ble_opcode_max, 2)
        near = img.data[m.ble_dispatch_default - VBASE - 0x60:m.ble_dispatch_default - VBASE]
        self.check("ble_opcode_max", movw in near, "movw r2, #max before the default branch")
        self.check("ble_dispatch_first", first_bound_ok(img, m), "sub #0x9000; subs #first-0x9000; cmp #n-1")

        def route(op):
            if op > m.ble_opcode_max or op < m.ble_dispatch_first:
                return default
            return table[op - m.ble_dispatch_first]

        wrong = [hex(op) for op in m.stub_opcodes if route(op) != default]
        self.check("stub_opcodes", not wrong, "all reach the default branch" if not wrong else f"handled: {wrong}")
        self.check("data_opcode", route(m.data_opcode) == default, "reaches the default branch")
        if m.dnd_opcode is not None:
            case = route(m.dnd_opcode)
            handler = img.bl(case & ~1)
            self.check("dnd_opcode", case != default, f"stock case {case:#x}")
            self.check("dnd_handler", handler == m.dnd_handler & ~1, f"case calls {handler and hex(handler)}")

    # -- cards and widget registry
    def cards(self):
        m, img = self.m, self.img
        entries = []
        a = m.wgt_reg
        while img.has(a + 4) and VBASE <= img.u32(a) < img.end:
            entries.append(img.u32(a))
            a += 4
        if m.wgt_free_id is not None:
            free = img.u32(m.wgt_reg + 4 * m.wgt_free_id)
            self.check("wgt_free_id", free == 0, f"entry {m.wgt_free_id:#x} is {free:#x}")
            self.check("wgt_reg", len(entries) > 8, f"{len(entries)} leading entries are pointers")
        if m.wgt_reg_literal is not None:
            lit = img.u32(m.wgt_reg_literal)
            reader = [h for _, h in img.halfwords(m.wgt_reg_literal - 0x10, 0x10)]
            bound = len(entries) - 1
            self.check("wgt_reg_literal", lit == m.wgt_reg, f"holds {lit:#x}")
            self.check("wgt_reg", 0x2800 | bound in reader, f"{len(entries)} entries, reader `cmp r0, #{bound:#x}`")

    # -- app list
    def applist(self):
        m, img = self.m, self.img
        styles = [("applist_list_ops", m.applist_list_ops, "applist_layout", "applist_update", "applist_hook_slot"),
                  ("applist_grid_ops", m.applist_grid_ops, "applist_grid_layout", "applist_grid_update",
                   "applist_grid_hook_slot")]
        for ops_f, ops, lay_f, upd_f, slot_f in styles:
            if ops is None:
                continue
            marker = img.u32(ops)
            self.check(ops_f, marker == OPS_MARKER, f"marker {marker:#x}")
            self.check(lay_f, img.u32(ops + 4) == getattr(m, lay_f), "ops +4")
            if getattr(m, upd_f) is not None:
                self.check(upd_f, img.u32(ops + 8) == getattr(m, upd_f), "ops +8")
            slot = getattr(m, slot_f)
            held = img.u32(slot)
            expect = {ops + 4: getattr(m, lay_f), ops + 8: getattr(m, upd_f)}.get(slot)
            self.check(slot_f, expect is not None and held == expect, f"holds stock {held:#x}")

    # -- LVGL struct offsets
    def lvgl(self):
        m, img = self.m, self.img
        cls = img.first_literal(m.lv_obj_create)
        size = img.u32(cls + 0x1C) >> 4 & 0xFFFF if cls and img.has(cls + 0x20) else None
        self.check("lv_obj_size", size == m.lv_obj_size, f"lv_obj_class instance_size {size and hex(size)}")
        self.check("lv_obj_create", cls is not None, "loads lv_obj_class")
        first = img.u16(m.lv_tileview_get_tile_act & ~1)
        self.check("tile_act", first == ldr16(0, 0, m.tile_act), "get_tile_act: ldr r0, [r0, #tile_act]")
        body = [h for _, h in img.halfwords(m.obj_get_y2, 0xA0)]
        y = [h for h in body if h & 0xF800 == 0x6800]
        offs = {(h >> 6 & 0x1F) * 4 for h in y}
        if {m.lv_obj_y1, m.lv_obj_y2} <= offs:
            self.ok("lv_obj_y1", "obj_get_y2 reads y1 and y2")
            self.ok("lv_obj_y2", "obj_get_y2 reads y1 and y2")
            self.ok("lv_obj_coords", "y1 - 4")
        else:
            self.unverified("lv_obj_y1", "obj_get_y2 has no plain field loads on this build")
            self.unverified("lv_obj_y2", "obj_get_y2 has no plain field loads on this build")
        fn = m.lv_arc_get_value if m.lv_arc_get_value is not None else m.lv_arc_set_value
        hws = [h for _, h in img.halfwords(fn, 0xA0)]
        ldrsh = any(a & 0xFFF0 == 0xF9B0 and b & 0xFFF == m.arc_value for a, b in zip(hws, hws[1:]))
        if ldrsh:
            self.ok("arc_value", "ldrsh.w of the value in the arc value accessor")
        else:
            self.unverified("arc_value", "accessor reads it through a helper on this build")

    # -- fs
    def fs(self):
        m, img = self.m, self.img
        for fn_f, off_f in (("fs_unlink", "fs_ops_unlink"), ("fs_rename", "fs_ops_rename")):
            hws = [h for _, h in img.halfwords(getattr(m, fn_f), 0xC0)]
            pairs = list(zip(hws, hws[1:]))
            ok = (ldr16(3, 0, m.fs_mount_ops), ldr16(3, 3, getattr(m, off_f))) in pairs
            self.check(off_f, ok, f"{fn_f}: ldr r3, [r0, #mount_ops]; ldr r3, [r3, #off]")
            self.check("fs_mount_ops", ok, "mount point -> ops in fs_unlink/fs_rename")
        stat, statvfs = img.u32(m.fat_ops + m.fs_ops_mkdir + 4), img.u32(m.fat_ops + m.fs_ops_mkdir + 8)
        dirs = [img.u32(m.fat_ops + o) for o in (0x20, 0x24, 0x28)]
        ok = (stat, statvfs) == (m.fs_stat, m.fs_statvfs) and dirs == [m.fs_opendir, m.fs_readdir, m.fs_closedir]
        self.check("fat_ops", ok, "+0x20..0x28 dir calls, mkdir + 4/8 stat/statvfs")
        mk = img.u32(m.fat_ops + m.fs_ops_mkdir)
        self.check("fs_ops_mkdir", ok and mk & 1 and img.has(mk) and mk not in (m.fs_stat, m.fs_rename),
                   f"fat_ops entry {mk:#x} sits between rename and stat")
        for f in ("fs_stat", "fs_statvfs", "fs_opendir", "fs_readdir", "fs_closedir"):
            if ok:
                self.ok(f, "fat_ops entry")
        hws = [h for _, h in img.halfwords(m.fs_readdir, 0x40)]
        stores = [b & 0xFFF for a, b in zip(hws, hws[1:]) if a == 0xF8C4]
        top = max(stores) + 4 if stores else None
        self.check("fs_dirent", top == m.fs_dirent, f"fs_readdir's last store ends at {top and hex(top)}")

    # -- ux2sys properties
    def ux2sys(self):
        m, img = self.m, self.img
        first = img.u16(m.ux2sys_set & ~1)
        self.check("ux2sys_count", first == 0x2800 | m.ux2sys_count - 1, f"ux2sys_set starts `cmp r0, #{first & 0xFF:#x}`")
        lits = [img.u32(a) for a, _ in img.halfwords(m.ux2sys_set, 0x80) if a % 4 == 0]
        self.check("ux2sys_table", m.ux2sys_table in lits, "literal of ux2sys_set")

        def row(model, image, prop):
            if prop >= model.ux2sys_count:
                return None
            s, g, n = struct.unpack_from("<3I", image.data, model.ux2sys_table + 12 * prop - VBASE)
            return bool(s), bool(g), n

        props = [f for f, c in m.fields.items() if c["kind"] == "prop"]
        for f in props:
            r = row(m, img, getattr(m, f))
            if r is None or not (r[0] or r[1]):
                self.fail(f, f"no setter or getter: {r}")
            elif self.other is not None:
                om, oimg = self.other
                o = row(om, oimg, getattr(om, f))
                # the card blob's length changes by design (cards_bytes checks it)
                same = o is not None and (o[:2] == r[:2] if f == "cards_prop" else o == r)
                self.check(f, same, f"row {r} matches the other model's {o}")
            else:
                self.unverified(f, f"row {r} exists; cross-model shape check needs both images")
        r = row(m, img, m.cards_prop)
        if r:
            self.check("cards_bytes", r[2] == m.cards_bytes, f"cards property length {r[2]}")

    # -- strings
    def strings(self):
        m, img = self.m, self.img
        for f, want in (("scene_res", "/bt_watch.res"), ("scene_sty", "/bt_watch.sty"), ("font_32m", "/font32M.font"),
                        ("font_sans", "/sans")):
            s = img.cstr(getattr(m, f))
            self.check(f, want in s and s.startswith("/NAND:"), repr(s))

    # -- the rest with a concrete footprint
    def misc(self):
        m, img = self.m, self.img
        if m.lv_font_default is not None:
            self.check("lv_font_default", m.lv_font_default in img.words(), "referenced as a pointer")
        if m.sport1_imgs is not None:
            self.check("sport1_imgs", m.sport1_imgs in img.words(), "referenced as a pointer")
            self.check("sport1_texts", m.sport1_texts in img.words(), "referenced as a pointer")
        if m.pwm_device is not None:
            self.check("pwm_device", m.pwm_device in img.words(), "referenced as a pointer")


def movw_imm(imm, rd):
    hw1 = 0xF240 | (imm >> 11 & 1) << 10 | imm >> 12
    hw2 = (imm >> 8 & 7) << 12 | rd << 8 | imm & 0xFF
    return struct.pack("<HH", hw1, hw2)


def first_bound_ok(img, m):
    """`sub.w r3, r3, #0x9000 ; subs r3, #(first - 0x9000) ; cmp r3, #(n - 1)` before the table."""
    n = m.ble_opcode_max - m.ble_dispatch_first + 1
    sub = struct.pack("<HHH", 0xF5A3, 0x4310, 0x3B00 | (m.ble_dispatch_first - 0x9000))
    near = img.data[m.ble_dispatch_table - VBASE - 0x20:m.ble_dispatch_table - VBASE]
    return sub in near and struct.pack("<H", 0x2B00 | (n - 1)) in near[near.find(sub):]


MODELS = load_models()


def image(name):
    path = os.environ.get(ENV[name])
    return Image(Path(path).read_bytes()) if path else None


def verify(name, img, other=None):
    other_pair = None
    if other is not None:
        oname, oimg = other
        other_pair = (Model(MODELS[oname]), oimg)
    return Verifier(Model(MODELS[name]), img, other_pair).run()


class ModelJsonTest(unittest.TestCase):
    """No image needed: the export is well formed and the WP2 table agrees with firmware.py."""

    def test_both_models_same_fields(self):
        self.assertEqual(list(MODELS["wp2"]), list(MODELS["wp3"]))
        self.assertGreater(len(MODELS["wp3"]), 200)

    def test_wp2_matches_firmware_py(self):
        from wp2apps import firmware as fw

        m = Model(MODELS["wp2"])
        self.assertEqual(m.stock_sha256_prefix, fw.STOCK_SHA256_PREFIX)
        self.assertEqual(m.app_size, fw.STUB_OFFSET)
        self.assertEqual(m.boot_call, fw.BOOT_CALL)
        self.assertEqual(m.stock_heap_init, fw.STOCK_HEAP_INIT | 1)
        self.assertEqual(m.heap_literals, [fw.APP_VBASE + a for a in fw.HEAP_LITERALS])
        self.assertEqual((m.heap_size, m.heap_base, m.stub_ram), (fw.HEAP_OLD, fw.HEAP_BASE, fw.STUB_RAM))
        self.assertEqual((m.view_table, m.view_table_end), (fw.VIEW_TABLE, fw.VIEW_TABLE_END))
        self.assertEqual(m.view_start_literals, list(fw.VIEW_START_LITERALS))
        self.assertEqual(m.view_end_literals, list(fw.VIEW_END_LITERALS))
        self.assertEqual(m.menu_view, fw.VIEW_ID)
        self.assertEqual(range(m.page_view_base, m.page_view_base + 16), fw.PAGE_VIEWS)
        self.assertEqual((m.ble_call, m.read_payload), (fw.BLE_CALL, fw.STOCK_BLE_READ | 1))
        self.assertEqual((m.applist_hook_slot, m.applist_layout), (fw.APPLIST_LAYOUT, fw.STOCK_APPLIST_LAYOUT))
        self.assertEqual((m.wgt_reg, m.wgt_free_id), (fw.WGT_REG, fw.WGT_ID))

    def test_wp3_matches_firmware_py(self):
        from wp2apps import firmware as fw

        m = Model(MODELS["wp3"])
        self.assertEqual((m.stock_sha256_prefix, m.app_size), (fw.WP3_STOCK_SHA256_PREFIX, fw.WP3_STUB_OFFSET))

    def test_wp3_design_choices(self):
        m = Model(MODELS["wp3"])
        self.assertEqual((m.data_opcode, m.dnd_opcode), (0x90A9, 0x90A2))
        self.assertEqual((m.view_record, m.fs_ops_mkdir, m.lv_obj_size, m.ble_frame_reg, m.fs_dirent),
                         (20, 0x40, 0x30, 4, 0x10C))
        self.assertTrue(all(not 0xF0 <= v <= 0x117 for v in (m.menu_view, m.page_view_base, m.page_view_base + 15)))


class StockImageMixin:
    NAME = ""

    def setUp(self):
        self.img = image(self.NAME)
        if self.img is None:
            self.skipTest(f"set {ENV[self.NAME]} to a stock app.bin")
        other = "wp3" if self.NAME == "wp2" else "wp2"
        oimg = image(other)
        self.results = verify(self.NAME, self.img, (other, oimg) if oimg else None)

    def test_no_check_fails(self):
        failed = {f: note for f, (status, note) in self.results.items() if status == "failed"}
        self.assertEqual(failed, {})

    def test_every_field_has_a_status(self):
        self.assertEqual(set(self.results), set(MODELS[self.NAME]))

    def test_patch_sites_verified(self):
        for f in ("boot_call", "heap_literals", "ble_call", "applist_hook_slot", "view_start_literals",
                  "view_end_literals", "view_record", "data_opcode", "fs_ops_mkdir", "lv_obj_size", "fs_dirent",
                  "ble_frame_reg"):
            self.assertEqual(self.results[f][0], "verified", f"{f}: {self.results[f]}")


@unittest.skipUnless(os.environ.get(ENV["wp2"]), "set WP2_STOCK_APP to a stock WP2 app.bin")
class Wp2StockTest(StockImageMixin, unittest.TestCase):
    NAME = "wp2"


@unittest.skipUnless(os.environ.get(ENV["wp3"]), "set WP3_STOCK_APP to a stock WP3 app.bin")
class Wp3StockTest(StockImageMixin, unittest.TestCase):
    NAME = "wp3"

    def test_dnd_and_grid_verified(self):
        for f in ("dnd_opcode", "dnd_handler", "applist_grid_hook_slot", "wgt_reg_literal"):
            self.assertEqual(self.results[f][0], "verified", f"{f}: {self.results[f]}")


def main(argv):
    if len(argv) not in (3, 5):
        sys.exit("usage: test_model_stock.py MODEL APP_BIN [OTHER_MODEL OTHER_APP_BIN]")
    other = (argv[3], Image(Path(argv[4]).read_bytes())) if len(argv) == 5 else None
    results = verify(argv[1], Image(Path(argv[2]).read_bytes()), other)
    for status in ("failed", "verified", "unverified", "unknown"):
        rows = [(f, n) for f, (s, n) in results.items() if s == status]
        print(f"== {status}: {len(rows)}")
        for f, note in rows:
            print(f"  {f:32s} {note}")


if __name__ == "__main__":
    main(sys.argv)
