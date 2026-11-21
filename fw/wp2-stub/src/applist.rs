// "Custom Apps" row at the top of the stock app list (list style), built like the stock rows (docs/recon.md).
use core::mem::transmute;
use wp2_core::heap::{self, RES};
use wp2_core::load::sync_psram;

const STOCK_LAYOUT: u32 = 0x1005EEE5;
const OBJ_CREATE: u32 = 0x10127CFD;
const OBJ_GET_CHILD: u32 = 0x1012C3DD;
const OBJ_GET_CHILD_CNT: u32 = 0x1012C4AD;
const OBJ_GET_Y: u32 = 0x101298B9;
const OBJ_GET_Y2: u32 = 0x10129B05;
const OBJ_GET_SCROLL_Y: u32 = 0x101C6515;
const OBJ_SET_SIZE: u32 = 0x101294C9;
const OBJ_SET_X: u32 = 0x101290CD;
const OBJ_SET_OPA: u32 = 0x101C74C5;
const OBJ_ALIGN: u32 = 0x101C5DE5;
const OBJ_ALIGN_TO: u32 = 0x1012A87D;
const OBJ_ADD_FLAG: u32 = 0x10128D39;
const OBJ_ADD_EVENT_CB: u32 = 0x10125A95;
const OBJ_ADD_STYLE: u32 = 0x1012BB15;
const EVENT_SEND: u32 = 0x101259C5;
const EVENT_GET_USER_DATA: u32 = 0x101C4CA7;
const VIMG_CREATE: u32 = 0x1005A015;
const VIMG_SET_SRC: u32 = 0x1005A02D;
const VIMG_SET_ZOOM: u32 = 0x1005A2D1;
const VLABEL_CREATE: u32 = 0x1005B579;
const VLABEL_SET_LONG_MODE: u32 = 0x101ACBA5;
const VLABEL_SET_TEXT: u32 = 0x1005B70D;
const OPEN_VIEW: u32 = 0x101898BD;

const ROWS: u8 = 25;
const SHORT_CLICKED: u32 = 4;
const SCROLL: u32 = 11;
const DELETE: u32 = 33;
const EVENT_BUBBLE: u32 = 0x4000;
const ALIGN_TOP_LEFT: u32 = 1;
const OUT_BOTTOM_LEFT: u32 = 0xD;
const OUT_RIGHT_MID: u32 = 0x14;

// Stock icons: LV_IMG_CF_TRUE_COLOR_ALPHA 96x96 (RGB565 LE + alpha), a filled circle with a light glyph.
pub(crate) const ICON: usize = 96;
// The emulator's GPU model reads 3-byte pixels only behind this header (cmf-watch-utils acts_vg_resource_packed);
// the firmware programs the GPU the same way for stock icons and ours.
const HEAD: usize = 64;
const CIRCLE: u16 = 0x2966;
const GLYPH: u16 = 0xF79E;

type Obj = *mut u8;

