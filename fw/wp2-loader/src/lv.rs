//! ABI 1.2 executor: the app thread hands a command buffer to the UI thread, which validates it and runs it
//! (docs/specs/2026-11-11-lvgl-apps-design.md). Everything but `submit` runs on ui_service.

use crate::{app, ui};
use core::ffi::c_void;
use core::ptr::{addr_of_mut, null_mut};
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU8, Ordering};
use wp2_core::events::{Event, CROWN, LV};
use wp2_core::layout::stub_ram;
use wp2_core::lvcmd::{self, Class, Rec, Table, FOCUS_MAX, MAX_FONTS, MAX_OBJS, MAX_OPTIONS};
use wp2_core::stock::*;

type Obj = *mut c_void;

const BATCH_MAX: usize = 2048;
const E_ARG: i32 = -1;
const E_FONT: i32 = -2;

const FLAG_CLICKABLE: u32 = 2;
const FLAG_SCROLLABLE: u32 = 1 << 4;
// LV_OBJ_FLAG_USER_4: the crown scrolls the object (docs/recon-lvgl.md, Input)
const FLAG_KNOB: u32 = 1 << 30;
const ALIGN_CENTER: u32 = 9;
const VALUE_CHANGED: u32 = 28;
// lv event code per EVENTS mask bit: PRESSED, CLICKED, LONG_PRESSED, RELEASED, VALUE_CHANGED
const CODES: [u32; 5] = [1, 7, 5, 8, VALUE_CHANGED];
// events[h] bit: the crown callback is registered
const KNOB_CB: u8 = 0x80;
// events[h] bit: the FOCUS callbacks are registered
const FOCUS_CB: u8 = 0x40;
// LVGL 8.3 event codes the FOCUS container listens to
const SCROLL: u32 = 11;
const LAYOUT_CHANGED: u32 = 43;
const STATE_CHECKED: u32 = 0x01;
// LV_DIR_TOP | LV_DIR_BOTTOM: a page scrolls the pager vertically only
const DIR_VER: u32 = 0x0C;
// lv_tileview_tile_t: lv_obj_t (0x2C B), then u8 dir (tile constructor 0x10138078)
const TILE_DIR: usize = 0x2C;
// lv_obj_t coords: i32 x1 y1 x2 y2 at +0x14 (docs/recon-lvgl.md)
const Y1: usize = 0x18;
const Y2: usize = 0x20;
const FONT_NAMES: [&[u8; 3]; lvcmd::FONT_FILES as usize] = [
    b"18M", b"24L", b"24M", b"24R", b"28M", b"28R", b"30R", b"32M", b"32R", b"34R", b"36L", b"36M", b"36N", b"36R",
    b"44L", b"48M",
];

// mailbox: one batch in flight, the app thread waits for it
const IDLE: u8 = 0;
const PENDING: u8 = 1;
const DONE: u8 = 2;
static STATE: AtomicU8 = AtomicU8::new(IDLE);
static REQ_PTR: AtomicU32 = AtomicU32::new(0);
static REQ_LEN: AtomicU32 = AtomicU32::new(0);
static RESULT: AtomicI32 = AtomicI32::new(0);

// repr(C), objs first: an e2e probe reads the root at LV_STATE + 0
#[repr(C)]
struct Lv {
    // handle -> object; 0 is the root, null until the first submit
    objs: [Obj; MAX_OBJS + 1],
    // EVENTS bits with a callback, plus KNOB_CB
    events: [u8; MAX_OBJS + 1],
    // roller option count, 0 = OPTIONS never set; a pager's page (the one shown, or the one it is gliding to)
    options: [u8; MAX_OBJS + 1],
    // lv_font_t (0x20 B) per slot: styles point here, so it lives as long as the loader block
    fonts: [[u32; 8]; MAX_FONTS as usize],
    fonts_open: u8,
    table: Table,
    // FOCUS: the container (0 = none), its child 0 side at the centre, the child nearest the centre (-1 = none
    // yet), the shape last set on each child (0 = none)
    focus: u16,
    focus_icon: u16,
    focus_sel: i16,
    focus_last: [u32; FOCUS_MAX],
}

