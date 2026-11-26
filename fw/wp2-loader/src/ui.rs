use core::ffi::c_void;
use core::ptr::{addr_of_mut, null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};
use wp2_core::gfx::{self, Canvas, H, W};
use crate::plat::heap::{self, FB};
use wp2_core::layout::E_NOMEM;
use crate::plat::stock::*;

// LV_COLOR_DEPTH 16, LV_COLOR_16_SWAP 0 (docs/recon.md)
pub const FB_BYTES: u32 = (W * H * 2) as u32;
const CF_TRUE_COLOR: u32 = 4;
const FLAG_HIDDEN: u32 = 1;
const FLAG_CLICKABLE: u32 = 2;
pub const PRESSED: u32 = 1;
pub const RELEASED: u32 = 8;
// vendor event: param points to the i32 crown step
pub const KNOB: u32 = 0x2D;

pub type Cb = extern "C" fn(*mut c_void);
type Obj = *mut c_void;

// gfx dirty box of the app's draw calls since its last present(); app thread only
pub static PENDING: AtomicU32 = AtomicU32::new(gfx::EMPTY);
// boxes present() (or launch: FULL) published since the tick took the last one
pub static PUBLISHED: AtomicU32 = AtomicU32::new(gfx::EMPTY);

// e2e counters (tests/test_demo_e2e.py): presents, flushes, flushed bytes, invalidated pixels
#[repr(C)]
pub struct FlushStats([AtomicU32; 4]);
pub static FLUSH_STATS: FlushStats = FlushStats([const { AtomicU32::new(0) }; 4]);

fn count(i: usize, n: u32) {
    FLUSH_STATS.0[i].fetch_add(n, Ordering::Relaxed);
}

pub fn count_present() {
    count(0, 1);
}

#[repr(C)]
struct ImgDsc {
    header: u32,
    data_size: u32,
    data: *const u8,
}

struct Ui {
    fb: *mut u8,
    scr: Obj,
    input: Option<Cb>,
    img: Obj,
    timer: *mut c_void,
    dsc: ImgDsc,
}

static mut UI: Ui = Ui { fb: null_mut(), scr: null_mut(), input: None, img: null_mut(), timer: null_mut(), dsc: ImgDsc { header: 0, data_size: 0, data: null() } };

fn ui() -> &'static mut Ui {
    unsafe { &mut *addr_of_mut!(UI) }
}

pub fn preload(view_id: u32) -> i32 {
    let fb = heap::alloc(FB, FB_BYTES, 0x40);
    if fb.is_null() {
        return E_NOMEM;
    }
    let rc = stock!(VIEW_LAYOUT, fn(u32) -> i32)(view_id);
    if rc < 0 {
        unsafe { heap::free(FB, fb) };
        return rc;
    }
    ui().fb = fb;
    rc
}

// returns the screen; the image starts hidden under the menu
pub fn layout(data: *mut c_void, input: Cb, tick: Cb) -> Obj {
    let u = ui();
    let scr = stock!(LV_DISP_GET_SCR_ACT, fn(*mut c_void) -> Obj)(unsafe { *(data as *const *mut c_void) });
    u.dsc = ImgDsc { header: CF_TRUE_COLOR | (W as u32) << 10 | (H as u32) << 21, data_size: FB_BYTES, data: u.fb };
    u.img = stock!(LV_IMG_CREATE, fn(Obj) -> Obj)(scr);
    stock!(LV_IMG_SET_SRC, fn(Obj, *const ImgDsc))(u.img, &u.dsc);
    stock!(LV_OBJ_ADD_FLAG, fn(Obj, u32))(u.img, FLAG_CLICKABLE | FLAG_HIDDEN);
    // a KNOB filter also registers the image as the display's knob object (crown events), docs/recon.md
    for filter in [PRESSED, RELEASED, KNOB] {
        stock!(LV_OBJ_ADD_EVENT_CB, fn(Obj, Cb, u32, *mut c_void) -> *mut c_void)(u.img, input, filter, null_mut());
    }
    // 10 ms: at 20 ms, 4-10 % of agent-glow's presents (33 ms frames) were merged into the next flush
    u.timer = stock!(LV_TIMER_CREATE, fn(Cb, u32, *mut c_void) -> *mut c_void)(tick, 10, null_mut());
    (u.scr, u.input) = (scr, Some(input));
    scr
}

