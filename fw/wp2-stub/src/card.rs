// "Apps" system widget for the cards left of the watch face (docs/recon.md, "Widget cards"). The patcher points
// the free widget registry slot 0x40 (wgt_reg 0x101EFC88) at WIDGET; a host puts it on a card with 0x906D
// (`wp2apps cards`). The stock card view lays the widget out, sizes and colours its container and calls `tap` on a
// click; the callbacks below only fill the container. They run on ui_service. param_id 0 is "Apps", N the app in
// slot N - 1 with its widget record (docs/app-guide.md, "Widget").
use core::mem::transmute;
use core::ptr::null_mut;
use wp2_core::fs::{self, File};
use wp2_core::heap::{self, RES};
use wp2_core::layout::{slot_path, stub_ram, widget_path, WIDGET_REC, WIDGET_TITLE};
use wp2_core::pkg::{file_size, parse_header, HEADER_SIZE};
use wp2_core::widget::{
    card, icon_at, trailer_head, Card, CAPTION, COLUMN, COLUMNS_AT, ICON_SIDE, NO_ICON, REC_MAX, TRAILER_HEAD,
};

use crate::applist::{icon_block, image_block, ImgDsc, ICON};

/// wgt_reg id; the stock table has NULL there
pub const ID: u8 = 0x40;
/// size class: index into the stock size table 0x101DF780 (0 circle 166, 1 square 166, 2 tall 166x344,
/// 3 wide 344x166, 4 full 344x344); 3 is the first style of every stock family
const WIDE: u8 = 3;
const W: i32 = 344;
const H: i32 = 166;
/// the loader view, opened by `tap`
const TAP_VIEW: u32 = 0xF0;

const OBJ_DEL: u32 = 0x1012C775;
const OBJ_ADD_FLAG: u32 = 0x10128D39;
const OBJ_SET_POS: u32 = 0x1012926D;
const OBJ_SET_SIZE: u32 = 0x101294C9;
const OBJ_SET_LOCAL_STYLE_PROP: u32 = 0x101C6F9B;
const LABEL_CREATE: u32 = 0x1013F5F1;
const LABEL_SET_TEXT: u32 = 0x101400A5;
const LABEL_SET_LONG_MODE: u32 = 0x1014051D;
const VIMG_CREATE: u32 = 0x1005A015;
const VIMG_SET_SRC: u32 = 0x1005A02D;
const FONT_OPEN: u32 = 0x10185C31;
const FONT_CLOSE: u32 = 0x10185D3D;
/// the stock widgets' font open (0x100DF9A0): FONT_OPEN, then line_height + 4 for font24M (docs/recon.md)
const STOCK_FONT_OPEN: u32 = 0x100DF9A1;

const STYLE_BG_COLOR: u32 = 0x20;
const STYLE_TEXT_COLOR: u32 = 0x55;
const STYLE_TEXT_FONT: u32 = 0x57;
const LONG_DOT: u32 = 1;
/// clicks on the icon and the text reach the container and its stock tap handler, as on the Custom Apps row
const EVENT_BUBBLE: u32 = 0x4000;
/// stock widget cards: the emulator frame reads (26, 28, 31) on them, as on the stock Settings row, whose RGB565 is
/// 5/11/6 (white reads 160 there, loader menu.rs ROW_BG). #1A1C1F taken literally (0x18E3) came out darker than a stock
/// widget next to it
const CARD_BG: u32 = 0x2966;
const WHITE: u32 = 0xFFFF;
/// an app widget's title, as the stock cards' grey captions
const GREY: u32 = 0x9CF3;
/// container user_data (lv_obj_t +0x10): the card entry's param_id, set by the stock layout before create
/// (emulator, docs/recon.md); 0 = "Apps", N = the app in slot N - 1
const USER_DATA: usize = 0x10;
const OPEN_VIEW: u32 = 0x101898BD;
const FONT_PATH: &[u8] = b"/NAND:A/font32M.font\0";
/// stock widget 97's caption and value fonts; font24M has `°` (97 draws `0°` in it)
const CAPTION_FONT: &[u8] = b"/NAND:A/font24L.font\0";
const VALUE_FONT: &[u8] = b"/NAND:A/font24M.font\0";
const TEXT: &[u8] = b"Apps\0";
const PAD: i32 = (H - ICON as i32) / 2;