// Lives in the loader block: zero again on every open of the view. Table::new() is not const: the zeroed table is
// reset on the first submit (no root yet).
static mut LV_STATE: Lv = Lv {
    objs: [null_mut(); MAX_OBJS + 1],
    events: [0; MAX_OBJS + 1],
    options: [0; MAX_OBJS + 1],
    fonts: [[0; 8]; MAX_FONTS as usize],
    fonts_open: 0,
    table: unsafe { core::mem::zeroed() },
    focus: 0,
    focus_icon: 0,
    focus_sel: -1,
    focus_last: [0; FOCUS_MAX],
};

fn st() -> &'static mut Lv {
    unsafe { &mut *addr_of_mut!(LV_STATE) }
}

pub fn root() -> Obj {
    st().objs[0]
}

/// UI thread: a horizontal pager lives; the right back swipe stays with it (main.rs key).
pub fn hpager() -> bool {
    !root().is_null() && st().table.has(Class::PagerH)
}

/// UI thread: the object of a live label, else null.
pub fn label(h: u16) -> Obj {
    if h != 0 && st().table.class(h) == Some(Class::Label) { st().objs[h as usize] } else { null_mut() }
}

/// App thread, inside submit (the table holds still): `h` is 0 or a live label.
fn label_ok(h: u16) -> bool {
    lvcmd::media_label_ok(&st().table, h)
}

/// App thread (Api.lv_submit): waits until the UI tick ran the batch.
pub extern "C" fn submit(buf: *const u8, len: usize) -> i32 {
    let ram = stub_ram();
    let (p, block, size) = (buf as usize, ram.app_block as usize, ram.app_size as usize);
    // no p + len: a buf near the top of the address space would wrap
    if buf.is_null() || len > BATCH_MAX || p < block || p - block > size || len > size - (p - block) {
        return E_ARG;
    }
    // SENSOR (heart rate, GPS) needs no LVGL: a canvas app sends it too, and it must not make the root
    if let Some((sensor, on)) = lvcmd::sensor(unsafe { core::slice::from_raw_parts(buf, len) }) {
        if sensor == wp2_core::gps::SENSOR_GPS {
            return crate::gps::request(on);
        }
        return crate::hr::request(sensor, on);
    }
    if lvcmd::gps_fix(unsafe { core::slice::from_raw_parts(buf, len) }) {
        // the record lies in the app block (checked above): its payload is the app's to overwrite
        return crate::gps::fix(unsafe { (buf as *mut u8).add(4) });
    }
    if lvcmd::page_query(unsafe { core::slice::from_raw_parts(buf, len) }) {
        return crate::app::ON_PAGE.load(Ordering::Acquire) as i32;
    }
    if let Some(rec) = lvcmd::widget(unsafe { core::slice::from_raw_parts(buf, len) }) {
        // a v2 record the running stub cannot draw (or a malformed one): -22, so the app falls back to v1
        if !wp2_core::widget::accepts(rec, wp2_core::layout::stub_ram().version()) {
            return -22;
        }
        return crate::svc::save_widget(rec);
    }
    if let Some(cmd) = lvcmd::media_key(unsafe { core::slice::from_raw_parts(buf, len) }) {
        return crate::media::key(cmd);
    }
    if let Some((on, title, artist)) = lvcmd::media_bind(unsafe { core::slice::from_raw_parts(buf, len) }) {
        if root().is_null() || !label_ok(title) || !label_ok(artist) {
            return -22;
        }
        crate::media::bind(on, title, artist);
        return 0;
    }
    REQ_PTR.store(p as u32, Ordering::Relaxed);
    REQ_LEN.store(len as u32, Ordering::Relaxed);
    STATE.store(PENDING, Ordering::Release);
    while STATE.load(Ordering::Acquire) != DONE {
        stock!(OS_SLEEP, fn(u32))(5);
    }
    let r = RESULT.load(Ordering::Relaxed);
    STATE.store(IDLE, Ordering::Release);
    r
}

