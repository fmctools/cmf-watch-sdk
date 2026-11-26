// "Apps" entry in the stock app list of the Watch 3 Pro (jx402_08 1.0.1.77), in the List and in the Grid style; a
// tap opens the stub's view (`model::WP3.menu_view`, menu.rs).
//
// Adapted from jimhooker's cmf-watch3-port-notes `patches/wp3-applist.patch` (MIT, Copyright (c) 2026 jimhooker),
// which ran on a real watch; addresses and offsets now come from `wp2_core::model::WP3`.
//
// Both hooks replace a style descriptor's update handler (`applist_hook_slot`, `applist_grid_hook_slot`). The stock
// view procedure calls it right after the layout, and the stock update rebuilds the screen through a direct call to
// the layout when an optional entry appears or goes, so the layout slot alone would lose the entry. No writable
// statics: the entry is there when the container has one child more than the stock entries shown.
//
// The entry keeps its own tap path (an event callback on our object): the stock list treats tap targets >= 0x118 as
// function pointers, so our view id cannot go into the stock entry table.
use core::mem::transmute;
use wp2_core::model::WP3 as M;

const fn some(v: Option<u32>) -> u32 {
    match v {
        Some(v) => v,
        None => panic!("WP3 model value missing"),
    }
}

const LIST_UPDATE: u32 = some(M.applist_update);
const GRID_UPDATE: u32 = some(M.applist_grid_update);
const GRID_PLACE: u32 = some(M.applist_grid_place);
const LABEL_SET_STRING_DSC: u32 = some(M.label_set_string_dsc);
const LABEL_SCROLL_SETUP: u32 = some(M.label_scroll_setup);
const IMG_SET_PIVOT: u32 = some(M.simple_img_set_pivot);

const VIEW_DATA: usize = some(M.applist_view_data) as usize;
const FLAGS: usize = some(M.applist_data_flags) as usize;
const LIST_CONT: usize = M.applist_data_cont as usize;
const LIST_COUNT: usize = M.applist_data_count as usize;
const LIST_STYLE: usize = M.applist_data_label_style as usize;
const LIST_LINE_HEIGHT: usize = M.applist_data_line_h as usize;
const GRID_OBJS: usize = some(M.applist_grid_objs) as usize;
const GRID_CONT: usize = some(M.applist_grid_cont) as usize;
const GRID_COUNT: usize = some(M.applist_grid_count) as usize;
const GRID_LOWEST: usize = some(M.applist_grid_lowest) as usize;
const ENTRIES: u32 = M.applist_entries;
const LIST_ICON: usize = M.applist_list_icon as usize;
const GRID_ICON: usize = some(M.applist_grid_icon) as usize;

const SHORT_CLICKED: u32 = 4;
const SCROLL: u32 = 11;
const DELETE: u32 = 33;
const HIDDEN: u32 = 1;
const CLICKABLE: u32 = 2;
const EVENT_BUBBLE: u32 = 0x4000;
const ALIGN_TOP_LEFT: u32 = 1;
const OUT_BOTTOM_LEFT: u32 = 0xD;
const OUT_RIGHT_MID: u32 = 0x14;
const LONG_SCROLL_CIRCULAR: u32 = 3;

// Like the stock icons: LV_IMG_CF_TRUE_COLOR_ALPHA (RGB565 LE + alpha), a filled circle with a light glyph, behind
// the 64-byte header the stock resource cache puts before its pixels (the emulator's GPU model reads it).
const HEAD: usize = 64;
const CIRCLE: u16 = 0x18C3; // the stock icons' circle on this watch
const GLYPH: u16 = 0xF79E;

type Obj = *mut u8;

#[repr(C)]
struct ImgDsc {
    header: u32,
    size: u32,
    data: *const u8,
}

#[repr(C)]
struct TextRec {
    text: *const u8,
    zero: u32,
}

unsafe impl Sync for TextRec {}

static LABEL: TextRec = TextRec { text: b"Apps\0".as_ptr(), zero: 0 };

macro_rules! stock {
    ($addr:expr, fn($($t:ty),*) -> $r:ty) => {
        unsafe { transmute::<usize, extern "C" fn($($t),*) -> $r>($addr as usize) }
    };
}

pub extern "C" fn wp3_applist_list(view: *mut u8) {
    stock!(LIST_UPDATE, fn(*mut u8) -> ())(view);
    unsafe { add_row(view) };
}

pub extern "C" fn wp3_applist_grid(view: *mut u8) {
    stock!(GRID_UPDATE, fn(*mut u8) -> ())(view);
    unsafe { add_icon(view) };
}