type Obj = *mut u8;

macro_rules! stock {
    ($addr:expr, fn($($t:ty),*) -> $r:ty) => {
        unsafe { transmute::<usize, extern "C" fn($($t),*) -> $r>($addr as usize) }
    };
}

/// A wgt_reg entry (0x101F0784 and on): {u8 id, u8 size class, u16 state size, u32 resource scene, u32 tap action,
/// 5 callbacks}. The card view allocates `state` bytes (zeroed) per instance; without state it skips every callback
/// but create and the first update.
#[repr(C)]
pub struct WgtReg {
    id: u8,
    class: u8,
    state: u16,
    scene: u32,
    /// a view id ≤ 0xF4, or a function the stock click handler (0x101B1B14) tail-calls with r0 = the container
    tap: extern "C" fn(Obj),
    delete: extern "C" fn(*mut Ctx) -> i32,
    create: extern "C" fn(*mut Ctx) -> i32,
    /// right after create, then from the card's 1 s timer while the card view is on top
    update: extern "C" fn(*mut Ctx) -> i32,
    focus: extern "C" fn(*mut Ctx, u32) -> i32,
    reload: extern "C" fn(*mut Ctx, u32) -> i32,
}

unsafe impl Sync for WgtReg {}

/// The card view's per-widget record (0x1C bytes from view data + 0x10): the first 16 bytes are a copy of the
/// view's presenter header, then the container, the state and the widget id.
#[repr(C)]
pub struct Ctx {
    presenter: [u32; 4],
    obj: Obj,
    state: *mut State,
    id: u16,
}

/// Per card: up to two fonts and, for `columns`, 4 × (caption, icon, value) objects and 4 icon blocks.
#[repr(C)]
pub struct State {
    font: [[u32; 8]; 2],
    font_open: [u32; 2],
    objs: [Obj; 12],
    blocks: [*mut u8; 4],
}

pub static WIDGET: WgtReg = WgtReg {
    id: ID,
    class: WIDE,
    state: size_of::<State>() as u16,
    scene: 0,
    tap,
    delete,
    create,
    update,
    focus,
    reload,
};

fn style(obj: Obj, prop: u32, value: u32) {
    stock!(OBJ_SET_LOCAL_STYLE_PROP, fn(Obj, u32, u32, u32) -> ())(obj, prop, value, 0)
}

fn param(obj: Obj) -> u8 {
    unsafe { *obj.add(USER_DATA) }
}

/// `open`: FONT_OPEN, or STOCK_FONT_OPEN as the stock widgets (it adds their line spacing to font24M and others)
fn open_font(s: &mut State, i: usize, open: u32, path: &[u8]) -> *const u32 {
    s.font_open[i] = (stock!(open, fn(*mut u32, *const u8) -> i32)(s.font[i].as_mut_ptr(), path.as_ptr()) == 0) as u32;
    if s.font_open[i] != 0 { s.font[i].as_ptr() } else { core::ptr::null() }
}

#[allow(clippy::too_many_arguments)]
fn label(parent: Obj, font: *const u32, text: *const u8, color: u32, x: i32, y: i32, w: i32, h: i32) -> Obj {
    let label = stock!(LABEL_CREATE, fn(Obj) -> Obj)(parent);
    if label.is_null() {
        return label;
    }
    stock!(OBJ_ADD_FLAG, fn(Obj, u32) -> ())(label, EVENT_BUBBLE);
    if !font.is_null() {
        style(label, STYLE_TEXT_FONT, font as u32);
    }
    style(label, STYLE_TEXT_COLOR, color);
    stock!(LABEL_SET_LONG_MODE, fn(Obj, u32) -> ())(label, LONG_DOT);
    stock!(LABEL_SET_TEXT, fn(Obj, *const u8) -> ())(label, text);
    stock!(OBJ_SET_POS, fn(Obj, i32, i32) -> ())(label, x, y);
    stock!(OBJ_SET_SIZE, fn(Obj, i32, i32) -> ())(label, w, h);
    label
}