/// UI tick: runs a pending batch. The app thread is blocked in `submit`, so the buffer holds still.
pub fn tick() {
    if STATE.load(Ordering::Acquire) != PENDING {
        return;
    }
    let buf = unsafe {
        core::slice::from_raw_parts(REQ_PTR.load(Ordering::Relaxed) as *const u8, REQ_LEN.load(Ordering::Relaxed) as usize)
    };
    // Storage runs on ui_service too: forced app stop cannot abort a filesystem operation midway.
    let result = if wp2_core::storage::request(buf).is_some() {
        // the request lies in the app block (checked in submit): its response space is the app's to overwrite
        crate::storage::execute(unsafe { core::slice::from_raw_parts_mut(buf.as_ptr() as *mut u8, buf.len()) })
    } else {
        run(buf)
    };
    RESULT.store(result, Ordering::Relaxed);
    STATE.store(DONE, Ordering::Release);
}

fn run(buf: &[u8]) -> i32 {
    if root().is_null() {
        st().table.reset();
    }
    if let Err(e) = lvcmd::validate(buf, &st().table, st().fonts_open) {
        return e.code();
    }
    if root().is_null() {
        make_root();
    }
    for r in lvcmd::records(buf) {
        // validated: every record decodes
        let Ok(r) = r else { return -22 };
        if let Err(code) = exec(&r) {
            return code;
        }
        lvcmd::apply(&mut st().table, &r);
    }
    0
}

// 466x466, black, not scrollable; touch and crown on its background go to the app as on the image
fn make_root() {
    let root = stock!(LV_OBJ_CREATE, fn(Obj) -> Obj)(ui::screen());
    stock!(LV_OBJ_SET_SIZE, fn(Obj, i32, i32))(root, 466, 466);
    style(root, LV_STYLE_BG_COLOR, 0, 0);
    style(root, LV_STYLE_BG_OPA, 0xFF, 0);
    stock!(LV_OBJ_CLEAR_FLAG, fn(Obj, u32))(root, FLAG_SCROLLABLE);
    stock!(LV_OBJ_ADD_FLAG, fn(Obj, u32))(root, FLAG_CLICKABLE);
    for filter in [ui::PRESSED, ui::RELEASED, ui::KNOB] {
        add_cb(root, ui::input(), filter, 0);
    }
    st().objs[0] = root;
    st().events[0] = KNOB_CB;
    // the image hides, the root takes the knob
    ui::show(true);
}

fn style(obj: Obj, prop: u32, value: u32, sel: u32) {
    stock!(LV_OBJ_SET_LOCAL_STYLE_PROP, fn(Obj, u32, u32, u32))(obj, prop, value, sel)
}

fn add_cb(obj: Obj, cb: ui::Cb, filter: u32, h: usize) {
    stock!(LV_OBJ_ADD_EVENT_CB, fn(Obj, ui::Cb, u32, usize) -> *mut c_void)(obj, cb, filter, h);
}

// a copy with a trailing NUL: TEXT and OPTIONS payloads are not terminated, LVGL copies the string
fn cstr(s: &[u8], out: &mut [u8; MAX_OPTIONS + 1]) -> *const u8 {
    out[..s.len()].copy_from_slice(s);
    out[s.len()] = 0;
    out.as_ptr()
}

