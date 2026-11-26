// The stub's own view (`model::WP3.menu_view`, 0x147). On PRELOAD it loads the loader package (slot L) from NAND
// into a RES block, relocates it and hands it every message from then on, as the WP2 stub's view.rs does (the loader
// draws the installed-apps menu and runs apps). Without a loader that loads (none installed, another model's, out of
// memory) the view stays the placeholder below: a titled "Apps" screen with a line saying why.
//
// Messages (`model::WP3.view_msg_*`, emulator recon): PRELOAD asks the stock for a LAYOUT, LAYOUT builds on the
// active screen, DELETE frees what LAYOUT made. Keys are left to the stock, like goodbye_view (also a proc-style
// record) does: back (right swipe, 0xFFFF0071) pops the view stack to the app list, a crown press goes to the watch
// face, crown turns (0x1E next, 0x1F previous) have nothing to scroll here.
//
// No writable statics: the placeholder's state lives in a RES block whose pointer StubRam keeps in `loader_entry`,
// a word that means something else only while a loader is loaded (`State` below). Not the view's user word
// (`data + 0xC`, where goodbye_view keeps its block): the reload at FOCUS runs PRELOAD without the view data, so
// that word could not be cleared for a new instance on every path.
use core::ffi::c_void;
use core::mem::transmute;
use core::ptr::null_mut;
use wp2_core::heap::RES;
use wp2_core::layout::{StubRam, E_NOMEM, LOADER_SLOT};
use wp2_core::model::WP3 as M;
use wp2_core::port;

const fn some(v: Option<u32>) -> u32 {
    match v {
        Some(v) => v,
        None => panic!("WP3 model value missing"),
    }
}

const PRELOAD: u32 = some(M.view_msg_preload);
const LAYOUT: u32 = some(M.view_msg_layout);
const DELETE: u32 = some(M.view_msg_delete);

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

const FOCUS: u32 = some(M.view_msg_focus);
const DEFOCUS: u32 = some(M.view_msg_defocus);
const REFUSE: i32 = -2;
// StubRam.loader_state
const LOADED: u8 = 1;
const NO_LOADER: u8 = 2;
const NO_MEMORY: u8 = 3;

type Proc = extern "C" fn(u32, u32, *mut c_void) -> i32;

pub extern "C" fn wp3_view_proc(view_id: u32, msg: u32, data: *mut u8) -> i32 {
    let ram = crate::ble::ram();
    ram.trace[ram.trace_head as usize % ram.trace.len()] = (msg << 16) | view_id;
    ram.trace_head = ram.trace_head.wrapping_add(1);
    if msg != PRELOAD && ram.loader_state == LOADED && u32::from(ram.loader_view) != view_id {
        return 0; // a view whose instance another view took over
    }
    if msg == FOCUS || msg == DEFOCUS {
        ram.loader_focus = (msg == FOCUS) as u8;
    }
    if msg == FOCUS && ram.reload != 0 && ram.loader_state == LOADED && ram.running_slot == 0xFF {
        // a new loader landed while this one was loaded (an install begun before the menu opened, ble.rs `end`): load
        // it as a PRELOAD would, but never under a running app (that waits for a FOCUS after the app ends)
        if wp3_view_proc(view_id, PRELOAD, null_mut()) < 0 {
            return REFUSE;
        }
    }
    if msg == PRELOAD {
        ram.reload = 0;
        ram.loader_view = view_id as u16;
        if ram.loader_state == LOADED {
            loader(ram)(view_id, DELETE, null_mut()); // a second PRELOAD without DELETE
        } else {
            unsafe { delete(ram) }; // a placeholder left by an instance without DELETE
        }
        release(ram);
        match port::load(&M, LOADER_SLOT, &mut ram.load_file, &mut ram.load_scratch) {
            Ok((block, h)) => {
                ram.loader_block = block as u32;
                ram.loader_size = h.image_size;
                ram.loader_entry = h.entry;
                ram.loader_state = LOADED;
            }
            Err(e) => ram.loader_state = if e == E_NOMEM { NO_MEMORY } else { NO_LOADER },
        }
    }
    if ram.loader_state != LOADED {
        ram.launch = 0;
        return placeholder(view_id, msg, data, ram);
    }
    ram.loader_active = 1;
    let rc = loader(ram)(view_id, msg, data as *mut c_void);
    if msg == PRELOAD && rc < 0 {
        release(ram);
        ram.loader_state = NO_MEMORY;
        return placeholder(view_id, msg, data, ram);
    }
    if msg == DELETE {
        ram.loader_focus = 0;
        release(ram);
        ram.loader_state = 0;
    }
    rc
}

fn loader(ram: &StubRam) -> Proc {
    unsafe { transmute(((ram.loader_block + ram.loader_entry) | 1) as usize) }
}

// Every way out of LOADED goes through here, so `loader_entry` is 0 (no placeholder state) afterwards.
fn release(ram: &mut StubRam) {
    unsafe { port::free(&M, RES, ram.loader_block as *mut u8) };
    ram.loader_block = 0;
    ram.loader_entry = 0;
    ram.loader_active = 0;
}

// While no loader is loaded: the placeholder's `State` block, 0 = none (`delete` checks the state first).
fn state(ram: &mut StubRam) -> &mut u32 {
    &mut ram.loader_entry
}

fn placeholder(view_id: u32, msg: u32, data: *mut u8, ram: &mut StubRam) -> i32 {
    match msg {
        PRELOAD => stock!(M.view_layout, fn(u32) -> i32)(view_id),
        LAYOUT if !data.is_null() => {
            unsafe { layout(data, ram) };
            0
        }
        DELETE => {
            unsafe { delete(ram) };
            0
        }
        _ => 0,
    }
}

unsafe fn layout(data: *mut u8, ram: &mut StubRam) {
    unsafe { delete(ram) }; // a second LAYOUT without DELETE
    let reason = ram.loader_state;
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
    *state(ram) = st as u32;
    if cont.is_null() {
        return;
    }
    style(cont, LV_STYLE_TEXT_FONT, font);
    style(cont, LV_STYLE_TEXT_COLOR, WHITE);
    let title = label(cont, 0, TITLE_Y, SCREEN, lh, b"Apps\0");
    style(title, LV_STYLE_TEXT_ALIGN, TEXT_ALIGN_CENTER);
    style(title, LV_STYLE_TEXT_COLOR, RED);
    let why: &'static [u8] = if reason == NO_MEMORY { b"Not enough memory\0" } else { b"No loader installed\0" };
    let empty = label(cont, TEXT_X, (SCREEN - lh) / 2, SCREEN - 2 * TEXT_X, lh, why);
    style(empty, LV_STYLE_TEXT_ALIGN, TEXT_ALIGN_CENTER);
    style(empty, LV_STYLE_TEXT_COLOR, GREY);
}

// Runs before the stock tears the scene down (view delete order, docs/recon.md), so our objects go first.
unsafe fn delete(ram: &mut StubRam) {
    if ram.loader_state == LOADED {
        return; // loader_entry is the loader's entry offset
    }
    let st = core::mem::replace(state(ram), 0) as *mut State;
    if st.is_null() {
        return;
    }
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
