// The stub's own view (`model::WP3.menu_view`, 0x147): a titled "Apps" screen with an empty-state line. Prototype
// for milestone M1 of the loader port: no loader is loaded yet, the view only proves the id, the 20-byte view-table
// record `wp2apps firmware --model wp3` writes for it, and the way back to the app list.
//
// Messages (`model::WP3.view_msg_*`, emulator recon): PRELOAD asks the stock for a LAYOUT, LAYOUT builds on the
// active screen, DELETE frees what LAYOUT made. Keys are left to the stock, like goodbye_view (also a proc-style
// record) does: back (right swipe, 0xFFFF0071) pops the view stack to the app list, a crown press goes to the watch
// face, crown turns (0x1E next, 0x1F previous) have nothing to scroll here.
//
// No writable statics: the state lives in a RES block whose pointer the view keeps at `data + VIEW_USER` (the word
// goodbye_view keeps its own block in).
use core::ffi::c_void;
use core::mem::transmute;
use core::ptr::null_mut;
use wp2_core::model::WP3 as M;

const fn some(v: Option<u32>) -> u32 {
    match v {
        Some(v) => v,
        None => panic!("WP3 model value missing"),
    }
}

const PRELOAD: u32 = some(M.view_msg_preload);
const LAYOUT: u32 = some(M.view_msg_layout);
const DELETE: u32 = some(M.view_msg_delete);
// own (goodbye_view 0x1004A3E1: `str r5, [r7, #0xc]` at layout, `ldr r4, [r2, #0xc]` at delete)
const VIEW_USER: usize = 0xC;

const LV_STYLE_BG_COLOR: u32 = 0x20;
const LV_STYLE_BG_OPA: u32 = 0x21;
const LV_STYLE_TEXT_COLOR: u32 = 0x55;
const LV_STYLE_TEXT_FONT: u32 = 0x57;
const LV_STYLE_TEXT_ALIGN: u32 = 0x5B;
const TEXT_ALIGN_CENTER: u32 = 2;
const LONG_DOT: u32 = 1;
const BLACK: u32 = 0x0000;
const WHITE: u32 = 0xFFFF;
const GREY: u32 = 0x8C92;
const RED: u32 = 0xFA23;
const SCREEN: i32 = 466;
const TITLE_Y: i32 = 56;
const TEXT_X: i32 = 48;

type Obj = *mut c_void;

#[repr(C)]
struct State {
    /// lv_font_t of the stock font32M (LVGL 8.3's is 0x28 bytes; room to spare)
    font: [u32; 16],
    font_open: u32,
    cont: Obj,
}

macro_rules! stock {
    ($addr:expr, fn($($t:ty),*) -> $r:ty) => {
        unsafe { transmute::<usize, extern "C" fn($($t),*) -> $r>($addr as usize) }
    };
}

pub static NAME: [u8; 13] = *b"wp3apps_view\0";

pub extern "C" fn wp3_view_proc(view_id: u32, msg: u32, data: *mut u8) -> i32 {
    match msg {
        PRELOAD => {
            // a new instance: the word may hold anything until our LAYOUT fills it
            if !data.is_null() {
                unsafe { *(data.add(VIEW_USER) as *mut *mut State) = null_mut() };
            }
            stock!(M.view_layout, fn(u32) -> i32)(view_id)
        }
        LAYOUT if !data.is_null() => {
            unsafe { layout(data) };
            0
        }
        DELETE if !data.is_null() => {
            unsafe { delete(data) };
            0
        }
        _ => 0,
    }
}

unsafe fn layout(data: *mut u8) {
    let slot = unsafe { data.add(VIEW_USER) as *mut *mut State };
    if !unsafe { *slot }.is_null() {
        unsafe { delete(data) }; // a second LAYOUT without DELETE
    }
    let scr = stock!(M.lv_disp_get_scr_act, fn(*mut c_void) -> Obj)(unsafe { *(data as *const *mut c_void) });
    if scr.is_null() {
        return;
    }
    let st = crate::alloc(size_of::<State>() as u32) as *mut State;
    if st.is_null() {
        return;
    }
    let s = unsafe { &mut *st };
    s.font = [0; 16];
    s.font_open = (stock!(M.lvgl_bitmap_font_open, fn(*mut u32, u32) -> i32)(s.font.as_mut_ptr(), M.font_32m) == 0) as u32;
    let font = if s.font_open != 0 { s.font.as_ptr() as u32 } else { M.lv_font_default };
    let lh = unsafe { *(font as *const i32).add(2) }.clamp(1, 80);

    let cont = panel(scr, 0, 0, SCREEN, SCREEN, BLACK);
    s.cont = cont;
    unsafe { *slot = st };
    if cont.is_null() {
        return;
    }
    style(cont, LV_STYLE_TEXT_FONT, font);
    style(cont, LV_STYLE_TEXT_COLOR, WHITE);
    let title = label(cont, 0, TITLE_Y, SCREEN, lh, b"Apps\0");
    style(title, LV_STYLE_TEXT_ALIGN, TEXT_ALIGN_CENTER);
    style(title, LV_STYLE_TEXT_COLOR, RED);
    let empty = label(cont, TEXT_X, (SCREEN - lh) / 2, SCREEN - 2 * TEXT_X, lh, b"No apps installed\0");
    style(empty, LV_STYLE_TEXT_ALIGN, TEXT_ALIGN_CENTER);
    style(empty, LV_STYLE_TEXT_COLOR, GREY);
}

// Runs before the stock tears the scene down (view delete order, docs/recon.md), so our objects go first.
unsafe fn delete(data: *mut u8) {
    let slot = unsafe { data.add(VIEW_USER) as *mut *mut State };
    let st = unsafe { *slot };
    if st.is_null() {
        return;
    }
    unsafe { *slot = null_mut() };
    let s = unsafe { &mut *st };
    if !s.cont.is_null() {
        stock!(M.lv_obj_del, fn(Obj) -> ())(s.cont);
    }
    if s.font_open != 0 {
        stock!(M.lvgl_bitmap_font_close, fn(*mut u32) -> ())(s.font.as_mut_ptr());
    }
    crate::free(st as *mut u8);
}

fn style(obj: Obj, prop: u32, value: u32) {
    if !obj.is_null() {
        stock!(M.lv_obj_set_local_style_prop, fn(Obj, u32, u32, u32) -> ())(obj, prop, value, 0);
    }
}

fn place(obj: Obj, x: i32, y: i32, w: i32, h: i32) -> Obj {
    if !obj.is_null() {
        stock!(M.lv_obj_set_pos, fn(Obj, i32, i32) -> ())(obj, x, y);
        stock!(M.lv_obj_set_size, fn(Obj, i32, i32) -> ())(obj, w, h);
    }
    obj
}

fn panel(parent: Obj, x: i32, y: i32, w: i32, h: i32, color: u32) -> Obj {
    let o = place(stock!(M.lv_obj_create, fn(Obj) -> Obj)(parent), x, y, w, h);
    style(o, LV_STYLE_BG_COLOR, color);
    style(o, LV_STYLE_BG_OPA, 0xFF);
    o
}

fn label(parent: Obj, x: i32, y: i32, w: i32, h: i32, text: &'static [u8]) -> Obj {
    let l = stock!(M.lv_label_create, fn(Obj) -> Obj)(parent);
    if l.is_null() {
        return l;
    }
    stock!(M.lv_label_set_long_mode, fn(Obj, u32) -> ())(l, LONG_DOT);
    stock!(M.lv_label_set_text, fn(Obj, *const u8) -> ())(l, text.as_ptr());
    place(l, x, y, w, h)
}