pub fn screen() -> Obj {
    ui().scr
}

// touch and crown of the image, also on the LVGL root; set by layout() before any app runs
pub fn input() -> Cb {
    ui().input.unwrap()
}

// shown: touch and crown go to the image (the app), or to the LVGL root once the app made one (the image stays hidden)
pub fn show(on: bool) {
    let (img, root) = (ui().img, crate::lv::root());
    if img.is_null() {
        return;
    }
    let obj = if root.is_null() {
        img
    } else {
        stock!(LV_OBJ_ADD_FLAG, fn(Obj, u32))(img, FLAG_HIDDEN);
        root
    };
    if on {
        stock!(LV_OBJ_CLEAR_FLAG, fn(Obj, u32))(obj, FLAG_HIDDEN);
        stock!(LV_DISP_ADD_KNOB_OBJ, fn(Obj))(obj);
    } else {
        stock!(LV_OBJ_ADD_FLAG, fn(Obj, u32))(obj, FLAG_HIDDEN);
    }
}

// The loader block is freed right after this returns: LVGL must keep no pointer into it.
pub fn delete() {
    let u = ui();
    if !u.timer.is_null() {
        stock!(LV_TIMER_DEL, fn(*mut c_void))(u.timer);
    }
    if !u.img.is_null() {
        stock!(LV_OBJ_DEL, fn(Obj))(u.img);
    }
    unsafe { heap::free(FB, u.fb) };
    u.fb = null_mut();
    u.scr = null_mut();
    u.input = None;
    u.img = null_mut();
    u.timer = null_mut();
}

pub fn invalidate() {
    flush(gfx::FULL);
}

// Writes rows y0..=y1 of the box back from the PSRAM cache, then invalidates the box (the image is at 0, 0).
pub fn flush(b: u32) {
    let u = ui();
    let Some([x0, y0, x1, y1]) = gfx::area(b) else { return };
    if u.img.is_null() {
        return;
    }
    let len = ((y1 + 1 - y0) * W * 2) as u32;
    crate::plat::load::sync_psram(unsafe { u.fb.add(y0 * W * 2) }, len);
    let a = [x0, y0, x1, y1].map(|v| v as i32);
    stock!(LV_OBJ_INVALIDATE_AREA, fn(Obj, *const i32))(u.img, a.as_ptr());
    count(1, 1);
    count(2, len);
    count(3, ((x1 + 1 - x0) * (y1 + 1 - y0)) as u32);
}

// app thread: the part of the screen a draw call touched
pub fn mark(b: u32) {
    PENDING.store(gfx::merge(PENDING.load(Ordering::Relaxed), b), Ordering::Relaxed);
}

pub fn canvas() -> Canvas<'static> {
    Canvas { px: unsafe { core::slice::from_raw_parts_mut(ui().fb as *mut u16, W * H) } }
}

pub fn event_code(e: *mut c_void) -> u32 {
    stock!(LV_EVENT_GET_CODE, fn(*mut c_void) -> u32)(e)
}

pub fn knob_step(e: *mut c_void) -> i32 {
    let p = stock!(LV_EVENT_GET_PARAM, fn(*mut c_void) -> *const i32)(e);
    if p.is_null() { 0 } else { unsafe { *p } }
}

// For PRESSED/RELEASED the event parameter is the active indev.
pub fn touch_point(e: *mut c_void) -> (i32, i32) {
    let indev = stock!(LV_EVENT_GET_PARAM, fn(*mut c_void) -> *mut c_void)(e);
    let mut p = [-1i32; 2];
    stock!(LV_INDEV_GET_POINT, fn(*mut c_void, *mut i32))(indev, p.as_mut_ptr());
    (p[0], p[1])
}