fn exec(r: &Rec) -> Result<(), i32> {
    let s = st();
    let o = |h: u16| s.objs[h as usize];
    let mut tmp = [0u8; MAX_OPTIONS + 1];
    match *r {
        Rec::Create { h, class, parent } => {
            let create = match class {
                Class::Obj => LV_OBJ_CREATE,
                Class::Label => LV_LABEL_CREATE,
                Class::Btn => LV_BTN_CREATE,
                Class::Arc => LV_ARC_CREATE,
                Class::Bar => LV_BAR_CREATE,
                Class::Slider => LV_SLIDER_CREATE,
                Class::Roller => LV_ROLLER_CREATE,
                Class::Pager | Class::PagerH => LV_TILEVIEW_CREATE,
                Class::Page => {
                    // the next row of its pager, or the next column of a horizontal one (validated: the parent is one)
                    let n = stock!(LV_OBJ_GET_CHILD_CNT, fn(Obj) -> u32)(o(parent));
                    let add = stock!(LV_TILEVIEW_ADD_TILE, fn(Obj, u32, u32, u32) -> Obj);
                    let obj = if s.table.class(parent) == Some(Class::PagerH) {
                        let (dir, prev) = lvcmd::hpage_dirs(n);
                        if let Some(d) = prev {
                            // the stock's middle tile goes both ways: the one before the newest gains RIGHT
                            unsafe { (child(o(parent), n - 1) as *mut u8).add(TILE_DIR).write(d) };
                        }
                        add(o(parent), n, 0, dir.into())
                    } else {
                        add(o(parent), 0, n, DIR_VER)
                    };
                    let h = h as usize;
                    (s.objs[h], s.events[h], s.options[h]) = (obj, 0, 0);
                    return Ok(());
                }
                Class::ScrollBar => return Err(-22),
                Class::Music => {
                    let obj = crate::music::create(o(parent), h)?;
                    let h = h as usize;
                    (s.objs[h], s.events[h], s.options[h]) = (obj, 0, 0);
                    return Ok(());
                }
            };
            let obj = stock!(create, fn(Obj) -> Obj)(o(parent));
            let h = h as usize;
            (s.objs[h], s.events[h], s.options[h]) = (obj, 0, 0);
            if class == Class::PagerH {
                // the page it settles on goes to the app; no crown (the stock workout screen pages by swipe only)
                add_cb(obj, on_pager, VALUE_CHANGED, h);
            }
            if class == Class::Pager {
                // the page it settles on goes to the app; the crown steps pages (on_event), as a roller takes the knob
                add_cb(obj, on_pager, VALUE_CHANGED, h);
                add_cb(obj, on_event, ui::KNOB, h);
                s.events[h] = KNOB_CB;
                stock!(LV_DISP_ADD_KNOB_OBJ, fn(Obj))(obj);
            }
        }
        Rec::Del(h) => {
            // before the delete: deleting children sends the container LAYOUT_CHANGED, and its fisheye must not touch
            // objects LVGL is freeing (it did: garbage reached the display engine)
            if lvcmd::freed_by_del(&s.table, h).any(|x| x == s.focus) {
                s.focus = 0;
            }
            let music = crate::music::handle();
            let music_gone = music != 0 && lvcmd::freed_by_del(&s.table, h).any(|x| x == music);
            if music_gone {
                crate::music::close(); // on the root, not under `h`
            }
            stock!(LV_OBJ_DEL, fn(Obj))(o(h));
            if music_gone {
                crate::music::free();
            }
            // the focus container's children may have shifted: shape them all again
            s.focus_last = [0; FOCUS_MAX];
            // scroll bars targeting the subtree sit on the root: deleted after their target, as stock lists do.
            // A bar handle never gets here (validate refuses DEL of a bar).
            for x in lvcmd::freed_by_del(&s.table, h) {
                let x = x as usize;
                if x != h as usize && s.table.class(x as u16) == Some(Class::ScrollBar) {
                    stock!(LV_OBJ_DEL, fn(Obj))(s.objs[x]);
                }
                s.objs[x] = null_mut();
            }
        }
        Rec::AppIcon {h,slot,side} => crate::menu::lv::app_icon(o(h),slot,side)?,
        Rec::Size { h, w, hh } => stock!(LV_OBJ_SET_SIZE, fn(Obj, i32, i32))(o(h), w, hh),
        Rec::Align { h, align, x, y } => stock!(LV_OBJ_ALIGN, fn(Obj, u32, i32, i32))(o(h), align.into(), x.into(), y.into()),
        Rec::Flags { h, set, clear } => {
            if set != 0 {
                stock!(LV_OBJ_ADD_FLAG, fn(Obj, u32))(o(h), set);
            }
            if clear != 0 {
                stock!(LV_OBJ_CLEAR_FLAG, fn(Obj, u32))(o(h), clear);
            }
        }
        Rec::Style { h, prop, sel, value } => {
            let v = if u32::from(prop) == LV_STYLE_TEXT_FONT { s.fonts[value as usize].as_ptr() as u32 } else { value as u32 };
            style(o(h), prop.into(), v, sel);
        }
        Rec::Font { slot, font } => open_font(slot as usize, font as usize)?,
        Rec::Text { h, s: t } => stock!(LV_LABEL_SET_TEXT, fn(Obj, *const u8))(o(h), cstr(t, &mut tmp)),
        Rec::Value { h, v, anim } => match s.table.class(h) {
            // validated: arc values fit i16, roller indices u16
            Some(Class::Arc) => stock!(LV_ARC_SET_VALUE, fn(Obj, i16))(o(h), v as i16),
            Some(Class::Pager | Class::PagerH) => go_page(h, v as u32, anim),
            Some(Class::Music) => crate::music::set_playing(v == 1),
            Some(Class::Roller) => {
                let n = s.options[h as usize];
                if n != 0 {
                    let sel = (v as u32).min(u32::from(n) - 1) as u16;
                    stock!(LV_ROLLER_SET_SELECTED, fn(Obj, u16, u32))(o(h), sel, anim.into());
                }
            }
            _ => stock!(LV_BAR_SET_VALUE, fn(Obj, i32, u32))(o(h), v, anim.into()),
        },
        Rec::Range { h, min, max } => stock!(LV_BAR_SET_RANGE, fn(Obj, i32, i32))(o(h), min, max),
        Rec::Options { h, mode, s: t } => {
            stock!(LV_ROLLER_SET_OPTIONS, fn(Obj, *const u8, u32))(o(h), cstr(t, &mut tmp), mode.into());
            // ≤ 240 B: at most 121 options
            s.options[h as usize] = (t.iter().filter(|&&c| c == b'\n').count() + 1) as u8;
        }
        Rec::Events { h, mask } => {
            let new = mask & !s.events[h as usize];
            for (bit, &code) in CODES.iter().enumerate() {
                if new >> bit & 1 != 0 {
                    add_cb(o(h), on_event, code, h as usize);
                }
            }
            s.events[h as usize] |= mask;
        }
        Rec::Flex { h, flow, main, cross, track } => {
            stock!(LV_OBJ_SET_FLEX_FLOW, fn(Obj, u32))(o(h), flow.into());
            stock!(LV_OBJ_SET_FLEX_ALIGN, fn(Obj, u32, u32, u32))(o(h), main.into(), cross.into(), track.into());
        }
        Rec::ScrollBar { h, target } => {
            // the stock call order (docs/recon-lvgl.md, Vendor components)
            let bar = stock!(JX_SCROLL_BAR_CREATE, fn(Obj) -> Obj)(o(0));
            stock!(LV_OBJ_SET_SIZE, fn(Obj, i32, i32))(bar, 462, 462);
            stock!(JX_SCROLL_BAR_SET_SCROLL_OBJ, fn(Obj, Obj))(bar, o(target));
            stock!(JX_SCROLL_BAR_SET_KNOB, fn(Obj, bool))(bar, true);
            stock!(LV_OBJ_ALIGN, fn(Obj, u32, i32, i32))(bar, ALIGN_CENTER, 0, 0);
            let h = h as usize;
            (s.objs[h], s.events[h], s.options[h]) = (bar, 0, 0);
        }
        Rec::Knob(h) => {
            let obj = o(h);
            if s.table.class(h) == Some(Class::Pager) {
                // its crown callback is registered at CREATE
                stock!(LV_DISP_ADD_KNOB_OBJ, fn(Obj))(obj);
            } else if stock!(LV_OBJ_HAS_FLAG, fn(Obj, u32) -> bool)(obj, FLAG_SCROLLABLE) {
                // the crown scrolls it; the flag registers it as the knob object. A flag set before (by its
                // SCROLLBAR) registers nothing, and a roller created since holds the knob: make it current
                stock!(LV_OBJ_ADD_FLAG, fn(Obj, u32))(obj, FLAG_KNOB);
                stock!(LV_DISP_ADD_KNOB_OBJ, fn(Obj))(obj);
            } else {
                // crown events (CROWN to the app); a KNOB filter registers the knob object too
                if s.events[h as usize] & KNOB_CB == 0 {
                    add_cb(obj, on_event, ui::KNOB, h as usize);
                    s.events[h as usize] |= KNOB_CB;
                }
                stock!(LV_DISP_ADD_KNOB_OBJ, fn(Obj))(obj);
            }
        }
        Rec::Angles { h, start, end, rot } => {
            stock!(LV_ARC_SET_BG_ANGLES, fn(Obj, u16, u16))(o(h), start, end);
            stock!(LV_ARC_SET_ROTATION, fn(Obj, u16))(o(h), rot);
        }
        Rec::LongMode { h, mode } => stock!(LV_LABEL_SET_LONG_MODE, fn(Obj, u32))(o(h), mode.into()),
        Rec::Focus { h, icon } => {
            if s.events[h as usize] & FOCUS_CB == 0 {
                add_cb(o(h), on_focus, SCROLL, h as usize);
                add_cb(o(h), on_focus, LAYOUT_CHANGED, h as usize);
                s.events[h as usize] |= FOCUS_CB;
            }
            (s.focus, s.focus_icon, s.focus_sel, s.focus_last) = (h, icon, -1, [0; FOCUS_MAX]);
            // before the first layout the coords are stale; LAYOUT_CHANGED shapes the children again
            shape_focus();
        }
    }
    Ok(())
}