/// The view data and its container when the stock layout has run and our entry is not there yet, with the number of
/// stock entries shown.
unsafe fn stock_state(view: *mut u8, cont: usize, count: usize) -> Option<(*mut u8, Obj, u32)> {
    let data = unsafe { *(view.add(VIEW_DATA) as *const *mut u8) };
    if data.is_null() || unsafe { *(data as *const Obj) }.is_null() {
        return None;
    }
    let cont = unsafe { *(data.add(cont) as *const Obj) };
    if cont.is_null() || u32::from(unsafe { *data.add(count) }) != ENTRIES {
        return None;
    }
    let shown = M.applist_always_shown + (unsafe { *data.add(FLAGS) } & 7).count_ones();
    (stock!(M.lv_obj_get_child_cnt, fn(Obj) -> u32)(cont) == shown).then_some((data, cont, shown))
}

// A row built like the stock rows (`applist_layout`), shown first; the stock fisheye walks every child.
unsafe fn add_row(view: *mut u8) {
    let Some((data, cont, shown)) = (unsafe { stock_state(view, LIST_CONT, LIST_COUNT) }) else { return };
    let Some((block, dsc)) = icon_block(LIST_ICON) else { return };
    let align_to = stock!(M.lv_obj_align_to, fn(Obj, Obj, u32, i32, i32) -> ());
    let add_flag = stock!(M.lv_obj_add_flag, fn(Obj, u32) -> ());
    let set_size = stock!(M.lv_obj_set_size, fn(Obj, i32, i32) -> ());
    let add_cb = stock!(M.lv_obj_add_event_cb, fn(Obj, extern "C" fn(*mut u8), u32, *mut u8) -> *mut u8);
    let row = stock!(M.lv_obj_create, fn(Obj) -> Obj)(cont);
    let img = if row.is_null() { row } else { stock!(M.simple_img_create, fn(Obj) -> Obj)(row) };
    if img.is_null() {
        if !row.is_null() {
            stock!(M.lv_obj_del, fn(Obj) -> ())(row); // no empty row in the list (the fisheye walks every child)
        }
        free(block);
        return;
    }
    add_cb(img, freed, DELETE, block);
    set_size(row, 0x18A, LIST_ICON as i32);
    add_cb(row, clicked, SHORT_CLICKED, core::ptr::null_mut());
    stock!(M.lv_obj_align, fn(Obj, u32, i32, i32) -> ())(row, ALIGN_TOP_LEFT, 0x10, 0x50);
    add_flag(img, EVENT_BUBBLE);
    stock!(M.simple_img_set_src, fn(Obj, *const ImgDsc) -> ())(img, dsc);
    let lab = stock!(M.lv_label_create, fn(Obj) -> Obj)(row);
    if !lab.is_null() {
        set_size(lab, 0x172 - LIST_ICON as i32, unsafe { *(data.add(LIST_LINE_HEIGHT) as *const i32) });
        stock!(M.obj_add_style, fn(Obj, *mut u8, u32) -> ())(lab, unsafe { data.add(LIST_STYLE) }, 0);
        stock!(M.lv_label_set_long_mode, fn(Obj, u32) -> ())(lab, LONG_SCROLL_CIRCULAR);
        stock!(LABEL_SET_STRING_DSC, fn(Obj, *const TextRec) -> ())(lab, &LABEL);
        stock!(LABEL_SCROLL_SETUP, fn(Obj, u32, i32, u32) -> ())(lab, 0, -1, 0);
        add_flag(lab, EVENT_BUBBLE);
        align_to(lab, img, OUT_RIGHT_MID, 0x18, 0);
    }
    let get_child = stock!(M.lv_obj_get_child, fn(Obj, i32) -> Obj);
    let mut prev = row;
    for i in 0..shown as i32 {
        let r = get_child(cont, i);
        align_to(r, prev, OUT_BOTTOM_LEFT, 0, 8);
        prev = r;
    }
    stock!(M.event_send, fn(Obj, u32, *mut u8) -> u32)(cont, SCROLL, core::ptr::null_mut());
}

// One more icon after the stock ones (`applist_grid_layout`): the stock position function takes the count from the
// container and the objects from a 30-pointer array, so the entry needs a stock entry to be hidden. Known limitation:
// with all three optional stock entries shown (27 + 3 = 30) the Grid has no Apps icon (the List still has its row).
// The array is full then and the word after it is the container pointer (`applist_grid_cont`), so a 31st object
// cannot go in, and the place function would read that word as the 31st object (docs/recon.md, "Apps in the Grid").
unsafe fn add_icon(view: *mut u8) {
    let Some((data, cont, shown)) = (unsafe { stock_state(view, GRID_CONT, GRID_COUNT) }) else { return };
    if shown >= ENTRIES {
        return;
    }
    let Some((block, dsc)) = icon_block(GRID_ICON) else { return };
    let add_flag = stock!(M.lv_obj_add_flag, fn(Obj, u32) -> ());
    let add_cb = stock!(M.lv_obj_add_event_cb, fn(Obj, extern "C" fn(*mut u8), u32, *mut u8) -> *mut u8);
    let img = stock!(M.simple_img_create, fn(Obj) -> Obj)(cont);
    if img.is_null() {
        free(block);
        return;
    }
    unsafe { *(data.add(GRID_OBJS + 4 * shown as usize) as *mut Obj) = img };
    add_cb(img, freed, DELETE, block);
    stock!(M.simple_img_set_src, fn(Obj, *const ImgDsc) -> ())(img, dsc);
    stock!(IMG_SET_PIVOT, fn(Obj, i32, i32) -> ())(img, GRID_ICON as i32 / 2, GRID_ICON as i32 / 2);
    add_flag(img, HIDDEN);
    add_cb(img, clicked, SHORT_CLICKED, core::ptr::null_mut());
    add_flag(img, CLICKABLE);
    add_flag(img, EVENT_BUBBLE);
    // three icons per row, GRID_ICON px apart
    let pitch = GRID_ICON as i32;
    unsafe { *(data.add(GRID_LOWEST) as *mut i32) = 480 - pitch * ((shown as i32 + 3) / 3) };
    stock!(GRID_PLACE, fn(*mut u8) -> ())(data);
}

