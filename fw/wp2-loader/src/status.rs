// The status bar on every app, as the stock workouts' top line: the watch GPS's strength in three bars while the
// app holds the GPS, and the time. LVGL objects over the app's image (or its LVGL root), so LVGL draws them over
// whatever the app draws and the app's pixels are never touched; non-clickable, touches go through to the app.
use core::ffi::c_void;
use core::ptr::{addr_of_mut, null_mut};
use core::sync::atomic::{AtomicBool, Ordering};
use wp2_core::stock::*;

type Obj = *mut c_void;

const FLAG_HIDDEN: u32 = 1;
const FLAG_CLICKABLE: u32 = 2;
const FLAG_SCROLLABLE: u32 = 1 << 4;
const LV_STYLE_BORDER_WIDTH: u32 = 0x32;
const LV_STYLE_SHADOW_WIDTH: u32 = 0x50;
const TEXT_ALIGN_CENTER: u32 = 2;

/// Where it sits: centred at the very top of the round panel (the chord there is ~120 px), over every app's title.
const Y: i32 = 4;
const LABEL_W: i32 = 72;
const BAR_W: i32 = 3;
const BAR_H: i32 = 17;
const BAR_GAP: i32 = 3;
/// bars, a gap, the time: their width together
const BARS_W: i32 = 3 * BAR_W + 2 * BAR_GAP;
const WHITE: u32 = 0xFFFF;
const GREY: u32 = 0x4208;
const EVERY_MS: u32 = 500;

/// The app's wish (STATUS record, lv.rs): the bar shown; every app starts with it (app::launch).
pub static SHOWN: AtomicBool = AtomicBool::new(true);

struct Status {
    parent: Obj,
    label: Obj,
    bars: [Obj; 3],
    font: [u32; 8],
    font_open: bool,
    /// what is shown: minute of the day (0xFFFF none), GPS level (0xFF the bars hidden)
    minute: u16,
    level: u8,
    at: u32,
    /// the app hid the bar (SHOWN false), as last applied
    hidden: bool,
}

static mut ST: Status = Status {
    parent: null_mut(),
    label: null_mut(),
    bars: [null_mut(); 3],
    font: [0; 8],
    font_open: false,
    minute: 0xFFFE,
    level: 0xFE,
    at: 0,
    hidden: false,
};

fn st() -> &'static mut Status {
    unsafe { &mut *addr_of_mut!(ST) }
}

fn style(obj: Obj, prop: u32, value: u32) {
    stock!(LV_OBJ_SET_LOCAL_STYLE_PROP, fn(Obj, u32, u32, u32))(obj, prop, value, 0)
}

/// The bar on `parent` (the app's image, then its LVGL root if it makes one): any old one goes first.
pub fn attach(parent: Obj) {
    let s = st();
    if !s.parent.is_null() {
        for o in [s.label, s.bars[0], s.bars[1], s.bars[2]] {
            if !o.is_null() {
                stock!(LV_OBJ_DEL, fn(Obj))(o);
            }
        }
    }
    s.parent = parent;
    if parent.is_null() {
        return;
    }
    if !s.font_open {
        let path = b"/NAND:A/font24M.font\0";
        s.font_open = stock!(LVGL_BITMAP_FONT_OPEN, fn(*mut u32, *const u8) -> i32)(s.font.as_mut_ptr(), path.as_ptr()) == 0;
        if !s.font_open {
            s.font = unsafe { (LV_FONT_DEFAULT as *const [u32; 8]).read() };
        }
    }
    for b in s.bars.iter_mut() {
        let o = stock!(LV_OBJ_CREATE, fn(Obj) -> Obj)(parent);
        stock!(LV_OBJ_SET_SIZE, fn(Obj, i32, i32))(o, BAR_W, BAR_H);
        stock!(LV_OBJ_CLEAR_FLAG, fn(Obj, u32))(o, FLAG_CLICKABLE | FLAG_SCROLLABLE);
        style(o, LV_STYLE_BG_OPA, 0xFF);
        style(o, LV_STYLE_BORDER_WIDTH, 0);
        style(o, LV_STYLE_SHADOW_WIDTH, 0);
        style(o, LV_STYLE_RADIUS, 1);
        *b = o;
    }
    let l = stock!(LV_LABEL_CREATE, fn(Obj) -> Obj)(parent);
    stock!(LV_OBJ_CLEAR_FLAG, fn(Obj, u32))(l, FLAG_CLICKABLE | FLAG_SCROLLABLE);
    stock!(LV_OBJ_SET_SIZE, fn(Obj, i32, i32))(l, LABEL_W, 30);
    style(l, LV_STYLE_TEXT_FONT, s.font.as_ptr() as u32);
    style(l, LV_STYLE_TEXT_COLOR, WHITE);
    style(l, LV_STYLE_TEXT_OPA, 0xFF);
    style(l, LV_STYLE_TEXT_ALIGN, TEXT_ALIGN_CENTER);
    s.label = l;
    (s.minute, s.level, s.at, s.hidden) = (0xFFFE, 0xFE, 0, false);
    update(crate::app::millis(), true);
}

