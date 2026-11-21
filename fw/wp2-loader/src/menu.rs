use wp2_core::layout::SLOTS;
use wp2_core::pkg::{check_slot, parse_header, HEADER_SIZE};

pub const ROWS: usize = SLOTS as usize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    pub slot: u8,
    pub name: [u8; 24],
    pub len: u8,
    pub version: u32,
    pub size: u32,
    // pkg::F_WIDGET | F_PAGE, read after the icon by scan
    pub features: u8,
}

pub fn entry(slot: u8, raw: &[u8; HEADER_SIZE]) -> Option<Entry> {
    let h = parse_header(raw).ok()?;
    check_slot(&h, slot).ok()?;
    let len = h.name.iter().position(|&b| b == 0).unwrap_or(h.name.len()) as u8;
    Some(Entry { slot, name: h.name, len, version: h.version, size: h.image_size, features: 0 })
}

// The phone's app order (docs/app-order.md): package name fields, NUL-padded as in the header; the first all-zero
// field ends the list. It lives in the loader image behind ORDER_MAGIC: the phone patches it into the loader
// package and installs that, so it needs no stub change and survives reboots.
pub const ORDER_MAX: usize = 16;
pub const ORDER_MAGIC: &[u8; 8] = b"WP2ORDER";
pub type Order = [[u8; 24]; ORDER_MAX];

#[repr(C)]
pub struct OrderTable {
    pub magic: [u8; 8],
    pub names: Order,
}

#[cfg(target_os = "none")]
#[used]
#[unsafe(no_mangle)]
static WP2_ORDER: OrderTable = OrderTable { magic: *ORDER_MAGIC, names: [[0; 24]; ORDER_MAX] };

// black_box: the compiler would fold the all-zero initializer the phone overwrites
#[cfg(target_os = "none")]
fn order() -> &'static Order {
    core::hint::black_box(&WP2_ORDER.names)
}

// the row key: the name's place in `order`, apps not in it after all of those; then the slot
pub fn rank(e: &Entry, order: &Order) -> (usize, u8) {
    let listed = order.iter().take_while(|n| n[0] != 0);
    (listed.clone().position(|n| *n == e.name).unwrap_or(ORDER_MAX), e.slot)
}

// Entries are packed from index 0 in the phone's order; row i shows entries[i].
#[cfg(target_os = "none")]
pub fn scan() -> [Option<Entry>; ROWS] {
    use wp2_core::{fs, layout::slot_path};
    let mut found: [(Option<Entry>, *mut u8); ROWS] = [(None, core::ptr::null_mut()); ROWS];
    let mut n = 0;
    let mut file: fs::File = [0; 0x2C];
    // a stub before 8 takes slots 0..7 only: 8.pkg..F.pkg left by a newer stub are not its apps
    for slot in 0..wp2_core::layout::stub_ram().slots() {
        if fs::open(&mut file, &slot_path(slot, false), fs::READ) < 0 {
            continue;
        }
        let mut raw = [0u8; HEADER_SIZE];
        let ok = fs::read_exact(&mut file, &mut raw);
        if let Some(mut e) = entry(slot, &raw).filter(|_| ok) {
            let mut desc = [0u8; wp2_core::pkg::ICON_DESC];
            if fs::read_exact(&mut file, &mut desc) {
                let icon = lv::read_icon(&mut file, &desc, e.size);
                let mut f = [0u8; wp2_core::pkg::FEATURES];
                // the declarations follow the icon, or stand at offset 0 without one
                e.features = if !icon.is_null() {
                    if fs::read_exact(&mut file, &mut f) { wp2_core::pkg::features(&f) } else { 0 }
                } else {
                    wp2_core::pkg::features(&desc)
                };
                found[n].1 = icon;
            }
            found[n].0 = Some(e);
            n += 1;
        }
        fs::close(&mut file);
    }
    let order = order();
    // ponytail: insertion sort, core's sort costs ~7 KiB of the 64 KiB loader region
    for i in 1..n {
        let mut j = i;
        while j > 0 && found[j].0.map(|e| rank(&e, order)) < found[j - 1].0.map(|e| rank(&e, order)) {
            found.swap(j, j - 1);
            j -= 1;
        }
    }
    let mut out = [None; ROWS];
    for (row, &(e, icon)) in found.iter().enumerate() {
        out[row] = e;
        lv::set_icon(row,e.map_or(255,|e|e.slot),icon);
    }
    out
}