fn image(parent: Obj, dsc: *const ImgDsc, x: i32, y: i32, side: i32) -> Obj {
    let img = stock!(VIMG_CREATE, fn(Obj) -> Obj)(parent);
    if !img.is_null() {
        stock!(OBJ_SET_POS, fn(Obj, i32, i32) -> ())(img, x, y);
        stock!(OBJ_SET_SIZE, fn(Obj, i32, i32) -> ())(img, side, side);
        stock!(VIMG_SET_SRC, fn(Obj, *const ImgDsc) -> ())(img, dsc);
        stock!(OBJ_ADD_FLAG, fn(Obj, u32) -> ())(img, EVENT_BUBBLE);
    }
    img
}

/// `buf.len()` bytes of `path` from `offset`; false if the file is missing or short
fn read_at(path: &[u8], offset: u32, buf: &mut [u8]) -> bool {
    let mut file: File = [0; 0x2C];
    if fs::open(&mut file, path, fs::READ) < 0 {
        return false;
    }
    let ok = (offset == 0 || fs::seek(&mut file, offset) == 0) && fs::read_exact(&mut file, buf);
    fs::close(&mut file);
    ok
}

/// up to `buf.len()` bytes of `path`, how many were read (0 if it is missing)
fn read_upto(path: &[u8], buf: &mut [u8]) -> usize {
    let mut file: File = [0; 0x2C];
    if fs::open(&mut file, path, fs::READ) < 0 {
        return 0;
    }
    let mut done = 0;
    while done < buf.len() {
        let n = fs::read(&mut file, &mut buf[done..]);
        if n <= 0 {
            break;
        }
        done += n as usize;
    }
    fs::close(&mut file);
    done
}

extern "C" fn create(ctx: *mut Ctx) -> i32 {
    let ctx = unsafe { &mut *ctx };
    if ctx.obj.is_null() || ctx.state.is_null() {
        return 0;
    }
    let s = unsafe { &mut *ctx.state };
    style(ctx.obj, STYLE_BG_COLOR, CARD_BG);
    let slot = param(ctx.obj).wrapping_sub(1);
    if slot >= wp2_core::layout::SLOTS {
        apps(s, ctx.obj);
        return 0;
    }
    // ponytail: read once per layout; the card is laid out again whenever the face shows, and an app only
    // runs (and writes its record) under the loader view, which deletes the cards (emulator, docs/recon.md)
    let pkg = slot_path(slot, false);
    let mut head = [0u8; HEADER_SIZE];
    let mut name = [0u8; WIDGET_TITLE];
    let (mut icons, mut trailer) = (0, 0);
    if read_at(&pkg, 0, &mut head) {
        name.copy_from_slice(&head[8..8 + WIDGET_TITLE]);
        // the widget icons: the trailer's head is checked before any icon is read; a bad one means no icons
        if let Ok(h) = parse_header(&head) {
            let mut t = [0u8; TRAILER_HEAD];
            trailer = file_size(&h);
            if let Some((n, _)) = read_at(&pkg, trailer, &mut t).then(|| trailer_head(&t)).flatten() {
                icons = n;
            }
        }
    } else {
        name[..TEXT.len()].copy_from_slice(TEXT);
    }
    let mut rec = [0u8; REC_MAX + 1];
    let n = read_upto(&widget_path(slot), &mut rec);
    match card(&rec[..n], icons) {
        Some(Card::Columns(k)) => columns(s, ctx.obj, &mut rec, k, &pkg, trailer),
        Some(Card::Text(at)) => {
            rec[at + WIDGET_TITLE - 1] = 0; // v1 fields are not checked: as before stub 9
            rec[at + WIDGET_REC] = 0;
            text(s, ctx.obj, &rec[at..])
        }
        // no record, or one this stub does not draw: the package name (header +8, 24 bytes NUL-padded)
        None => {
            name[WIDGET_TITLE - 1] = 0;
            text(s, ctx.obj, &name)
        }
    }
    0
}

/// title (grey) and value (white) in font32M; `rec` is NUL-terminated at WIDGET_TITLE - 1 and, if longer, at WIDGET_REC
fn text(s: &mut State, obj: Obj, rec: &[u8]) {
    let font = open_font(s, 0, FONT_OPEN, FONT_PATH);
    let line = if font.is_null() { 32 } else { unsafe { *font.add(2) as i32 } }.clamp(1, H);
    s.objs[0] = label(obj, font, rec.as_ptr(), GREY, 24, 24, W - 48, line);
    if rec.len() > WIDGET_TITLE {
        s.objs[1] = label(obj, font, rec[WIDGET_TITLE..].as_ptr(), WHITE, 24, 24 + line + 8, W - 48, H - 48 - line - 8);
    }
}