fn mid_y(obj: Obj) -> i32 {
    unsafe { (*(obj.cast::<u8>().add(Y1) as *const i32) + *(obj.cast::<u8>().add(Y2) as *const i32)) / 2 }
}

// user_data = handle; an older FOCUS container keeps its callbacks but is no longer shaped
extern "C" fn on_focus(e: *mut c_void) {
    if stock!(LV_EVENT_GET_USER_DATA, fn(*mut c_void) -> usize)(e) as u16 == st().focus {
        shape_focus();
    }
}

// The stock app list's fisheye on every child of the FOCUS container: x shift, text opacity and the
// side of child 0 by distance from the container's centre line; the nearest child is CHECKED and the app gets
// FOCUSED with its index when it changes. The shift is the child's PAD_LEFT, so its content moves inside it: a
// TRANSLATE_X widens the container's content and makes it scroll sideways (and lv_obj_set_scroll_dir is not
// found in the image). A shape is set only when it changed: a style change relayouts the container, which sends
// LAYOUT_CHANGED again.
fn shape_focus() {
    let s = st();
    let h = s.focus;
    let cont = s.objs[h as usize];
    if h == 0 || cont.is_null() {
        return;
    }
    let child = |o: Obj, i: usize| stock!(LV_OBJ_GET_CHILD, fn(Obj, i32) -> Obj)(o, i as i32);
    let n = (stock!(LV_OBJ_GET_CHILD_CNT, fn(Obj) -> u32)(cont) as usize).min(FOCUS_MAX);
    let cy = mid_y(cont);
    let mut best = (i32::MAX, -1i16);
    for i in 0..n {
        let c = child(cont, i);
        let d = (mid_y(c) - cy).abs();
        if d < best.0 {
            best = (d, i as i16);
        }
        let (shift, zoom, opa) = lvcmd::focus_shape(d);
        // shift < 256, zoom <= 256, opa < 256
        let packed = shift as u32 | zoom << 8 | opa << 20;
        if s.focus_last[i] == packed {
            continue;
        }
        s.focus_last[i] = packed;
        style(c, LV_STYLE_TEXT_OPA, opa, 0);
        if s.focus_icon == lvcmd::FOCUS_CARDS {
            // the card itself narrows, its content moves with its left edge. The container lays the cards out at a
            // fixed x (a width set from LAYOUT_CHANGED is not laid out again, so a flex centre lags a scroll
            // behind); TRANSLATE_X centres it without a relayout.
            style(c, LV_STYLE_WIDTH, lvcmd::card_width(d) as u32, 0);
            style(c, LV_STYLE_TRANSLATE_X, shift as u32, 0);
            continue;
        }
        style(c, LV_STYLE_PAD_LEFT, shift as u32, 0);
        let icon = if s.focus_icon != 0 { child(c, 0) } else { null_mut() };
        if !icon.is_null() {
            let side = (u32::from(s.focus_icon) * zoom / 256) as i32;
            stock!(LV_OBJ_SET_SIZE, fn(Obj, i32, i32))(icon, side, side);
            // the stock icon zooms around its centre (simple_img_set_zoom keeps the 96 px box), so the smaller
            // child 0 moves right by half of what it lost; its siblings stay put, as the stock label does
            style(icon, LV_STYLE_TRANSLATE_X, ((i32::from(s.focus_icon) - side) / 2) as u32, 0);
        }
    }
    if best.1 == s.focus_sel {
        return;
    }
    s.focus_sel = best.1;
    // clear_state on a child without CHECKED returns early (lv_obj_set_state)
    for i in 0..n {
        let set = if i as i16 == best.1 { LV_OBJ_ADD_STATE } else { LV_OBJ_CLEAR_STATE };
        stock!(set, fn(Obj, u32))(child(cont, i), STATE_CHECKED);
    }
    if best.1 >= 0 && app::running() && !app::paused() {
        app::QUEUE.coalesce(Event { kind: LV, x: h as i16, y: lvcmd::FOCUSED, delta: best.1, ..Event::default() });
    }
}