#[repr(C)]
pub(crate) struct ImgDsc {
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

static LABEL: TextRec = TextRec { text: b"Custom Apps\0".as_ptr(), zero: 0 };

macro_rules! stock {
    ($addr:expr, fn($($t:ty),*) -> $r:ty) => {
        unsafe { transmute::<usize, extern "C" fn($($t),*) -> $r>($addr as usize) }
    };
}

pub extern "C" fn wp2_applist_layout(view: *mut u8) -> i32 {
    let rc = stock!(STOCK_LAYOUT, fn(*mut u8) -> i32)(view);
    if rc == 0 {
        unsafe { add_row(*(view.add(0x14) as *const *mut u8)) };
    }
    rc
}

unsafe fn add_row(data: *mut u8) {
    let cont = unsafe { *(data.add(4) as *const Obj) };
    if cont.is_null() || unsafe { *data.add(8) } != ROWS || stock!(OBJ_GET_CHILD_CNT, fn(Obj) -> u32)(cont) != ROWS as u32 {
        return;
    }
    let Some((block, dsc)) = icon_block() else { return };

    // The row stays the last child: the stock fisheye walks children 0..24 and must keep seeing the stock rows.
    let get_child = stock!(OBJ_GET_CHILD, fn(Obj, i32) -> Obj);
    let align_to = stock!(OBJ_ALIGN_TO, fn(Obj, Obj, u32, i32, i32) -> ());
    let add_flag = stock!(OBJ_ADD_FLAG, fn(Obj, u32) -> ());
    let set_size = stock!(OBJ_SET_SIZE, fn(Obj, i32, i32) -> ());
    let add_cb = stock!(OBJ_ADD_EVENT_CB, fn(Obj, extern "C" fn(*mut u8), u32, *mut u8) -> *mut u8);
    let row = stock!(OBJ_CREATE, fn(Obj) -> Obj)(cont);
    let img = if row.is_null() { row } else { stock!(VIMG_CREATE, fn(Obj) -> Obj)(row) };
    if img.is_null() {
        unsafe { heap::free(RES, block) };
        return;
    }
    add_cb(img, freed, DELETE, block);
    set_size(row, 0x18A, ICON as i32);
    add_cb(row, clicked, SHORT_CLICKED, core::ptr::null_mut());
    stock!(OBJ_ALIGN, fn(Obj, u32, i32, i32) -> ())(row, ALIGN_TOP_LEFT, 0x10, 0x50);
    add_flag(img, EVENT_BUBBLE);
    stock!(VIMG_SET_SRC, fn(Obj, *const ImgDsc) -> ())(img, dsc);
    let lab = stock!(VLABEL_CREATE, fn(Obj) -> Obj)(row);
    set_size(lab, 0x172 - ICON as i32, unsafe { *(data.add(0x214) as *const i32) });
    stock!(OBJ_ADD_STYLE, fn(Obj, *mut u8, u32) -> ())(lab, unsafe { data.add(0x200) }, 0);
    stock!(VLABEL_SET_LONG_MODE, fn(Obj, u32) -> ())(lab, 3);
    stock!(VLABEL_SET_TEXT, fn(Obj, *const TextRec) -> ())(lab, &LABEL);
    add_flag(lab, EVENT_BUBBLE);
    align_to(lab, img, OUT_RIGHT_MID, 0x18, 0);

    let mut prev = row;
    for i in 0..ROWS as i32 {
        let r = get_child(cont, i);
        align_to(r, prev, OUT_BOTTOM_LEFT, 0, 8);
        prev = r;
    }
    add_cb(cont, fisheye, SCROLL, row);
    stock!(EVENT_SEND, fn(Obj, u32, *mut u8) -> u32)(cont, SCROLL, core::ptr::null_mut());
}

/// The Custom Apps icon in a RES block (`image_block`). Freed with `heap::free(RES, block)`.
pub(crate) fn icon_block() -> Option<(*mut u8, *const ImgDsc)> {
    image_block(ICON, |px| {
        draw_icon(px);
        true
    })
}

/// A `side`² CF 5 image in a RES block: [64 B header like the stock resource cache's][pixels, 64-byte aligned like
/// the stock icons][ImgDsc]; `fill` writes the pixels, false frees the block. Freed with `heap::free(RES, block)`.
pub(crate) fn image_block(side: usize, fill: impl FnOnce(&mut [u8]) -> bool) -> Option<(*mut u8, *const ImgDsc)> {
    let pixels = side * side * 3;
    let size = HEAD + pixels + size_of::<ImgDsc>();
    let block = heap::alloc(RES, size as u32, 0x40);
    if block.is_null() {
        return None;
    }
    let (px, dsc) = unsafe { (block.add(HEAD), block.add(HEAD + pixels) as *mut ImgDsc) };
    if !fill(unsafe { core::slice::from_raw_parts_mut(px, pixels) }) {
        unsafe { heap::free(RES, block) };
        return None;
    }
    unsafe {
        let head = core::slice::from_raw_parts_mut(block as *mut u32, HEAD / 4);
        head.fill(0);
        (head[2], head[4], head[5], head[6], head[8]) = ((HEAD + pixels) as u32, side as u32, side as u32, 3, px as u32);
        dsc.write(ImgDsc { header: 5 | (side as u32) << 10 | (side as u32) << 21, size: pixels as u32, data: px });
    }
    sync_psram(block, size as u32);
    Some((block, dsc))
}

// The stock fisheye (0x1005ED98) for our row: x shift, icon zoom and label opacity by distance from the centre.
extern "C" fn fisheye(e: *mut u8) {
    let row = stock!(EVENT_GET_USER_DATA, fn(*mut u8) -> Obj)(e);
    let cont = unsafe { *(row.add(4) as *const Obj) };
    let get_child = stock!(OBJ_GET_CHILD, fn(Obj, i32) -> Obj);
    let mid = (stock!(OBJ_GET_Y, fn(Obj) -> i32)(row) + stock!(OBJ_GET_Y2, fn(Obj) -> i32)(row)) >> 1;
    let d = (mid - 233 - stock!(OBJ_GET_SCROLL_Y, fn(Obj) -> i32)(cont)).abs();
    let Some((x, zoom, opa)) = shape(d) else { return };
    stock!(VIMG_SET_ZOOM, fn(Obj, u32) -> ())(get_child(row, 0), zoom);
    stock!(OBJ_SET_OPA, fn(Obj, u32, u32) -> ())(get_child(row, 1), opa, 0);
    stock!(OBJ_SET_X, fn(Obj, i32) -> ())(row, x);
}

fn shape(d: i32) -> Option<(i32, u32, u32)> {
    match d {
        0..104 => Some((d * 4 / 13 + 16, 0x100, 0xFF)),
        104..281 => Some(((d - 104) * 25 / 48 + 48, (256 - (90 * d - 9360) / 177) as u32, (255 - (180 * d - 18720) / 177) as u32)),
        _ => None,
    }
}

extern "C" fn freed(e: *mut u8) {
    let block = stock!(EVENT_GET_USER_DATA, fn(*mut u8) -> *mut u8)(e);
    unsafe { heap::free(RES, block) };
}

extern "C" fn clicked(_e: *mut u8) {
    stock!(OPEN_VIEW, fn(u32, u32) -> i32)(0xF0, 0);
}

// Own artwork: a filled circle with a 2x2 grid of rounded squares, antialiased; distances in 1/16 px.
fn draw_icon(out: &mut [u8]) {
    const C: i32 = ICON as i32 * 8;
    const SQ: i32 = 17 * 16;
    const GAP: i32 = 6 * 16;
    const R: i32 = 5 * 16;
    for y in 0..ICON {
        for x in 0..ICON {
            let (px, py) = (x as i32 * 16 + 8 - C, y as i32 * 16 + 8 - C);
            let circle = cover(isqrt(px * px + py * py) - C);
            // each square's centre is (SQ + GAP) / 2 from the icon centre on both axes
            let (qx, qy) = (px.abs() - (SQ + GAP) / 2, py.abs() - (SQ + GAP) / 2);
            let (qx, qy) = (qx.abs() - (SQ / 2 - R), qy.abs() - (SQ / 2 - R));
            let glyph = cover(isqrt(qx.max(0).pow(2) + qy.max(0).pow(2)) + qx.max(qy).min(0) - R);
            let c = mix(CIRCLE, GLYPH, glyph);
            let i = (y * ICON + x) * 3;
            out[i..i + 3].copy_from_slice(&[c as u8, (c >> 8) as u8, (circle * 255 / 16) as u8]);
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