/// Stock widget 97's layout (create 0x100AC7EC, docs/recon.md "Widget cards"): columns 80 px apart from x 20; the
/// caption in font24L at y 18 (80×36), the 40 px icon centred on the tile, the value in font24M at y 116, all white.
/// Fewer than 4 columns share the 320 px evenly. `°` (C2 B0) is drawn as `º` (C2 BA): the stock fonts have only that
/// one, and 97 writes its temperatures with it (emulator).
fn columns(s: &mut State, obj: Obj, rec: &mut [u8], k: u8, pkg: &[u8], trailer: u32) {
    for i in 1..rec.len() {
        if rec[i - 1] == 0xC2 && rec[i] == 0xB0 {
            rec[i] = 0xBA;
        }
    }
    let caption = open_font(s, 0, STOCK_FONT_OPEN, CAPTION_FONT);
    let value = open_font(s, 1, STOCK_FONT_OPEN, VALUE_FONT);
    let stride = 320 / i32::from(k);
    for i in 0..usize::from(k) {
        let col = &rec[COLUMNS_AT + i * COLUMN..];
        let x = 20 + i as i32 * stride;
        s.objs[3 * i] = label(obj, caption, col.as_ptr(), WHITE, x, 18, stride, 36);
        let icon = col[CAPTION];
        if icon != NO_ICON {
            let at = trailer + icon_at(icon) as u32;
            if let Some((block, dsc)) = image_block(ICON_SIDE.into(), |px| read_at(pkg, at, px)) {
                s.blocks[i] = block;
                s.objs[3 * i + 1] = image(obj, dsc, x, (H - i32::from(ICON_SIDE)) / 2, ICON_SIDE.into());
            }
        }
        s.objs[3 * i + 2] = label(obj, value, col[CAPTION + 1..].as_ptr(), WHITE, x, 116, stride, 44);
    }
}

/// param_id 0: the Custom Apps icon and "Apps"
fn apps(s: &mut State, obj: Obj) {
    let font = open_font(s, 0, FONT_OPEN, FONT_PATH);
    let line = if font.is_null() { 32 } else { unsafe { *font.add(2) as i32 } }.clamp(1, H);
    if let Some((block, dsc)) = icon_block() {
        s.blocks[0] = block;
        s.objs[0] = image(obj, dsc, PAD, PAD, ICON as i32);
    }
    let x = PAD + ICON as i32 + 20;
    s.objs[1] = label(obj, font, TEXT.as_ptr(), WHITE, x, (H - line) / 2, W - x - 24, line);
}

// Children go before the font and the pixels they use; the stock then frees the state and deletes the screen.
extern "C" fn delete(ctx: *mut Ctx) -> i32 {
    let ctx = unsafe { &mut *ctx };
    if ctx.state.is_null() {
        return 0;
    }
    let s = unsafe { &mut *ctx.state };
    let del = stock!(OBJ_DEL, fn(Obj) -> ());
    for obj in s.objs.iter_mut().rev() {
        if !obj.is_null() {
            del(*obj);
            *obj = null_mut();
        }
    }
    for i in 0..2 {
        if s.font_open[i] != 0 {
            stock!(FONT_CLOSE, fn(*mut u32) -> ())(s.font[i].as_mut_ptr());
            s.font_open[i] = 0;
        }
    }
    for block in &mut s.blocks {
        if !block.is_null() {
            unsafe { heap::free(RES, *block) };
            *block = null_mut();
        }
    }
    0
}

// r0 = the container (stock 0x101B1B14 tail-calls the action with the current target, emulator): the loader takes
// `launch` at LAYOUT and starts that slot instead of showing the menu
extern "C" fn tap(obj: Obj) {
    stub_ram().launch = param(obj);
    stock!(OPEN_VIEW, fn(u32, u32) -> i32)(TAP_VIEW, 0);
}

extern "C" fn update(_ctx: *mut Ctx) -> i32 {
    0
}

extern "C" fn focus(_ctx: *mut Ctx, _on: u32) -> i32 {
    0
}

extern "C" fn reload(_ctx: *mut Ctx, _arg: u32) -> i32 {
    0
}