// Label texts, NUL-terminated for lv_label_set_text.
pub type Text = [u8; 26];

pub fn name_text(e: &Entry) -> Text {
    let mut t = [0; 26];
    t[..usize::from(e.len)].copy_from_slice(&e.name[..usize::from(e.len)]);
    t
}

// first character of the name (ASCII upper-cased) for the row icon
pub fn initial(e: &Entry) -> Text {
    let mut t = [0; 26];
    let n = e.name[1..usize::from(e.len).max(1)].iter().take_while(|&&b| b & 0xC0 == 0x80).count() + 1;
    t[..n].copy_from_slice(&e.name[..n]);
    t[0] = t[0].to_ascii_uppercase();
    t
}

pub fn error_text(code: i32) -> Text {
    num(b"Error ", code < 0, code.unsigned_abs())
}

pub fn version_text(v: u32) -> Text {
    num(b"v", false, v)
}

fn num(prefix: &[u8], neg: bool, mut v: u32) -> Text {
    let mut t = [0; 26];
    t[..prefix.len()].copy_from_slice(prefix);
    let mut n = prefix.len();
    if neg {
        t[n] = b'-';
        n += 1;
    }
    let start = n;
    loop {
        t[n] = b'0' + (v % 10) as u8;
        n += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    t[start..n].reverse();
    t
}

// Geometry on the 466x466 round screen, rows as in the stock Settings list (emulator frame: rows 120 px with
// an 8 px gap from y 120, x 16..449, radius 44); the icon is a 96 px circle as in the stock app list.
// PAD_BOTTOM lets the last row scroll up to TITLE_Y, so the list always scrolls (a drag over a row
// scrolls it and LVGL sends no CLICKED) and the crown always moves it.
pub const TITLE_Y: i32 = 56;
pub const TOP: i32 = 120;
pub const PITCH: i32 = 128;
pub const ROW_H: i32 = 120;
pub const COL_X: i32 = 16;
pub const COL_W: i32 = 434;
pub const RADIUS: i32 = 44;
pub const ICON: i32 = wp2_core::pkg::ICON_SIDE as i32;
pub const ICON_X: i32 = (ROW_H - ICON) / 2;
pub const TEXT_X: i32 = ICON_X + ICON + 20;
pub const PAD_R: i32 = 28;
pub const VER_W: i32 = 64;
pub const NAME_W: i32 = COL_W - TEXT_X - PAD_R - VER_W - 8;
pub const PAD_BOTTOM: i32 = 466 - TITLE_Y - ROW_H;

// the long-press sheet: title, then buttons top-down (up to four: widget, page, delete, cancel)
pub const SHEET_TITLE_Y: i32 = 64;
pub const SHEET_W: i32 = 280;
pub const SHEET_H: i32 = 72;
pub const fn sheet_y(i: usize) -> i32 {
    130 + 84 * i as i32
}

// "No apps" sits where row 0 would
#[cfg(test)]
pub fn content_height(rows: usize) -> i32 {
    TOP + PITCH * (rows.max(1) as i32 - 1) + ROW_H + PAD_BOTTOM
}

#[cfg(target_os = "none")]
pub mod lv {
    use super::*;
    use core::ffi::c_void;
    use core::ptr::{addr_of_mut, null, null_mut};
    use wp2_core::fs;
    use wp2_core::heap::{self, RES};
    use wp2_core::load::sync_psram;
    use wp2_core::pkg::{icon_ok, ICON_BYTES, ICON_DESC, ICON_SIDE};
    use wp2_core::stock::*;

    pub type Obj = *mut c_void;
    pub type Cb = extern "C" fn(*mut c_void);

    const FLAG_HIDDEN: u32 = 1;
    // cleared on the icon so a press on it goes to the row
    const FLAG_CLICKABLE: u32 = 2;
    // LV_OBJ_FLAG_USER_4: registers the object as the knob object; the crown scrolls it (docs/recon-lvgl.md)
    pub const FLAG_KNOB: u32 = 1 << 30;
    pub const CLICKED: u32 = 7;
    pub const LONG_PRESSED: u32 = 5;
    const STATE_PRESSED: u32 = 0x20;
    const LONG_DOT: u32 = 1;
    const ALIGN_CENTER: u32 = 2;
    const ALIGN_RIGHT: u32 = 3;
    const ALIGN_CENTER_OBJ: u32 = 9;
    const BLACK: u32 = 0x0000;
    const WHITE: u32 = 0xFFFF;
    const GREY: u32 = 0x8C92; // #8E9196
    // stock accent: the emulator shows it as (160, 43, 15), i.e. RGB565 31/17/3
    const RED: u32 = 0xFA23;
    // stock Settings row: the emulator frame reads (26, 28, 31), white reads 160, so RGB565 5/11/6
    const ROW_BG: u32 = 0x2966;
    const ROW_PRESSED: u32 = 0x4229;
    // letter circle of an app without an icon: #52555A, lighter than the row
    const LETTER_BG: u32 = 0x52AB;
    const CF_TRUE_COLOR_ALPHA: u32 = 5;
    // [64 B like the stock resource-cache header][pixels, 64-byte aligned]: the emulator's GPU model reads
    // RGB565+A8 only behind that header, the hardware ignores it (applist-style, docs/recon.md)
    const HEAD: usize = 64;
    const BLOCK: u32 = HEAD as u32 + ICON_BYTES;
    const FONT_PATH: &[u8] = b"/NAND:A/font32M.font\0";

    // repr(C): the e2e probe reads it by offset
    #[repr(C)]
    struct Ui {
        cont: Obj,
        names: [Obj; ROWS],
        font: [u32; 8],
        font_open: bool,
        bar: Obj,
        icons: [*mut u8; ROWS],
        slots: [u8; ROWS],
        thumbs: [*mut u8; ROWS],
        thumb_dsc: [ImgDsc; ROWS],
        dsc: [ImgDsc; ROWS],
        // "Delete?" over the list after a long press, on the screen; NULL when closed
        confirm: Obj,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ImgDsc {
        header: u32,
        size: u32,
        data: *const u8,
    }

    const NO_DSC: ImgDsc = ImgDsc { header: 0, size: 0, data: null() };

    static mut UI: Ui = Ui {
        cont: null_mut(),
        names: [null_mut(); ROWS],
        font: [0; 8],
        font_open: false,
        bar: null_mut(),
        icons: [null_mut(); ROWS],
        slots: [255; ROWS],
        thumbs: [null_mut(); ROWS],
        thumb_dsc: [NO_DSC; ROWS],
        dsc: [NO_DSC; ROWS],
        confirm: null_mut(),
    };

    fn ui() -> &'static mut Ui {
        unsafe { &mut *addr_of_mut!(UI) }
    }

    // `desc` = image bytes 0..16, the file is positioned after them. Any failure gives NULL: the row shows the letter.
    pub fn read_icon(file: &mut fs::File, desc: &[u8; ICON_DESC], image_size: u32) -> *mut u8 {
        if !icon_ok(desc, image_size) {
            return null_mut();
        }
        let block = heap::alloc(RES, BLOCK, 0x40);
        if block.is_null() {
            return block;
        }
        let px = unsafe { block.add(HEAD) };
        if !fs::read_exact(file, unsafe { core::slice::from_raw_parts_mut(px, ICON_BYTES as usize) }) {
            unsafe { heap::free(RES, block) };
            return null_mut();
        }
        let head = unsafe { core::slice::from_raw_parts_mut(block as *mut u32, HEAD / 4) };
        head.fill(0);
        (head[2], head[4], head[5], head[6], head[8]) = (BLOCK, ICON_SIDE, ICON_SIDE, 3, px as u32);
        sync_psram(block, BLOCK);
        block
    }

    pub fn set_icon(row: usize, slot:u8, block: *mut u8) {
        let u=ui();
        unsafe { heap::free(RES,u.thumbs[row]); }
        u.thumbs[row]=null_mut();u.thumb_dsc[row]=NO_DSC;
        u.slots[row]=slot;
        let old = core::mem::replace(&mut ui().icons[row], block);
        unsafe { heap::free(RES, old) };
    }

    // UI thread only. At most sixteen 64px thumbnails, allocated lazily; descriptors live with the menu.
    pub fn app_icon(obj:Obj,slot:u8,side:u8)->Result<(),i32> {
        let u=ui();
        style(obj,LV_STYLE_BG_IMG_SRC,0,0);
        let Some(i)=(0..ROWS).find(|&i|u.slots[i]==slot && !u.icons[i].is_null()) else{return Ok(());};
        if side==96 {
            u.dsc[i]=ImgDsc{header:CF_TRUE_COLOR_ALPHA|ICON_SIDE<<10|ICON_SIDE<<21,size:ICON_BYTES,data:unsafe{u.icons[i].add(HEAD)}};
            style(obj,LV_STYLE_BG_IMG_SRC,&u.dsc[i] as *const ImgDsc as u32,0);
            return Ok(());
        }
        const SIDE:u32=64; const BYTES:u32=SIDE*SIDE*3; const TOTAL:u32=(64+BYTES+63)&!63;
        if u.thumbs[i].is_null() {
            let block=heap::alloc(RES,TOTAL,0x40); if block.is_null(){return Ok(());}
            unsafe {
                let px=block.add(64);let src=u.icons[i].add(HEAD);
                for y in 0..SIDE as usize {for x in 0..SIDE as usize {
                    let from=((y*96/64)*96+x*96/64)*3;let to=(y*64+x)*3;
                    core::ptr::copy_nonoverlapping(src.add(from),px.add(to),3);
                }}
                let h=core::slice::from_raw_parts_mut(block as *mut u32,16);h.fill(0);
                (h[2],h[4],h[5],h[6],h[8])=(TOTAL,SIDE,SIDE,3,px as u32);
                u.thumb_dsc[i]=ImgDsc{header:CF_TRUE_COLOR_ALPHA|SIDE<<10|SIDE<<21,size:BYTES,data:px};
            }
            sync_psram(block,TOTAL);u.thumbs[i]=block;
        }
        style(obj,LV_STYLE_BG_IMG_SRC, &u.thumb_dsc[i] as *const ImgDsc as u32,0);
        Ok(())
    }

    /// UI thread, after app LVGL objects have been deleted. Menu-native pixels remain intact.
    pub fn clear_app_icons() {
        let u=ui();
        for b in &mut u.thumbs {unsafe {heap::free(RES,*b)};*b=null_mut();}
        u.thumb_dsc=[NO_DSC;ROWS];
    }

    fn style(obj: Obj, prop: u32, value: u32, selector: u32) {
        stock!(LV_OBJ_SET_LOCAL_STYLE_PROP, fn(Obj, u32, u32, u32))(obj, prop, value, selector)
    }

    fn place(obj: Obj, x: i32, y: i32, w: i32, h: i32) -> Obj {
        stock!(LV_OBJ_SET_POS, fn(Obj, i32, i32))(obj, x, y);
        stock!(LV_OBJ_SET_SIZE, fn(Obj, i32, i32))(obj, w, h);
        obj
    }

    fn panel(parent: Obj, x: i32, y: i32, w: i32, h: i32, color: u32) -> Obj {
        let o = place(stock!(LV_OBJ_CREATE, fn(Obj) -> Obj)(parent), x, y, w, h);
        style(o, LV_STYLE_BG_COLOR, color, 0);
        style(o, LV_STYLE_BG_OPA, 0xFF, 0);
        o
    }

    fn label(parent: Obj, x: i32, y: i32, w: i32, h: i32, text: &Text) -> Obj {
        let l = stock!(LV_LABEL_CREATE, fn(Obj) -> Obj)(parent);
        stock!(LV_LABEL_SET_LONG_MODE, fn(Obj, u32))(l, LONG_DOT);
        set_text(l, text);
        place(l, x, y, w, h)
    }

    fn set_text(l: Obj, text: &Text) {
        stock!(LV_LABEL_SET_TEXT, fn(Obj, *const u8))(l, text.as_ptr())
    }

    fn text(s: &[u8]) -> Text {
        let mut t = [0; 26];
        t[..s.len()].copy_from_slice(s);
        t
    }

    // UI thread (view msg 3). `click` gets CLICKED with the row index as user data.
    // `long` gets LONG_PRESSED likewise (a CLICKED still follows on release).
    pub fn build(scr: Obj, entries: &[Option<Entry>; ROWS], click: Cb, long: Cb) {
        let u = ui();
        u.font_open = stock!(LVGL_BITMAP_FONT_OPEN, fn(*mut u32, *const u8) -> i32)(u.font.as_mut_ptr(), FONT_PATH.as_ptr()) == 0;
        let font = if u.font_open { u.font.as_ptr() as u32 } else { LV_FONT_DEFAULT };
        let lh = unsafe { *(font as *const i32).add(2) }.clamp(1, ROW_H);
        let ty = (ROW_H - lh) / 2;
        let cont = panel(scr, 0, 0, 466, 466, BLACK);
        style(cont, LV_STYLE_PAD_BOTTOM, PAD_BOTTOM as u32, 0);
        style(cont, LV_STYLE_TEXT_FONT, font, 0);
        style(cont, LV_STYLE_TEXT_COLOR, WHITE, 0);
        let title = label(cont, 0, TITLE_Y, 466, lh, &text(b"Apps"));
        style(title, LV_STYLE_TEXT_ALIGN, ALIGN_CENTER, 0);
        style(title, LV_STYLE_TEXT_COLOR, RED, 0);
        let mut rows = 0;
        for (i, e) in entries.iter().flatten().enumerate() {
            let row = panel(cont, COL_X, TOP + PITCH * i as i32, COL_W, ROW_H, ROW_BG);
            style(row, LV_STYLE_RADIUS, RADIUS as u32, 0);
            style(row, LV_STYLE_BG_COLOR, ROW_PRESSED, STATE_PRESSED);
            stock!(LV_OBJ_ADD_EVENT_CB, fn(Obj, Cb, u32, usize) -> *mut c_void)(row, click, CLICKED, i);
            stock!(LV_OBJ_ADD_EVENT_CB, fn(Obj, Cb, u32, usize) -> *mut c_void)(row, long, LONG_PRESSED, i);
            let block = u.icons[i];
            let icon = if block.is_null() {
                let c = panel(row, ICON_X, ICON_X, ICON, ICON, LETTER_BG);
                style(c, LV_STYLE_RADIUS, (ICON / 2) as u32, 0);
                let letter = label(c, 0, (ICON - lh) / 2, ICON, lh, &initial(e));
                style(letter, LV_STYLE_TEXT_ALIGN, ALIGN_CENTER, 0);
                c
            } else {
                let px = unsafe { block.add(HEAD) };
                u.dsc[i] = ImgDsc { header: CF_TRUE_COLOR_ALPHA | ICON_SIDE << 10 | ICON_SIDE << 21, size: ICON_BYTES, data: px };
                let img = stock!(LV_IMG_CREATE, fn(Obj) -> Obj)(row);
                stock!(LV_IMG_SET_SRC, fn(Obj, *const ImgDsc))(img, &u.dsc[i]);
                stock!(LV_OBJ_SET_POS, fn(Obj, i32, i32))(img, ICON_X, ICON_X);
                img
            };
            stock!(LV_OBJ_CLEAR_FLAG, fn(Obj, u32))(icon, FLAG_CLICKABLE);
            u.names[i] = label(row, TEXT_X, ty, NAME_W, lh, &name_text(e));
            let v = label(row, COL_W - PAD_R - VER_W, ty, VER_W, lh, &version_text(e.version));
            style(v, LV_STYLE_TEXT_ALIGN, ALIGN_RIGHT, 0);
            style(v, LV_STYLE_TEXT_COLOR, GREY, 0);
            rows += 1;
        }
        if rows == 0 {
            label(cont, COL_X + TEXT_X, TOP + ty, NAME_W, lh, &text(b"No apps"));
        }
        // as the stock app list (0x1005EFA6); the bar also gives the list the knob flag
        let bar = stock!(JX_SCROLL_BAR_CREATE, fn(Obj) -> Obj)(scr);
        stock!(LV_OBJ_SET_SIZE, fn(Obj, i32, i32))(bar, 462, 462);
        stock!(JX_SCROLL_BAR_SET_SCROLL_OBJ, fn(Obj, Obj))(bar, cont);
        stock!(JX_SCROLL_BAR_SET_KNOB, fn(Obj, bool))(bar, true);
        stock!(LV_OBJ_ALIGN, fn(Obj, u32, i32, i32))(bar, ALIGN_CENTER_OBJ, 0, 0);
        u.cont = cont;
        u.bar = bar;
    }

    // The action sheet over the list after a long press: the app's name, then one button per (text, red, user data),
    // all CLICKED to `cb`. Close it from the timer, not from the callback (it runs on its own children).
    pub fn confirm(scr: Obj, name: &Text, buttons: &[(&[u8], bool, usize)], cb: Cb) {
        let u = ui();
        if u.cont.is_null() || !u.confirm.is_null() {
            return;
        }
        let font = if u.font_open { u.font.as_ptr() as u32 } else { LV_FONT_DEFAULT };
        let lh = unsafe { *(font as *const i32).add(2) }.clamp(1, ROW_H);
        let c = panel(scr, 0, 0, 466, 466, BLACK);
        style(c, LV_STYLE_TEXT_FONT, font, 0);
        style(c, LV_STYLE_TEXT_COLOR, WHITE, 0);
        let t = label(c, COL_X, SHEET_TITLE_Y, COL_W, lh, name);
        style(t, LV_STYLE_TEXT_ALIGN, ALIGN_CENTER, 0);
        for (i, &(text, red, data)) in buttons.iter().enumerate() {
            let b = panel(c, (466 - SHEET_W) / 2, sheet_y(i), SHEET_W, SHEET_H, if red { RED } else { ROW_BG });
            style(b, LV_STYLE_RADIUS, (SHEET_H / 2) as u32, 0);
            style(b, LV_STYLE_BG_COLOR, ROW_PRESSED, STATE_PRESSED);
            stock!(LV_OBJ_ADD_EVENT_CB, fn(Obj, Cb, u32, usize) -> *mut c_void)(b, cb, CLICKED, data);
            let l = label(b, 0, (SHEET_H - lh) / 2, SHEET_W, lh, &self::text(text));
            style(l, LV_STYLE_TEXT_ALIGN, ALIGN_CENTER, 0);
            stock!(LV_OBJ_CLEAR_FLAG, fn(Obj, u32))(l, FLAG_CLICKABLE);
        }
        u.confirm = c;
    }

    pub fn confirm_open() -> bool {
        !ui().confirm.is_null()
    }

    pub fn close_confirm() {
        let u = ui();
        if !u.confirm.is_null() {
            stock!(LV_OBJ_DEL, fn(Obj))(core::mem::replace(&mut u.confirm, null_mut()));
        }
    }

    pub fn row_of(e: *mut c_void) -> usize {
        stock!(LV_EVENT_GET_USER_DATA, fn(*mut c_void) -> usize)(e)
    }

    pub fn set_row_text(row: usize, t: &Text) {
        let l = ui().names[row];
        if !l.is_null() {
            set_text(l, t);
        }
    }

    // hidden: no touch, not drawn; shown: the knob object again (the image took it while an app ran).
    // The bar stays hidden until the list scrolls, as after its create.
    pub fn show(on: bool) {
        let u = ui();
        if u.cont.is_null() {
            return;
        }
        if on {
            stock!(LV_OBJ_CLEAR_FLAG, fn(Obj, u32))(u.cont, FLAG_HIDDEN);
            stock!(LV_DISP_ADD_KNOB_OBJ, fn(Obj))(u.cont);
        } else {
            stock!(LV_OBJ_ADD_FLAG, fn(Obj, u32))(u.cont, FLAG_HIDDEN);
            stock!(LV_OBJ_ADD_FLAG, fn(Obj, u32))(u.bar, FLAG_HIDDEN);
        }
    }

    // UI thread, view msg 4: the loader block is freed afterwards, so no object, callback or font is left.
    pub fn delete() {
        close_confirm();
        let u = ui();
        if !u.cont.is_null() {
            stock!(LV_OBJ_CLEAR_FLAG, fn(Obj, u32))(u.cont, FLAG_KNOB);
            stock!(LV_OBJ_DEL, fn(Obj))(u.cont);
            // after the list, as stock deletes the screen's children; its destructor drops its timer
            stock!(LV_OBJ_DEL, fn(Obj))(u.bar);
        }
        if u.font_open {
            stock!(LVGL_BITMAP_FONT_CLOSE, fn(*mut u32))(u.font.as_mut_ptr());
        }
        for b in &mut u.icons {
            unsafe { heap::free(RES, *b) };
            *b = null_mut();
        }
        for b in &mut u.thumbs { unsafe {heap::free(RES,*b)}; *b=null_mut(); }
        u.thumb_dsc=[NO_DSC; ROWS];u.slots=[255; ROWS];
        u.dsc = [NO_DSC; ROWS];
        u.cont = null_mut();
        u.bar = null_mut();
        u.names = [null_mut(); ROWS];
        u.font_open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp2_core::pkg::crc32;

    fn header(name: &[u8], kind: u8) -> [u8; HEADER_SIZE] {
        let mut raw = [0u8; HEADER_SIZE];
        raw[..4].copy_from_slice(b"WP2A");
        raw[4] = 1;
        raw[6] = kind;
        raw[8..8 + name.len()].copy_from_slice(name);
        raw[0x20..0x24].copy_from_slice(&7u32.to_le_bytes());
        raw[0x24..0x28].copy_from_slice(&0x100u32.to_le_bytes());
        let crc = crc32(0, &raw[..0x34]);
        raw[0x34..0x38].copy_from_slice(&crc.to_le_bytes());
        raw
    }

    fn cstr(t: &Text) -> &[u8] {
        &t[..t.iter().position(|&b| b == 0).unwrap()]
    }

    #[test]
    fn entry_needs_valid_app_header() {
        let e = entry(2, &header(b"hello", 0)).unwrap();
        assert_eq!((e.slot, &e.name[..e.len as usize], e.version), (2, &b"hello"[..], 7));
        assert_eq!(entry(2, &header(b"loader", 1)), None);
        let mut bad = header(b"hello", 0);
        bad[0x20] ^= 1;
        assert_eq!(entry(2, &bad), None);
    }

    #[test]
    fn texts_are_nul_terminated() {
        let e = entry(0, &header(b"abcdefghijklmnopqrstuvw", 0)).unwrap();
        assert_eq!(cstr(&name_text(&e)), b"abcdefghijklmnopqrstuvw");
        assert_eq!(cstr(&error_text(-12)), b"Error -12");
        assert_eq!(cstr(&error_text(i32::MIN)), b"Error -2147483648");
        assert_eq!(cstr(&error_text(5)), b"Error 5");
        assert_eq!(cstr(&version_text(0)), b"v0");
        assert_eq!(cstr(&version_text(u32::MAX)), b"v4294967295");
        assert_eq!(cstr(&initial(&e)), b"A");
        assert_eq!(cstr(&initial(&entry(0, &header("éte".as_bytes(), 0)).unwrap())), "é".as_bytes());
        assert_eq!(cstr(&initial(&entry(0, &header(b"7", 0)).unwrap())), b"7");
    }

    #[test]
    fn order_ranks_rows() {
        let mut order: Order = [[0; 24]; ORDER_MAX];
        let field = |s: &[u8]| { let mut f = [0u8; 24]; f[..s.len()].copy_from_slice(s); f };
        (order[0], order[1], order[2]) = (field(b"c"), field(b"gone"), field(b"a"));
        order[4] = field(b"b"); // after the end of the list: ignored
        let mut rows = [b"a", b"b", b"c", b"d"].map(|n| entry(0, &header(n, 0)).unwrap());
        for (i, e) in rows.iter_mut().enumerate() {
            e.slot = i as u8;
        }
        rows.sort_unstable_by_key(|e| rank(e, &order));
        assert_eq!(rows.map(|e| e.name[0]), *b"cabd");
        rows.sort_unstable_by_key(|e| rank(e, &[[0; 24]; ORDER_MAX]));
        assert_eq!(rows.map(|e| e.slot), [0, 1, 2, 3], "no list: slot order");
        assert_eq!(core::mem::size_of::<OrderTable>(), 8 + 24 * ORDER_MAX);
    }

    #[test]
    fn list_always_scrolls() {
        for n in 0..=ROWS {
            assert!(content_height(n) > 466, "{n} rows");
        }
        assert_eq!(content_height(ROWS) - 466, TOP + PITCH * (ROWS as i32 - 1) - TITLE_Y, "last row reaches the title");
        assert!(NAME_W > 200);
        assert_eq!(COL_X + COL_W, 450);
        assert!(ICON_X + ICON < TEXT_X && 2 * RADIUS < ROW_H);
    }
}