// A failed open leaves the built-in font in the slot (styles may already point at it) and the slot closed.
fn open_font(slot: usize, font: usize) -> Result<(), i32> {
    let s = st();
    let f = s.fonts[slot].as_mut_ptr();
    if s.fonts_open >> slot & 1 != 0 {
        stock!(LVGL_BITMAP_FONT_CLOSE, fn(*mut u32))(f);
        s.fonts_open &= !(1 << slot);
    }
    let mut path = *b"/NAND:A/fontXXX.font\0";
    path[12..15].copy_from_slice(FONT_NAMES[font]);
    if stock!(LVGL_BITMAP_FONT_OPEN, fn(*mut u32, *const u8) -> i32)(f, path.as_ptr()) != 0 {
        s.fonts[slot] = unsafe { (LV_FONT_DEFAULT as *const [u32; 8]).read() };
        return Err(E_FONT);
    }
    s.fonts_open |= 1 << slot;
    Ok(())
}

// user_data = handle
extern "C" fn on_event(e: *mut c_void) {
    if !app::running() || app::paused() {
        return;
    }
    let code = ui::event_code(e);
    let h = stock!(LV_EVENT_GET_USER_DATA, fn(*mut c_void) -> usize)(e) as u16;
    if code == ui::KNOB && st().table.class(h) == Some(Class::Pager) {
        pager_crown(h, ui::knob_step(e));
        return;
    }
    if code == ui::KNOB {
        let step = ui::knob_step(e).signum() as i16;
        if step != 0 {
            app::QUEUE.push(Event { kind: CROWN, delta: step, ..Event::default() });
        }
        return;
    }
    if code == VALUE_CHANGED && matches!(st().table.class(h), Some(Class::Pager | Class::PagerH)) {
        return; // on_pager reports it, with the page
    }
    let mut ev = Event { kind: LV, x: h as i16, y: code as i16, ..Event::default() };
    if code != VALUE_CHANGED {
        app::QUEUE.push(ev);
        return;
    }
    let obj = st().objs[h as usize];
    let v = match st().table.class(h) {
        Some(Class::Arc) => stock!(LV_ARC_GET_VALUE, fn(Obj) -> i16)(obj).into(),
        Some(Class::Bar | Class::Slider) => stock!(LV_BAR_GET_VALUE, fn(Obj) -> i32)(obj),
        Some(Class::Roller) => stock!(LV_ROLLER_GET_SELECTED, fn(Obj) -> u16)(obj).into(),
        _ => 0,
    };
    ev.delta = v.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
    app::QUEUE.coalesce(ev);
}