/// The icon in a RES block: [64 B header][size x size pixels][ImgDsc]. Freed with `free(block)`.
fn icon_block(size: usize) -> Option<(*mut u8, *const ImgDsc)> {
    let pixels_len = size * size * 3;
    let total = HEAD + pixels_len + size_of::<ImgDsc>();
    let block = crate::alloc(total as u32);
    if block.is_null() {
        return None;
    }
    let (pixels, dsc) = unsafe { (block.add(HEAD), block.add(HEAD + pixels_len) as *mut ImgDsc) };
    unsafe {
        let head = core::slice::from_raw_parts_mut(block as *mut u32, HEAD / 4);
        head.fill(0);
        (head[2], head[4], head[5], head[6], head[8]) = ((HEAD + pixels_len) as u32, size as u32, size as u32, 3, pixels as u32);
        draw_icon(core::slice::from_raw_parts_mut(pixels, pixels_len), size);
        dsc.write(ImgDsc { header: 5 | (size as u32) << 10 | (size as u32) << 21, size: pixels_len as u32, data: pixels });
    }
    // pixels written through the cached PSRAM mapping must reach PSRAM before the display reads them
    unsafe { core::arch::asm!("dsb sy", options(nostack)) };
    stock!(M.psram_cache_flush_invalidate, fn(*mut u8, u32) -> i32)(block, total as u32);
    unsafe { core::arch::asm!("dsb sy", "isb sy", options(nostack)) };
    Some((block, dsc))
}

fn free(block: *mut u8) {
    crate::free(block);
}

extern "C" fn freed(e: *mut u8) {
    free(stock!(M.lv_event_get_user_data, fn(*mut u8) -> *mut u8)(e));
}

extern "C" fn clicked(_e: *mut u8) {
    stock!(M.view_open, fn(u32, u32) -> i32)(M.menu_view, 0);
}

// Own artwork (as on the Watch Pro 2 stub): a filled circle with a 2x2 grid of rounded squares, antialiased;
// distances in 1/16 px, sizes given for a 96 px icon and scaled.
fn draw_icon(out: &mut [u8], size: usize) {
    let scale = |px: i32| px * 16 * size as i32 / 96;
    let (c, sq, gap, r) = (size as i32 * 8, scale(17), scale(6), scale(5));
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as i32 * 16 + 8 - c, y as i32 * 16 + 8 - c);
            let circle = cover(isqrt(px * px + py * py) - c);
            // each square's centre is (sq + gap) / 2 from the icon centre on both axes
            let (qx, qy) = (px.abs() - (sq + gap) / 2, py.abs() - (sq + gap) / 2);
            let (qx, qy) = (qx.abs() - (sq / 2 - r), qy.abs() - (sq / 2 - r));
            let glyph = cover(isqrt(qx.max(0).pow(2) + qy.max(0).pow(2)) + qx.max(qy).min(0) - r);
            let colour = mix(CIRCLE, GLYPH, glyph);
            let i = (y * size + x) * 3;
            out[i..i + 3].copy_from_slice(&[colour as u8, (colour >> 8) as u8, (circle * 255 / 16) as u8]);
        }
    }
}

// 0..=16 of a pixel covered by a shape whose edge is `d` (1/16 px, negative inside) from the pixel centre
fn cover(d: i32) -> i32 {
    (8 - d).clamp(0, 16)
}

fn mix(a: u16, b: u16, t: i32) -> u16 {
    let ch = |shift: u32, mask: i32| {
        let (ca, cb) = ((a >> shift) as i32 & mask, (b >> shift) as i32 & mask);
        (((ca * (16 - t) + cb * t + 8) / 16) as u16) << shift
    };
    ch(11, 31) | ch(5, 63) | ch(0, 31)
}

fn isqrt(n: i32) -> i32 {
    let mut x = n as u32;
    let mut r = 0u32;
    let mut bit = 1u32 << 30;
    while bit > x {
        bit >>= 2;
    }
    while bit != 0 {
        if x >= r + bit {
            x -= r + bit;
            r = (r >> 1) + bit;
        } else {
            r >>= 1;
        }
        bit >>= 2;
    }
    r as i32
}