/// The objects went with their parent (app exit): forget them, close the font.
pub fn detach() {
    let s = st();
    s.parent = null_mut();
    s.label = null_mut();
    s.bars = [null_mut(); 3];
    if s.font_open {
        stock!(LVGL_BITMAP_FONT_CLOSE, fn(*mut u32))(s.font.as_mut_ptr());
        s.font_open = false;
    }
}

/// UI thread, every tick: the time and the GPS bars, twice a second.
pub fn tick() {
    update(crate::app::millis(), false);
}

fn update(now: u32, force: bool) {
    let s = st();
    if s.label.is_null() {
        return;
    }
    // hidden or shown again at once, not on the next half second
    let hide = !SHOWN.load(Ordering::Acquire);
    if hide != s.hidden {
        s.hidden = hide;
        for o in [s.label, s.bars[0], s.bars[1], s.bars[2]] {
            stock!(LV_OBJ_ADD_FLAG, fn(Obj, u32))(o, FLAG_HIDDEN);
        }
        if !hide {
            // shown again: the label back, the bars as the GPS level wants
            stock!(LV_OBJ_CLEAR_FLAG, fn(Obj, u32))(s.label, FLAG_HIDDEN);
            (s.level, s.at) = (0xFE, 0);
        }
    }
    if hide || (!force && now.wrapping_sub(s.at) < EVERY_MS) {
        return;
    }
    s.at = now;
    let minute = crate::svc::clock().map_or(0xFFFF, |(h, m)| u16::from(h) * 60 + u16::from(m));
    let level = crate::gps::level().unwrap_or(0xFF);
    if level != s.level {
        let bars = level != 0xFF;
        let x0 = 233 - (BARS_W + LABEL_W) / 2;
        for (i, &b) in s.bars.iter().enumerate() {
            if bars {
                stock!(LV_OBJ_CLEAR_FLAG, fn(Obj, u32))(b, FLAG_HIDDEN);
                stock!(LV_OBJ_SET_POS, fn(Obj, i32, i32))(b, x0 + i as i32 * (BAR_W + BAR_GAP), Y + 5);
                style(b, LV_STYLE_BG_COLOR, if (i as u8) < level { WHITE } else { GREY });
            } else {
                stock!(LV_OBJ_ADD_FLAG, fn(Obj, u32))(b, FLAG_HIDDEN);
            }
        }
        let lx = if bars { x0 + BARS_W } else { 233 - LABEL_W / 2 };
        stock!(LV_OBJ_SET_POS, fn(Obj, i32, i32))(s.label, lx, Y);
        s.level = level;
    }
    if minute != s.minute {
        let mut t = *b"--:--\0";
        if minute != 0xFFFF {
            t = clock12(minute);
        }
        stock!(LV_LABEL_SET_TEXT, fn(Obj, *const u8))(s.label, t.as_ptr());
        s.minute = minute;
    }
}

/// Minutes of the day as 12-hour time, NUL-ended: 0:05 → "12:05", 18:07 → "6:07".
pub fn clock12(minute: u16) -> [u8; 6] {
    let (h, m) = ((minute / 60 % 24) as u8, (minute % 60) as u8);
    let h = if h % 12 == 0 { 12 } else { h % 12 };
    if h >= 10 {
        [b'1', b'0' + h % 10, b':', b'0' + m / 10, b'0' + m % 10, 0]
    } else {
        [b'0' + h, b':', b'0' + m / 10, b'0' + m % 10, 0, 0]
    }
}

/// A GPS level from the fix quality and satellites in use: 0 looking, 1-3 bars.
pub fn bars(fix: u8, sats: u8) -> u8 {
    match (fix, sats) {
        (0, _) => 0,
        (_, 8..) => 3,
        (_, 5..) => 2,
        _ => 1,
    }
}