fn child(o: Obj, i: u32) -> Obj {
    stock!(LV_OBJ_GET_CHILD, fn(Obj, i32) -> Obj)(o, i as i32)
}

// Glides the pager to page `i` (the last one past the end); its VALUE_CHANGED follows when the glide ends.
fn go_page(h: u16, i: u32, anim: bool) {
    let obj = st().objs[h as usize];
    let n = stock!(LV_OBJ_GET_CHILD_CNT, fn(Obj) -> u32)(obj);
    if n == 0 {
        return;
    }
    let i = i.min(n - 1);
    st().options[h as usize] = i as u8;
    let (col, row) = if st().table.class(h) == Some(Class::PagerH) { (i, 0) } else { (0, i) };
    stock!(LV_OBJ_SET_TILE_ID, fn(Obj, u32, u32, u32))(obj, col, row, anim.into());
}

// The crown on a pager, like a drag: the page's own content scrolls first (by the stock crown step), and at its end
// the next step glides to the next page. `step` > 0 moves down the content (to the next page).
fn pager_crown(h: u16, step: i32) {
    if step == 0 {
        return; // the "crown still" event
    }
    let s = st();
    let obj = s.objs[h as usize];
    let at = u32::from(s.options[h as usize]);
    let page = child(obj, at);
    if page.is_null() {
        return;
    }
    let y = stock!(LV_OBJ_GET_SCROLL_Y, fn(Obj) -> i32)(page);
    let room = if step > 0 { stock!(LV_OBJ_GET_SCROLL_BOTTOM, fn(Obj) -> i32)(page) } else { y };
    if room > 0 {
        let dy = step.abs().min(room) * step.signum();
        stock!(LV_OBJ_SCROLL_TO_Y, fn(Obj, i32, u32))(page, y + dy, 0);
    } else if crate::app::millis().wrapping_sub(unsafe { PAGED_AT }) < GLIDE_MS {
        // a turn gives many steps: one page per glide, not one per step (the stock tileview sets its active tile at
        // the glide's start, so it cannot tell a glide)
    } else if step > 0 || at > 0 {
        unsafe { PAGED_AT = crate::app::millis() };
        go_page(h, if step > 0 { at + 1 } else { at - 1 }, true);
    }
}

const GLIDE_MS: u32 = 400;
static mut PAGED_AT: u32 = 0;

// VALUE_CHANGED of a pager (the stock tileview sends it when a scroll settles on a page): the page index to the app.
// user_data = handle
extern "C" fn on_pager(e: *mut c_void) {
    let h = stock!(LV_EVENT_GET_USER_DATA, fn(*mut c_void) -> usize)(e) as u16;
    let s = st();
    let obj = s.objs[h as usize];
    let act = stock!(LV_TILEVIEW_GET_TILE_ACT, fn(Obj) -> Obj)(obj);
    let n = stock!(LV_OBJ_GET_CHILD_CNT, fn(Obj) -> u32)(obj);
    let Some(i) = (0..n).find(|&i| child(obj, i) == act) else { return };
    s.options[h as usize] = i as u8;
    if app::running() && !app::paused() {
        app::QUEUE.coalesce(Event { kind: LV, x: h as i16, y: VALUE_CHANGED as i16, delta: i as i16, ..Event::default() });
    }
}

/// UI thread, app thread aborted, before the app block is freed: nothing of the app is left in LVGL.
pub fn teardown() {
    let s = st();
    s.focus = 0; // before the delete, as in DEL
    crate::music::close();
    if !s.objs[0].is_null() {
        // children first, scroll bars after their targets (created later)
        stock!(LV_OBJ_DEL, fn(Obj))(s.objs[0]);
    }
    crate::music::free();
    for slot in 0..MAX_FONTS as usize {
        if s.fonts_open >> slot & 1 != 0 {
            stock!(LVGL_BITMAP_FONT_CLOSE, fn(*mut u32))(s.fonts[slot].as_mut_ptr());
        }
    }
    s.fonts_open = 0;
    s.objs = [null_mut(); MAX_OBJS + 1];
    s.events = [0; MAX_OBJS + 1];
    s.options = [0; MAX_OBJS + 1];
    s.table.reset();
    (s.focus, s.focus_icon, s.focus_sel, s.focus_last) = (0, 0, -1, [0; FOCUS_MAX]);
    STATE.store(IDLE, Ordering::Release);
}
