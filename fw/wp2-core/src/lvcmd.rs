//! ABI 1.2 LVGL command buffer: decode, validate, handle table (docs/specs/2026-11-11-lvgl-apps-design.md).
//! Pure logic, no heap, no LVGL calls. The loader validates a whole batch before the first LVGL call.

use crate::stock::LV_STYLE_TEXT_FONT;

/// STYLE takes only the stock setter ids whose value is a scalar: not the image sources 40 (BG_IMG_SRC) and 84
/// (ARC_IMG_SRC), which are pointers, nor 45, 92, 93 and 102, whose value type is unproven (Ruling 14).
pub const ACCEPTED_PROPS: [u16; 42] = [
    1, 4, 6, 7, 8, 9, 10, 11, 16, 17, 18, 19, 20, 21, 22, 32, 33, 34, 48, 50, 53, 54, 55, 70, 71, 72, 80, 81, 82, 83, 85,
    86, 87, 89, 91, 96, 107, 108, 113, 114, 115, 116,
];
// LV_PART_* MAIN, SCROLLBAR, INDICATOR, KNOB, SELECTED, ITEMS (sel >> 16); single LV_STATE_* (sel & 0xFFFF): DEFAULT,
// CHECKED, FOCUSED, PRESSED, DISABLED. At most 30 local styles per object, under the 6-bit style_cnt (Ruling 15).
const STYLE_PARTS: u32 = 5;
const STYLE_STATES: [u32; 5] = [0, 0x01, 0x02, 0x20, 0x80];

pub const MAX_OBJS: usize = 192;
pub const MAX_FONTS: u8 = 4;
/// Image objects alive at once: the loader keeps one `lv_img_dsc_t` for each (docs/bitmap-assets.md).
pub const MAX_IMAGES: usize = 8;
pub const FONT_FILES: u8 = 16;
pub const MAX_TEXT: usize = 128;
pub const MAX_OPTIONS: usize = 240;

const FREE: u8 = 0xFF;
// HIDDEN 0, CLICKABLE 1, CHECKABLE 3, SCROLLABLE 4, SCROLL_ELASTIC 5, SCROLL_MOMENTUM 6
const FLAG_MASK: u32 = 0x7B;
const EVENT_MASK: u8 = 0x1F;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Class {
    Obj = 0,
    Label = 1,
    Btn = 2,
    Arc = 3,
    Bar = 4,
    Slider = 5,
    Roller = 6,
    /// Loader-internal (SCROLLBAR record); CREATE with class 7 is `BadValue`.
    ScrollBar = 7,
    /// `lv_tileview`: a vertical pager of full-screen pages that snaps to one page per swipe or crown step
    /// (loader of 2026-11-14; an older loader answers `-22`). Its children are [`Class::Page`]s only.
    Pager = 8,
    /// A tileview tile: the next row of its parent [`Class::Pager`] or the next column of its [`Class::PagerH`]; it
    /// scrolls its own content vertically.
    Page = 9,
    /// `lv_tileview` as a horizontal pager (the stock workout screen `sport1_view`): pages side by side, one per
    /// swipe, no crown. While one lives the loader keeps the right back swipe from the app (docs/abi-1.4.md,
    /// "Horizontal pager"). Loader of 2026-11-16; an older loader answers `-22`. Its children are pages only.
    PagerH = 10,
    /// The stock workout screen's music tile (`sport1_view`, docs/abi-1.4.md "Music tile"): track title and artist,
    /// "No music", previous / play-pause / next and volume, with the stock images and fonts. The loader fills it
    /// from the phone and sends the keys itself. One at a time. Loader of 2026-11-16; an older loader answers `-22`.
    Music = 11,
    /// A bitmap the app carries: `lv_img` over pixels in the app block (docs/bitmap-assets.md). Set with the
    /// IMAGE_SRC record. Loader with bitmaps; an older one refuses CREATE (`-22`).
    Image = 12,
}

impl Class {
    fn from_u8(v: u8) -> Option<Class> {
        Some(match v {
            0 => Class::Obj,
            1 => Class::Label,
            2 => Class::Btn,
            3 => Class::Arc,
            4 => Class::Bar,
            5 => Class::Slider,
            6 => Class::Roller,
            7 => Class::ScrollBar,
            8 => Class::Pager,
            9 => Class::Page,
            10 => Class::PagerH,
            11 => Class::Music,
            12 => Class::Image,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Malformed,
    BadHandle,
    BadClass,
    BadValue,
    Budget,
}

impl Error {
    pub fn code(self) -> i32 {
        match self {
            Error::Budget => -12,
            _ => -22,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rec<'a> {
    Create { h: u16, class: Class, parent: u16 },
    Del(u16),
    Size { h: u16, w: i32, hh: i32 },
    Align { h: u16, align: u8, x: i16, y: i16 },
    Flags { h: u16, set: u32, clear: u32 },
    Style { h: u16, prop: u16, sel: u32, value: i32 },
    Font { slot: u8, font: u8 },
    Text { h: u16, s: &'a [u8] },
    Value { h: u16, v: i32, anim: bool },
    Range { h: u16, min: i32, max: i32 },
    Options { h: u16, mode: u8, s: &'a [u8] },
    Events { h: u16, mask: u8 },
    Flex { h: u16, flow: u8, main: u8, cross: u8, track: u8 },
    ScrollBar { h: u16, target: u16 },
    Knob(u16),
    Angles { h: u16, start: u16, end: u16, rot: u16 },
    Focus { h: u16, icon: u16 },
    LongMode { h: u16, mode: u8 },
    AppIcon { h: u16, slot: u8, side: u8 },
    /// Pixels in the app block as an `lv_img_dsc_t` source (`IMAGE_SRC`, opcode 0x19): `ptr` points at `w * hh`
    /// RGB565 pixels (`cf` 4) that must lie inside the running app's image (checked in `lv::submit`).
    Image { h: u16, ptr: u32, w: u16, hh: u16, cf: u8 },
}

/// Handle table: class per handle (`0xFF` = free; 0 is the root, class Obj) and the parent
/// (for a ScrollBar: its scroll target). ~580 B, cheap to copy for a dry run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    class: [u8; MAX_OBJS + 1],
    parent: [u16; MAX_OBJS + 1],
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

impl Table {
    pub fn new() -> Table {
        let mut t = Table { class: [FREE; MAX_OBJS + 1], parent: [0; MAX_OBJS + 1] };
        t.class[0] = Class::Obj as u8;
        t
    }

    pub fn reset(&mut self) {
        *self = Table::new();
    }

    pub fn class(&self, h: u16) -> Option<Class> {
        match self.class.get(h as usize) {
            Some(&c) if c != FREE => Class::from_u8(c),
            _ => None,
        }
    }

    fn live(&self) -> usize {
        self.class[1..].iter().filter(|&&c| c != FREE).count()
    }

    /// A live object of class `c`.
    pub fn has(&self, c: Class) -> bool {
        self.class[1..].contains(&(c as u8))
    }
}

// --- decoding ---

fn u16le(p: &[u8]) -> u16 {
    u16::from_le_bytes([p[0], p[1]])
}
fn i16le(p: &[u8]) -> i16 {
    i16::from_le_bytes([p[0], p[1]])
}
fn u32le(p: &[u8]) -> u32 {
    u32::from_le_bytes([p[0], p[1], p[2], p[3]])
}
fn i32le(p: &[u8]) -> i32 {
    i32::from_le_bytes([p[0], p[1], p[2], p[3]])
}

fn decode<'a>(buf: &mut &'a [u8]) -> Result<Rec<'a>, Error> {
    if buf.len() < 4 {
        return Err(Error::Malformed);
    }
    let (op, len, h) = (buf[0], buf[1] as usize, u16le(&buf[2..]));
    if buf.len() < 4 + len {
        return Err(Error::Malformed);
    }
    let p = &buf[4..4 + len];
    *buf = &buf[4 + len..];
    let need = |n: usize| if p.len() == n { Ok(()) } else { Err(Error::Malformed) };
    Ok(match op {
        0x01 => {
            need(3)?;
            let class = match Class::from_u8(p[0]) {
                Some(c) if c != Class::ScrollBar => c,
                _ => return Err(Error::BadValue),
            };
            Rec::Create { h, class, parent: u16le(&p[1..]) }
        }
        0x02 => {
            need(0)?;
            Rec::Del(h)
        }
        0x03 => {
            need(8)?;
            Rec::Size { h, w: i32le(p), hh: i32le(&p[4..]) }
        }
        0x04 => {
            need(5)?;
            Rec::Align { h, align: p[0], x: i16le(&p[1..]), y: i16le(&p[3..]) }
        }
        0x05 => {
            need(8)?;
            Rec::Flags { h, set: u32le(p), clear: u32le(&p[4..]) }
        }
        0x06 => {
            need(10)?;
            Rec::Style { h, prop: u16le(p), sel: u32le(&p[2..]), value: i32le(&p[6..]) }
        }
        0x07 => {
            need(2)?;
            if h != 0 {
                return Err(Error::BadHandle);
            }
            Rec::Font { slot: p[0], font: p[1] }
        }
        0x08 => Rec::Text { h, s: p },
        0x09 => {
            need(5)?;
            Rec::Value { h, v: i32le(p), anim: p[4] != 0 }
        }
        0x0A => {
            need(8)?;
            Rec::Range { h, min: i32le(p), max: i32le(&p[4..]) }
        }
        0x0B => {
            if p.is_empty() {
                return Err(Error::Malformed);
            }
            Rec::Options { h, mode: p[0], s: &p[1..] }
        }
        0x0C => {
            need(1)?;
            Rec::Events { h, mask: p[0] }
        }
        0x0D => {
            need(4)?;
            Rec::Flex { h, flow: p[0], main: p[1], cross: p[2], track: p[3] }
        }
        0x0E => {
            need(2)?;
            Rec::ScrollBar { h, target: u16le(p) }
        }
        0x0F => {
            need(0)?;
            Rec::Knob(h)
        }
        0x10 => {
            need(6)?;
            Rec::Angles { h, start: u16le(p), end: u16le(&p[2..]), rot: u16le(&p[4..]) }
        }
        0x11 => {
            need(2)?;
            Rec::Focus { h, icon: u16le(p) }
        }
        0x1A => {
            need(2)?;
            Rec::AppIcon { h, slot: p[0], side: p[1] }
        }
        0x19 => {
            need(9)?;
            Rec::Image { h, ptr: u32le(p), w: u16le(&p[4..]), hh: u16le(&p[6..]), cf: p[8] }
        }
        0x12 => {
            need(1)?;
            Rec::LongMode { h, mode: p[0] }
        }
        _ => return Err(Error::Malformed),
    })
}

struct Records<'a> {
    buf: &'a [u8],
    done: bool,
}

impl<'a> Iterator for Records<'a> {
    type Item = Result<Rec<'a>, Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done || self.buf.is_empty() {
            return None;
        }
        let r = decode(&mut self.buf);
        self.done = r.is_err();
        Some(r)
    }
}

/// Decodes records back to back; stops after the first error.
pub fn records(buf: &[u8]) -> impl Iterator<Item = Result<Rec<'_>, Error>> {
    Records { buf, done: false }
}

// --- validation ---

fn cls(t: &Table, h: u16) -> Result<Class, Error> {
    t.class(h).ok_or(Error::BadHandle)
}

/// Any live handle except the root.
fn child(t: &Table, h: u16) -> Result<Class, Error> {
    if h == 0 {
        return Err(Error::BadHandle);
    }
    cls(t, h)
}

fn want(t: &Table, h: u16, ok: &[Class]) -> Result<(), Error> {
    if ok.contains(&cls(t, h)?) {
        Ok(())
    } else {
        Err(Error::BadClass)
    }
}

/// A handle about to be created (CREATE, SCROLLBAR). Handles above `MAX_OBJS` are the 193rd object: `Budget`
/// when all 192 are live, `BadHandle` otherwise.
fn fresh(t: &Table, h: u16) -> Result<(), Error> {
    if h == 0 {
        return Err(Error::BadHandle);
    }
    if h as usize > MAX_OBJS {
        return Err(if t.live() == MAX_OBJS { Error::Budget } else { Error::BadHandle });
    }
    if t.class[h as usize] != FREE {
        return Err(Error::BadHandle);
    }
    Ok(())
}

fn utf8(s: &[u8], max: usize) -> Result<(), Error> {
    if s.len() <= max && core::str::from_utf8(s).is_ok() {
        Ok(())
    } else {
        Err(Error::BadValue)
    }
}

fn check(t: &Table, fonts: &mut u8, r: &Rec) -> Result<(), Error> {
    let bad = |c: bool| if c { Err(Error::BadValue) } else { Ok(()) };
    match *r {
        Rec::Create { h, class, parent } => {
            fresh(t, h)?;
            // a pager holds pages only (each is the next row), and a page lives in a pager
            let p = cls(t, parent)?;
            if p == Class::ScrollBar || matches!(p, Class::Pager | Class::PagerH) != (class == Class::Page) {
                return Err(Error::BadClass);
            }
            // the loader keeps one music tile's images, fonts and objects
            bad(class == Class::Music && t.has(Class::Music))?;
            bad(class == Class::Image && t.class[1..].iter().filter(|&&c| c == Class::Image as u8).count() >= MAX_IMAGES)?;
        }
        Rec::Del(h) => {
            // a bar goes with its target: the target keeps callbacks into the bar (Ruling 13). A page goes with its
            // pager: the pages after it would keep their rows and leave a gap.
            if matches!(child(t, h)?, Class::ScrollBar | Class::Page) {
                return Err(Error::BadClass);
            }
        }
        Rec::Size { h, .. } => {
            child(t, h)?;
        }
        Rec::Align { h, align, .. } => {
            child(t, h)?;
            bad(align > 21)?;
        }
        Rec::Flags { h, set, clear } => {
            cls(t, h)?;
            bad((set | clear) & !FLAG_MASK != 0)?;
        }
        Rec::Style { h, prop, sel, value } => {
            cls(t, h)?;
            bad(ACCEPTED_PROPS.binary_search(&prop).is_err())?;
            bad(sel >> 16 > STYLE_PARTS || !STYLE_STATES.contains(&(sel & 0xFFFF)))?;
            if prop as u32 == LV_STYLE_TEXT_FONT {
                bad(!(0..MAX_FONTS as i32).contains(&value) || *fonts >> value & 1 == 0)?;
            }
        }
        Rec::Font { slot, font } => {
            bad(slot >= MAX_FONTS || font >= FONT_FILES)?;
            *fonts |= 1 << slot;
        }
        Rec::Text { h, s } => {
            want(t, h, &[Class::Label])?;
            utf8(s, MAX_TEXT)?;
        }
        Rec::Value { h, v, .. } => match cls(t, h)? {
            Class::Arc => bad(i16::try_from(v).is_err())?,
            Class::Bar | Class::Slider => {}
            Class::Roller => bad(u16::try_from(v).is_err())?,
            // the page to show; past the last one it shows the last
            Class::Pager | Class::PagerH => bad(!(0..=MAX_OBJS as i32).contains(&v))?,
            // the play button: 0 shows play, 1 pause (until the phone's state changes)
            Class::Music => bad(!(0..=1).contains(&v))?,
            _ => return Err(Error::BadClass),
        },
        Rec::Range { h, min, max } => {
            want(t, h, &[Class::Bar, Class::Slider])?;
            bad(min >= max)?;
        }
        Rec::Options { h, mode, s } => {
            want(t, h, &[Class::Roller])?;
            bad(mode > 1)?;
            utf8(s, MAX_OPTIONS)?;
        }
        Rec::Events { h, mask } => {
            child(t, h)?;
            bad(mask & !EVENT_MASK != 0)?;
        }
        Rec::Flex { h, flow, main, cross, track } => {
            cls(t, h)?;
            // LV_FLEX_FLOW: ROW 0, COLUMN 1, +WRAP 4, +REVERSE 8; LV_FLEX_ALIGN 0..=5
            bad(flow > 13 || flow & 2 != 0 || main > 5 || cross > 5 || track > 5)?;
        }
        Rec::ScrollBar { h, target } => {
            fresh(t, h)?;
            if child(t, target)? == Class::ScrollBar {
                return Err(Error::BadClass);
            }
        }
        Rec::Knob(h) => {
            if cls(t, h)? == Class::ScrollBar {
                return Err(Error::BadClass);
            }
        }
        Rec::Angles { h, start, end, rot } => {
            want(t, h, &[Class::Arc])?;
            bad(start > 360 || end > 360 || rot > 360)?;
        }
        Rec::Focus { h, icon } => {
            if child(t, h)? != Class::Obj {
                return Err(Error::BadClass);
            }
            bad(icon > 466 && icon != FOCUS_CARDS)?;
        }
        Rec::AppIcon { h, slot, side } => {
            want(t, h, &[Class::Obj])?;
            bad(slot >= crate::layout::SLOTS || !matches!(side, 64 | 96))?;
        }
        Rec::Image { h, w, hh, cf, .. } => {
            want(t, h, &[Class::Image])?;
            bad(!(1..=466).contains(&w) || !(1..=466).contains(&hh) || cf != 4)?;
        }
        Rec::LongMode { h, mode } => {
            want(t, h, &[Class::Label])?;
            // LV_LABEL_LONG_WRAP, DOT, SCROLL, SCROLL_CIRCULAR, CLIP
            bad(mode > 4)?;
        }
    }
    Ok(())
}

/// Dry run of a whole batch on a copy of `t`; `fonts_open` has bit `slot` set for every open font slot.
pub fn validate(buf: &[u8], t: &Table, fonts_open: u8) -> Result<(), Error> {
    let mut t = t.clone();
    let mut fonts = fonts_open;
    for r in records(buf) {
        let r = r?;
        check(&t, &mut fonts, &r)?;
        apply(&mut t, &r);
    }
    Ok(())
}

// --- table updates ---

fn get(s: &[u64; 4], x: usize) -> bool {
    x < 256 && s[x / 64] >> (x % 64) & 1 != 0
}

/// `h` and everything under it (children, and scroll bars targeting a member), as a bitset.
fn doomed(t: &Table, h: u16) -> [u64; 4] {
    let mut s = [0u64; 4];
    s[h as usize / 64] |= 1 << (h % 64);
    loop {
        let mut changed = false;
        for x in 1..=MAX_OBJS {
            if t.class[x] != FREE && !get(&s, x) && get(&s, t.parent[x] as usize) {
                s[x / 64] |= 1 << (x % 64);
                changed = true;
            }
        }
        if !changed {
            return s;
        }
    }
}

/// Updates the table for CREATE, DEL and SCROLLBAR (the loader calls it per record after the LVGL call).
/// The record must have passed `validate`.
pub fn apply(t: &mut Table, r: &Rec) {
    match *r {
        Rec::Create { h, class, parent } => {
            t.class[h as usize] = class as u8;
            t.parent[h as usize] = parent;
        }
        Rec::ScrollBar { h, target } => {
            t.class[h as usize] = Class::ScrollBar as u8;
            t.parent[h as usize] = target;
        }
        Rec::Del(h) if h != 0 => {
            let s = doomed(t, h);
            for x in 1..=MAX_OBJS {
                if get(&s, x) {
                    t.class[x] = FREE;
                    t.parent[x] = 0;
                }
            }
        }
        _ => {}
    }
}

/// The handles `DEL h` frees, in index order: `h`, its descendants and the scroll bars whose target is among them.
pub fn freed_by_del(t: &Table, h: u16) -> impl Iterator<Item = u16> {
    let mut s = doomed(t, h);
    if h == 0 {
        s[0] &= !1;
    }
    (1..=MAX_OBJS as u16).filter(move |&x| get(&s, x as usize))
}

/// FOCUS: the most children a focus container shapes (later ones are left as they are).
pub const FOCUS_MAX: usize = 32;
/// FOCUS: the LV event code the app gets when the child nearest the centre changes (`delta` = its index).
pub const FOCUSED: i16 = 100;
/// FOCUS `icon` for the stock Exercise list: each child is a card that narrows with its distance from the centre
/// line (`card_width`) instead of shifting its content; text fades as in the app list.
pub const FOCUS_CARDS: u16 = 0xFFFF;
/// Width of a FOCUS_CARDS card: the stock Exercise list's 434 px in the centre, 2 × the app list's shift less
/// further out (370 px one 104 px row away, as in the stock list).
pub fn card_width(d: i32) -> i32 {
    434 - 2 * focus_shape(d).0
}

/// The stock app list's fisheye (`0x1005ED98`, docs/recon.md) for a child `d` px from the centre line: (x shift,
/// zoom of child 0 in 1/256, text opacity). The stock x is `16 + shift`; past 280 px it stays as at 280.
pub fn focus_shape(d: i32) -> (i32, u32, u32) {
    match d.clamp(0, 280) {
        d @ 0..104 => (d * 4 / 13, 0x100, 0xFF),
        d => ((d - 104) * 25 / 48 + 32, (256 - (90 * d - 9360) / 177) as u32, (255 - (180 * d - 18720) / 177) as u32),
    }
}

/// WIDGET record (op 0x14, handle 0, payload = the widget record, 1..=128 bytes, docs/app-guide.md "Widget"), alone
/// in a batch: the loader writes it to the app's N.wgt on the app thread. Up to loader-2026-11-22-884e9a6 it took exactly
/// 64 bytes; on an older loader still op 0x14 is `Malformed`.
pub fn widget(buf: &[u8]) -> Option<&[u8]> {
    match buf {
        [0x14, n, 0, 0, rec @ ..] if usize::from(*n) == rec.len() && (1..=crate::widget::REC_MAX).contains(&rec.len()) => {
            Some(rec)
        }
        _ => None,
    }
}

/// BLIT record (op 0x1B, handle 0, payload `ptr u32, x i16, y i16, w u16, h u16`), alone in a batch: the loader
/// copies `w * h` RGB565 pixels from the app block into the framebuffer (docs/bitmap-assets.md). It runs on the app
/// thread before LVGL, like SENSOR, so a canvas app may send it. Inside a longer batch, and on an older loader,
/// op 0x1B is `None` and the batch falls through to the LVGL path (`Malformed`, -22). An odd `ptr` is `None` too:
/// the loader reads the pixels as `u16`.
pub fn blit(buf: &[u8]) -> Option<(u32, i16, i16, u16, u16)> {
    match *buf {
        [0x1B, 12, 0, 0, p0, p1, p2, p3, x0, x1, y0, y1, w0, w1, h0, h1]
            if p0 & 1 == 0
                && (1..=466).contains(&u16::from_le_bytes([w0, w1]))
                && (1..=466).contains(&u16::from_le_bytes([h0, h1])) => {
            Some((u32::from_le_bytes([p0, p1, p2, p3]), i16::from_le_bytes([x0, x1]), i16::from_le_bytes([y0, y1]),
                  u16::from_le_bytes([w0, w1]), u16::from_le_bytes([h0, h1])))
        }
        _ => None,
    }
}

/// Whether every IMAGE_SRC source in `buf` points at `w * hh * 2` pixel bytes, 2-aligned, inside
/// `[block, block + size)` (the running app's image). The batch itself is range-checked in `lv::submit`; this is the same check for the
/// pixels it names, so a record can never make the loader render memory outside the app. A malformed record returns
/// `true`: the batch validator rejects the batch before any record runs.
pub fn image_src_in(buf: &[u8], block: u32, size: u32) -> bool {
    for r in records(buf) {
        // cf is 4 (RGB565) only: `check` refuses the rest, so 2 bytes per pixel
        let Ok(Rec::Image { ptr, w, hh, .. }) = r else { continue };
        let (start, end) = (u64::from(ptr), u64::from(ptr) + u64::from(w) * u64::from(hh) * 2);
        let (lo, hi) = (u64::from(block), u64::from(block) + u64::from(size));
        if ptr & 1 != 0 || start < lo || end > hi {
            return false;
        }
    }
    true
}

/// PAGE query (op 0x15, handle 0, no payload), alone in a batch: 1 when the app runs as its page next to the watch
/// face (docs/app-guide.md, "Page"), else 0. On an older loader op 0x15 is `Malformed`.
pub fn page_query(buf: &[u8]) -> bool {
    buf == [0x15, 0, 0, 0]
}

/// SENSOR record (op 0x13, handle 0, payload `[sensor, on]`), alone in a batch: the loader takes it on the app
/// thread before LVGL, so a canvas app may send it (docs/abi-1.4.md, "Heart rate"). Inside a longer batch, and
/// on an older loader, op 0x13 is `Malformed` (-22).
pub fn sensor(buf: &[u8]) -> Option<(u8, bool)> {
    match *buf {
        [0x13, 2, 0, 0, sensor, on @ (0 | 1)] => Some((sensor, on == 1)),
        _ => None,
    }
}

/// GPS_FIX record (op 0x18, handle 0, payload = `gps::FIX_LEN` bytes the loader overwrites with the latest fix),
/// alone in a batch (docs/abi-1.4.md, "GPS"). On an older loader op 0x18 is `Malformed` (-22).
pub fn gps_fix(buf: &[u8]) -> bool {
    buf.len() == 4 + crate::gps::FIX_LEN && buf[..4] == [0x18, crate::gps::FIX_LEN as u8, 0, 0]
}

/// LV_DIR_LEFT / LV_DIR_RIGHT.
pub const DIR_LEFT: u8 = 1;
pub const DIR_RIGHT: u8 = 2;

/// Scroll directions of the tile added as column `col` of a horizontal pager, as the stock `sport1_view` sets its
/// three (2, 3, 1): the first goes right, the newest (last) left; the one before the newest gains RIGHT
/// (`prev`, None for the first).
pub fn hpage_dirs(col: u32) -> (u8, Option<u8>) {
    match col {
        0 => (DIR_RIGHT, None),
        1 => (DIR_LEFT, Some(DIR_RIGHT)),
        _ => (DIR_LEFT, Some(DIR_LEFT | DIR_RIGHT)),
    }
}

/// MEDIA_KEY commands (ux2sys 0x4E, as the stock music view and workout screen send them).
pub const MEDIA_PLAY: u8 = 1;
pub const MEDIA_PAUSE: u8 = 2;
pub const MEDIA_PREV: u8 = 3;
pub const MEDIA_NEXT: u8 = 4;

/// MEDIA_KEY record (op 0x16, handle 0, payload `[cmd]`, cmd 1 play, 2 pause, 3 previous, 4 next, 5 volume up,
/// 6 volume down), alone in a batch (docs/abi-1.4.md, "Music"). On an older loader op 0x16 is `Malformed` (-22);
/// the loaders of 2026-11-16 to 2026-11-19 answer `-22` to 5 and 6.
pub fn media_key(buf: &[u8]) -> Option<u8> {
    match *buf {
        [0x16, 1, 0, 0, cmd @ MEDIA_PLAY..=MEDIA_VOLUME_DOWN] => Some(cmd),
        _ => None,
    }
}

/// MEDIA_BIND record (op 0x17, handle 0, payload `[on, title u16, artist u16]`), alone in a batch: on = 1 the
/// loader writes the phone's track title and artist into those labels (handle 0 = none) and sends MEDIA events;
/// on = 0 stops it. On an older loader op 0x17 is `Malformed` (-22).
pub fn media_bind(buf: &[u8]) -> Option<(bool, u16, u16)> {
    match *buf {
        [0x17, 5, 0, 0, on @ (0 | 1), t0, t1, a0, a1] => Some((on == 1, u16::from_le_bytes([t0, t1]), u16::from_le_bytes([a0, a1]))),
        _ => None,
    }
}

/// Music tile images: SPORT1_IMGS[MUSIC_IMG_FIRST..][..MUSIC_IMGS], indices into that run.
pub const MUSIC_IMG_FIRST: usize = 6;
pub const MUSIC_IMGS: usize = 13;
pub const IMG_PAUSE: usize = 0;
pub const IMG_PREV: usize = 1;
pub const IMG_NEXT: usize = 2;
pub const IMG_VOLUME: usize = 3;
pub const IMG_PLAY: usize = 4;
pub const IMG_PLAY_OFF: usize = 6;
/// The volume popup's: muted and on icons, slider track and fill.
pub const IMG_MUTED: usize = 9;
pub const IMG_VOLUME_ON: usize = 10;
pub const IMG_TRACK: usize = 11;
pub const IMG_FILL: usize = 12;
/// ux2sys 0x4E volume commands: the volume popup's buttons and crown, and MEDIA_KEY from an app.
pub const MEDIA_VOLUME_UP: u8 = 5;
pub const MEDIA_VOLUME_DOWN: u8 = 6;

/// The volume popup's crown (sport1_view 0x10078FE4): six knob events one way make one volume command, with a
/// vibration when the volume can still move that way. `counts` = (up, down); a positive step is up.
pub fn volume_knob(counts: &mut (u8, u8), step: i32, cur: u8, max: u8) -> Option<(u8, bool)> {
    if step > 0 {
        *counts = (counts.0.wrapping_add(1), 0);
    } else if step < 0 {
        *counts = (0, counts.1.wrapping_add(1));
    }
    if counts.0 > 5 {
        counts.0 = 0;
        return Some((MEDIA_VOLUME_UP, cur < max));
    }
    if counts.1 > 5 {
        counts.1 = 0;
        return Some((MEDIA_VOLUME_DOWN, cur != 0));
    }
    None
}
/// "No music": SPORT1_TEXTS[14].
pub const TEXT_NO_MUSIC: usize = 14;
/// Music tile buttons: the `delta` of the app's `LV` CLICKED event (`x` = the tile's handle).
pub const MUSIC_PLAY: i16 = 0;
pub const MUSIC_PREV: i16 = 1;
pub const MUSIC_NEXT: i16 = 2;
pub const MUSIC_VOLUME: i16 = 3;

/// The play button's image, as the stock tile picks it (`0x101AF03C`): grey play without phone media, else pause
/// while playing and play while paused.
pub fn music_icon(available: bool, playing: bool) -> usize {
    match (available, playing) {
        (false, _) => IMG_PLAY_OFF,
        (true, true) => IMG_PAUSE,
        (true, false) => IMG_PLAY,
    }
}

/// A MEDIA_BIND label: 0 (none) or a live label.
pub fn media_label_ok(t: &Table, h: u16) -> bool {
    h == 0 || t.class(h) == Some(Class::Label)
}

/// ux2sys 0x4F, as the stock reads it: title in `[0..0x40]`, artist in `[0x41..0x81]`, each up to its first NUL
/// (the stock writes the NUL at 0x40 and 0x81 itself). Invalid UTF-8 is cut at the last valid char.
pub fn media_info(buf: &[u8; 0x82]) -> (&[u8], &[u8]) {
    fn field(f: &[u8]) -> &[u8] {
        let f = &f[..f.iter().position(|&c| c == 0).unwrap_or(f.len())];
        match core::str::from_utf8(f) {
            Ok(_) => f,
            Err(e) => &f[..e.valid_up_to()],
        }
    }
    (field(&buf[..0x40]), field(&buf[0x41..0x81]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stock::STYLE_PROPS;

    const TEXT_FONT: u16 = 87;

    fn r(op: u8, h: u16, p: &[u8]) -> Vec<u8> {
        let mut v = vec![op, p.len() as u8];
        v.extend_from_slice(&h.to_le_bytes());
        v.extend_from_slice(p);
        v
    }
    #[test]
    fn installed_icon_records() {
        let native = [0x1A, 2, 1, 0, 3, 96];
        assert!(matches!(decode(&mut &native[..]), Ok(Rec::AppIcon { h: 1, slot: 3, side: 96 })));
        let short = [0x1A, 1, 1, 0, 3];
        assert!(decode(&mut &short[..]).is_err());
    }
    #[test]
    fn installed_icon_validation() {
        assert_eq!(check(&[create(1, 0, 0), r(0x1A, 1, &[15, 96])]), Ok(()));
        assert_eq!(check(&[create(1, 0, 0), r(0x1A, 1, &[0, 64])]), Ok(()));
        assert_eq!(check(&[create(1, 0, 0), r(0x1A, 1, &[16, 96])]), Err(Error::BadValue));
        assert_eq!(check(&[create(1, 0, 0), r(0x1A, 1, &[0, 65])]), Err(Error::BadValue));
        assert_eq!(check(&[create(1, 1, 0), r(0x1A, 1, &[0, 96])]), Err(Error::BadClass));
        assert_eq!(check(&[r(0x1A, 1, &[0, 96])]), Err(Error::BadHandle));
    }
    #[test]
    fn image_records() {
        let src = |h: u16, w: u16, hh: u16, cf: u8| {
            let mut p = 0x0010_0000u32.to_le_bytes().to_vec();
            p.extend_from_slice(&w.to_le_bytes());
            p.extend_from_slice(&hh.to_le_bytes());
            p.push(cf);
            r(0x19, h, &p)
        };
        assert!(matches!(decode(&mut &src(12, 64, 32, 4)[..]),
                         Ok(Rec::Image { h: 12, ptr: 0x0010_0000, w: 64, hh: 32, cf: 4 })));
        assert!(decode(&mut &src(12, 64, 0, 4)[..]).is_ok(), "dims are checked, not decoded");
        let image = |w: u16, hh: u16, cf: u8| vec![create(12, Class::Image as u8, 0), src(12, w, hh, cf)];
        assert_eq!(check(&image(64, 32, 4)), Ok(()));
        assert_eq!(check(&image(0, 32, 4)), Err(Error::BadValue));
        assert_eq!(check(&image(467, 32, 4)), Err(Error::BadValue));
        assert_eq!(check(&image(64, 32, 5)), Err(Error::BadValue), "alpha waits");
        assert_eq!(check(&[src(1, 64, 32, 4)]), Err(Error::BadHandle));
        let mut many: Vec<Vec<u8>> = (1..=MAX_IMAGES as u16).map(|h| create(h, Class::Image as u8, 0)).collect();
        assert_eq!(check(&many), Ok(()));
        many.push(create(20, Class::Image as u8, 0));
        assert_eq!(check(&many), Err(Error::BadValue), "one descriptor per image in the loader");
    }
    #[test]
    fn blit_record_alone() {
        let good = [0x1B, 12, 0, 0, 0x00, 0x00, 0x10, 0x00, 0x10, 0xFF, 0x20, 0xFF, 64, 0, 32, 0];
        assert_eq!(blit(&good), Some((0x0010_0000, -240, -224, 64, 32)));
        assert_eq!(blit(&[0x1B, 12, 0, 0, 0, 0, 0x10, 0, 0, 0, 0, 0, 0, 0, 32, 0]), None, "w 0");
        assert_eq!(blit(&[0x1B, 12, 0, 0, 0, 0, 0x10, 0, 0, 0, 0, 0, 64, 0]), None, "short");
        assert_eq!(blit(&[0x1B, 12, 0, 0, 1, 0, 0x10, 0, 0, 0, 0, 0, 64, 0, 32, 0]), None, "odd ptr");
        let mut longer = good.to_vec();
        longer.extend_from_slice(&[0x13, 2, 0, 0, 1, 1]);
        assert_eq!(blit(&longer), None, "alone in its batch");
    }
    #[test]
    fn image_sources_stay_in_the_app_block() {
        let src = |ptr: u32| {
            let mut p = ptr.to_le_bytes().to_vec();
            p.extend_from_slice(&4u16.to_le_bytes());
            p.extend_from_slice(&4u16.to_le_bytes());
            p.push(4);
            r(0x19, 12, &p)
        };
        assert!(image_src_in(&src(0x1000), 0x1000, 32), "4*4*2 = 32 B, exact fit");
        assert!(!image_src_in(&src(0x0FFE), 0x1000, 32), "starts before");
        assert!(!image_src_in(&src(0x1000), 0x1000, 31), "runs past");
        assert!(!image_src_in(&src(0x1001), 0x1000, 64), "odd: the pixels are u16");
        assert!(image_src_in(&[0x99, 3], 0x1000, 1), "malformed: the validator rejects it");
    }
    fn create(h: u16, class: u8, parent: u16) -> Vec<u8> {
        let mut p = vec![class];
        p.extend_from_slice(&parent.to_le_bytes());
        r(1, h, &p)
    }
    fn del(h: u16) -> Vec<u8> {
        r(2, h, &[])
    }
    fn align(h: u16, a: u8) -> Vec<u8> {
        r(4, h, &[a, 0, 0, 0, 0])
    }
    fn flags(h: u16, set: u32, clear: u32) -> Vec<u8> {
        let mut p = set.to_le_bytes().to_vec();
        p.extend_from_slice(&clear.to_le_bytes());
        r(5, h, &p)
    }
    fn style(h: u16, prop: u16, value: i32) -> Vec<u8> {
        style_sel(h, prop, 0, value)
    }
    fn style_sel(h: u16, prop: u16, sel: u32, value: i32) -> Vec<u8> {
        let mut p = prop.to_le_bytes().to_vec();
        p.extend_from_slice(&sel.to_le_bytes());
        p.extend_from_slice(&value.to_le_bytes());
        r(6, h, &p)
    }
    fn font(slot: u8, f: u8) -> Vec<u8> {
        r(7, 0, &[slot, f])
    }
    fn text(h: u16, s: &[u8]) -> Vec<u8> {
        r(8, h, s)
    }
    fn value(h: u16, v: i32) -> Vec<u8> {
        let mut p = v.to_le_bytes().to_vec();
        p.push(0);
        r(9, h, &p)
    }
    fn range(h: u16, min: i32, max: i32) -> Vec<u8> {
        let mut p = min.to_le_bytes().to_vec();
        p.extend_from_slice(&max.to_le_bytes());
        r(0x0A, h, &p)
    }
    fn options(h: u16, s: &[u8]) -> Vec<u8> {
        let mut p = vec![0];
        p.extend_from_slice(s);
        r(0x0B, h, &p)
    }
    fn events(h: u16, mask: u8) -> Vec<u8> {
        r(0x0C, h, &[mask])
    }
    fn scrollbar(h: u16, target: u16) -> Vec<u8> {
        r(0x0E, h, &target.to_le_bytes())
    }

    fn check(batch: &[Vec<u8>]) -> Result<(), Error> {
        validate(&batch.concat(), &Table::new(), 0)
    }
    fn built(batch: &[Vec<u8>]) -> Table {
        let mut t = Table::new();
        for rec in records(&batch.concat()) {
            apply(&mut t, &rec.unwrap());
        }
        t
    }

    #[test]
    fn style_props_are_sorted_unique_48() {
        assert_eq!(STYLE_PROPS.len(), 48);
        assert!(STYLE_PROPS.windows(2).all(|w| w[0] < w[1]));
        // accepted = the stock setters minus the pointer and unproven ids
        let deny = [40, 45, 84, 92, 93, 102];
        let want: Vec<u16> = STYLE_PROPS.iter().copied().filter(|p| !deny.contains(p)).collect();
        assert_eq!(ACCEPTED_PROPS, *want);
    }

    /// The SDK's encoder wrote this file (`cmf_wp2_app::lv` test `golden_batch_bytes`): one record of every op.
    const GOLDEN: &[u8] = include_bytes!("../../../tests/fixtures/lvcmd-golden.bin");

    #[test]
    fn golden_batch_decodes() {
        let got: Vec<_> = records(GOLDEN).collect::<Result<_, _>>().unwrap();
        let ru = "Привет".as_bytes();
        assert_eq!(
            got,
            [
                // the spec's example batch
                Rec::Create { h: 1, class: Class::Label, parent: 0 },
                Rec::Font { slot: 0, font: 7 },
                Rec::Style { h: 1, prop: TEXT_FONT, sel: 0, value: 0 },
                Rec::Text { h: 1, s: ru },
                Rec::Align { h: 1, align: 9, x: 0, y: 0 },
                Rec::Events { h: 1, mask: 2 },
                // every other class and op, payload offsets pinned by distinct values
                Rec::Create { h: 2, class: Class::Obj, parent: 0 },
                Rec::Create { h: 3, class: Class::Btn, parent: 2 },
                Rec::Create { h: 4, class: Class::Arc, parent: 2 },
                Rec::Create { h: 5, class: Class::Bar, parent: 2 },
                Rec::Create { h: 6, class: Class::Slider, parent: 2 },
                Rec::Create { h: 7, class: Class::Roller, parent: 2 },
                Rec::Size { h: 2, w: 0x2000_0064, hh: 0x2000_07D1 },
                Rec::Flags { h: 2, set: 1 << 4, clear: 1 },
                Rec::Value { h: 4, v: -5, anim: true },
                Rec::Range { h: 5, min: 0, max: 100 },
                Rec::Options { h: 7, mode: 1, s: b"a\nb\nc" },
                Rec::Flex { h: 2, flow: 1, main: 2, cross: 2, track: 0 },
                Rec::ScrollBar { h: 8, target: 2 },
                Rec::Knob(2),
                Rec::Angles { h: 4, start: 135, end: 45, rot: 10 },
                Rec::Focus { h: 2, icon: 96 },
                Rec::LongMode { h: 1, mode: 1 },
                Rec::Create { h: 9, class: Class::Obj, parent: 0 },
                Rec::Del(9),
            ]
        );
        assert_eq!(validate(GOLDEN, &Table::new(), 0), Ok(()));
        assert_eq!(cmf_wp2_app::lv::FOCUSED, FOCUSED);
    }

    #[test]
    fn sdk_props_are_the_accepted_props() {
        use cmf_wp2_app::lv::Prop;
        let mut ids: Vec<u16> = Prop::ALL.iter().map(|&p| p as u16).collect();
        ids.sort_unstable();
        assert_eq!(ids, ACCEPTED_PROPS);
        assert_eq!(Prop::TextFont as u32, LV_STYLE_TEXT_FONT);
    }

    #[test]
    fn truncated_and_unknown_records_are_malformed() {
        let bad: [&[u8]; 6] = [
            &[1, 3, 1],                // header cut
            &[1, 3, 1, 0, 1],          // len past the end
            &[0x11, 0, 1, 0],          // unknown op
            &[0x00, 0, 1, 0],          // unknown op
            &[1, 2, 1, 0, 1, 0],       // CREATE payload must be 3
            &[2, 1, 1, 0, 0],          // DEL has no payload
        ];
        for b in bad {
            assert_eq!(records(b).next(), Some(Err(Error::Malformed)), "{b:?}");
            assert_eq!(validate(b, &Table::new(), 0), Err(Error::Malformed), "{b:?}");
        }
        assert_eq!(Error::Malformed.code(), -22);
        assert_eq!(Error::BadValue.code(), -22);
        assert_eq!(Error::Budget.code(), -12);
    }

    #[test]
    fn handles_are_checked() {
        assert_eq!(check(&[create(1, 0, 0), create(1, 0, 0)]), Err(Error::BadHandle));
        assert_eq!(check(&[create(0, 0, 0)]), Err(Error::BadHandle));
        assert_eq!(check(&[create(193, 0, 0)]), Err(Error::BadHandle));
        assert_eq!(check(&[create(1, 0, 9)]), Err(Error::BadHandle));
        assert_eq!(check(&[style(5, TEXT_FONT, 0)]), Err(Error::BadHandle));
        assert_eq!(check(&[text(5, b"x")]), Err(Error::BadHandle));
        assert_eq!(check(&[del(5)]), Err(Error::BadHandle));
        assert_eq!(check(&[del(0)]), Err(Error::BadHandle));
        // a scroll bar goes with its target only
        let list = [create(1, 0, 0), scrollbar(2, 1)];
        assert_eq!(check(&[list[0].clone(), list[1].clone(), del(2)]), Err(Error::BadClass));
        assert_eq!(check(&[list[0].clone(), list[1].clone(), del(1)]), Ok(()));
        assert_eq!(check(&[create(1, 1, 0), value(1, 5)]), Err(Error::BadClass));
        assert_eq!(check(&[create(1, 4, 0), options(1, b"a\nb")]), Err(Error::BadClass));
        assert_eq!(check(&[create(1, 3, 0), text(1, b"x")]), Err(Error::BadClass));
        // RANGE only on bar and slider
        assert_eq!(check(&[create(1, 3, 0), range(1, 0, 10)]), Err(Error::BadClass));
        assert_eq!(check(&[create(1, 4, 0), range(1, 0, 10)]), Ok(()));
        assert_eq!(check(&[create(1, 5, 0), range(1, 0, 10)]), Ok(()));
    }

    #[test]
    fn values_are_checked() {
        let bv = Err(Error::BadValue);
        assert_eq!(check(&[create(1, 0, 0), style(1, 2, 0)]), bv);
        // pointer and unproven props are refused (Ruling 14)
        for p in [40, 45, 84, 92, 93, 102] {
            assert_eq!(check(&[create(1, 0, 0), style(1, p, 0)]), bv, "{p}");
        }
        assert_eq!(check(&[create(1, 0, 0), style(1, 96, 0)]), Ok(()));
        // selector: parts 0..=5, one state of DEFAULT, CHECKED, FOCUSED, PRESSED, DISABLED (Ruling 15)
        for sel in [0, 0x0005_0000, 0x0002_0020, 0x0001_0080, 0x01, 0x02] {
            assert_eq!(check(&[create(1, 0, 0), style_sel(1, 32, sel, 0)]), Ok(()), "{sel:#x}");
        }
        for sel in [0x0006_0000, 0x000F_0000, 0x04, 0x21, 0x8000, 0x00FF_FFFF] {
            assert_eq!(check(&[create(1, 0, 0), style_sel(1, 32, sel, 0)]), bv, "{sel:#x}");
        }
        assert_eq!(check(&[create(1, 0, 0), flags(1, 1 << 2, 0)]), bv);
        assert_eq!(check(&[create(1, 0, 0), flags(1, 0, 1 << 7)]), bv);
        assert_eq!(check(&[create(1, 0, 0), flags(1, 0x7B, 0)]), Ok(()));
        assert_eq!(check(&[create(1, 0, 0), align(1, 22)]), bv);
        assert_eq!(check(&[create(1, 0, 0), align(1, 21)]), Ok(()));
        assert_eq!(check(&[font(4, 0)]), bv);
        assert_eq!(check(&[font(0, 16)]), bv);
        assert_eq!(check(&[create(1, 7, 0)]), bv);
        assert_eq!(check(&[create(1, 1, 0), text(1, &[b'a'; 129])]), bv);
        assert_eq!(check(&[create(1, 1, 0), text(1, &[b'a'; 128])]), Ok(()));
        assert_eq!(check(&[create(1, 1, 0), text(1, &[0xFF, 0xFE])]), bv);
        assert_eq!(check(&[create(1, 6, 0), options(1, &[b'a'; 241])]), bv);
        assert_eq!(check(&[create(1, 6, 0), options(1, &[b'a'; 240])]), Ok(()));
        // arc value is i16, roller index >= 0
        assert_eq!(check(&[create(1, 3, 0), value(1, 40000)]), bv);
        assert_eq!(check(&[create(1, 3, 0), value(1, -5)]), Ok(()));
        assert_eq!(check(&[create(1, 6, 0), value(1, -1)]), bv);
        // TEXT_FONT needs an opened slot
        let tf = [create(1, 1, 0), style(1, TEXT_FONT, 0)];
        assert_eq!(check(&tf), bv);
        assert_eq!(check(&[font(0, 3), tf[0].clone(), tf[1].clone()]), Ok(()));
        assert_eq!(validate(&tf.concat(), &Table::new(), 1), Ok(()));
        assert_eq!(validate(&[create(1, 1, 0), style(1, TEXT_FONT, 2)].concat(), &Table::new(), 1), bv);
        assert_eq!(validate(&[create(1, 1, 0), style(1, TEXT_FONT, 4)].concat(), &Table::new(), 0xFF), bv);
    }

    #[test]
    fn budget() {
        let full: Vec<_> = (1..=192).map(|h| create(h, 0, 0)).collect();
        assert_eq!(check(&full), Ok(()));
        let t = built(&full);
        assert_eq!(validate(&create(193, 0, 0), &t, 0), Err(Error::Budget));
        assert_eq!(validate(&create(1, 0, 0), &t, 0), Err(Error::BadHandle));
        assert_eq!(check(&[font(0, 1), font(0, 2)]), Ok(()));
    }

    #[test]
    fn del_frees_descendants_and_bars() {
        let t = built(&[
            create(1, 0, 0),
            create(2, 0, 1),
            create(3, 0, 2),
            scrollbar(4, 1),
            create(5, 0, 0),
        ]);
        let freed: Vec<u16> = freed_by_del(&t, 1).collect();
        assert_eq!(freed, [1, 2, 3, 4]);
        assert_eq!(freed_by_del(&t, 2).collect::<Vec<_>>(), [2, 3]);

        let mut t2 = t.clone();
        apply(&mut t2, &Rec::Del(1));
        assert_eq!(t2.class(1), None);
        assert_eq!(t2.class(3), None);
        assert_eq!(t2.class(4), None);
        assert_eq!(t2.class(5), Some(Class::Obj));
        assert_eq!(validate(&style(3, TEXT_FONT, 0), &t2, 1), Err(Error::BadHandle));
        // inside one batch too
        assert_eq!(
            validate(&[del(1), style(3, TEXT_FONT, 0)].concat(), &t, 1),
            Err(Error::BadHandle)
        );
        assert_eq!(validate(&style(5, TEXT_FONT, 0), &t2, 1), Ok(()));
        assert_eq!(t.class(4), Some(Class::ScrollBar));
    }

    #[test]
    fn invalid_batch_changes_nothing() {
        let t = built(&[create(1, 0, 0)]);
        let before = t.clone();
        let batch = [create(2, 0, 1), create(3, 1, 2), del(1), create(4, 0, 0), style(3, 2, 0)].concat();
        assert_eq!(validate(&batch, &t, 0), Err(Error::BadHandle));
        assert_eq!(t, before);
    }

    #[test]
    fn focus_record_and_shape() {
        let mut t = Table::new();
        let mut b = std::vec::Vec::new();
        b.extend([0x01, 3, 1, 0, 0, 0, 0]); // CREATE obj 1 on the root
        b.extend([0x01, 3, 2, 0, 1, 0, 0]); // CREATE label 2
        let f = |h: u16, icon: u16| {
            let mut r = std::vec::Vec::from([0x11u8, 2]);
            r.extend(h.to_le_bytes());
            r.extend(icon.to_le_bytes());
            r
        };
        assert_eq!(validate(&[&b[..], &f(1, 96)].concat(), &t, 0), Ok(()));
        assert_eq!(records(&f(1, 96)).next(), Some(Ok(Rec::Focus { h: 1, icon: 96 })));
        for r in records(&b) {
            apply(&mut t, &r.unwrap());
        }
        assert_eq!(validate(&f(0, 96), &t, 0), Err(Error::BadHandle), "the root");
        assert_eq!(validate(&f(2, 96), &t, 0), Err(Error::BadClass), "a label");
        assert_eq!(validate(&f(3, 96), &t, 0), Err(Error::BadHandle), "a free handle");
        assert_eq!(validate(&f(1, 467), &t, 0), Err(Error::BadValue));
        assert_eq!(validate(&f(1, FOCUS_CARDS), &t, 0), Ok(()), "cards");
        assert_eq!((card_width(0), card_width(104), card_width(9999)), (434, 370, 188));
        assert_eq!(validate(&[0x11, 1, 1, 0, 9], &t, 0), Err(Error::Malformed));
        assert_eq!(validate(&[0x12, 1, 2, 0, 1], &t, 0), Ok(()), "LONG_MODE DOT on a label");
        assert_eq!(validate(&[0x12, 1, 2, 0, 5], &t, 0), Err(Error::BadValue));
        assert_eq!(validate(&[0x12, 1, 1, 0, 1], &t, 0), Err(Error::BadClass), "not a label");
        // the stub's float-matched integers (fw/wp2-stub applist.rs shape) less its base x of 16
        assert_eq!(focus_shape(0), (0, 256, 255));
        assert_eq!(focus_shape(103), (31, 256, 255));
        assert_eq!(focus_shape(104), (32, 256, 255));
        assert_eq!(focus_shape(280), (123, 167, 77));
        assert_eq!(focus_shape(5000), focus_shape(280));
        assert_eq!(focus_shape(-5), focus_shape(0));
        let mut last = focus_shape(0);
        for d in 1..=280 {
            let s = focus_shape(d);
            assert!(s.0 >= last.0 && s.1 <= last.1 && s.2 <= last.2, "monotonic at {d}");
            last = s;
        }
    }

    #[test]
    fn pager_holds_pages() {
        let t = built(&[create(1, 8, 0), create(2, 9, 1), create(3, 1, 2)]);
        assert_eq!(t.class(1), Some(Class::Pager));
        assert_eq!(t.class(2), Some(Class::Page));
        let v = |b: &[Vec<u8>]| validate(&b.concat(), &t, 0);
        assert_eq!(v(&[create(4, 9, 1)]), Ok(()));
        assert_eq!(v(&[create(4, 9, 0)]), Err(Error::BadClass), "a page outside a pager");
        assert_eq!(v(&[create(4, 9, 2)]), Err(Error::BadClass), "a page in a page");
        assert_eq!(v(&[create(4, 0, 1)]), Err(Error::BadClass), "a pager holds pages only");
        assert_eq!(v(&[create(4, 8, 2)]), Ok(()), "a pager inside a page");
        assert_eq!(v(&[del(2)]), Err(Error::BadClass), "a page goes with its pager");
        assert_eq!(v(&[del(1), create(1, 0, 0)]), Ok(()));
        assert_eq!(freed_by_del(&t, 1).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(v(&[value(1, 3)]), Ok(()));
        assert_eq!(v(&[value(1, -1)]), Err(Error::BadValue));
        assert_eq!(v(&[value(1, 193)]), Err(Error::BadValue));
        assert_eq!(v(&[value(2, 0)]), Err(Error::BadClass));
        assert_eq!(v(&[r(1, 4, &[13, 0, 0])]), Err(Error::BadValue), "no class 13");
    }

    #[test]
    fn page_query_alone() {
        assert!(page_query(&[0x15, 0, 0, 0]));
        assert!(!page_query(&[0x15, 0, 0, 0, 0]));
        assert!(!page_query(&[0x14, 0, 0, 0]));
    }

    #[test]
    fn widget_record() {
        let rec = cmf_wp2_app::widget_record("Vibration Lab", "Strength 35%\nFreq 170Hz");
        let got = widget(&rec).expect("a widget record");
        assert_eq!(&got[..14], b"Vibration Lab\0");
        assert_eq!(&got[24..48], b"Strength 35%\nFreq 170Hz\0");
        assert_eq!(widget(&rec[..67]), None, "length byte says 64");
        let mut v2 = vec![0x14, 87, 0, 0];
        v2.extend_from_slice(&[0; 87]);
        assert_eq!(widget(&v2).map(<[u8]>::len), Some(87));
        assert_eq!(widget(&v2[..90]), None, "length byte and payload differ");
        let mut big = vec![0x14, 129, 0, 0];
        big.extend_from_slice(&[0; 129]);
        assert_eq!(widget(&big), None);
        assert_eq!(widget(&[0x13, 2, 0, 0, 1, 1]), None);
    }

    #[test]
    fn sensor_record() {
        assert_eq!(sensor(&[0x13, 2, 0, 0, 1, 1]), Some((1, true)));
        assert_eq!(sensor(&[0x13, 2, 0, 0, 1, 0]), Some((1, false)));
        assert_eq!(sensor(&[0x13, 2, 0, 0, 1, 2]), None);
        assert_eq!(sensor(&[0x13, 2, 1, 0, 1, 1]), None, "handle 0 only");
        assert_eq!(sensor(&[0x13, 2, 0, 0, 1, 1, 0x02, 0, 1, 0]), None, "alone in its batch");
        // the SDK's records, and the -22 an older loader (or a mixed batch) answers
        assert_eq!(sensor(cmf_wp2_app::HEART_ON), Some((crate::heart::SENSOR_HEART, true)));
        assert_eq!(sensor(cmf_wp2_app::HEART_OFF), Some((crate::heart::SENSOR_HEART, false)));
        assert_eq!(validate(cmf_wp2_app::HEART_ON, &Table::new(), 0), Err(Error::Malformed));
        assert_eq!(sensor(cmf_wp2_app::GPS_ON), Some((crate::gps::SENSOR_GPS, true)));
        assert_eq!(sensor(cmf_wp2_app::GPS_OFF), Some((crate::gps::SENSOR_GPS, false)));
    }

    #[test]
    fn gps_fix_record() {
        let mut buf = [0u8; 4 + crate::gps::FIX_LEN];
        buf[..4].copy_from_slice(&[0x18, 20, 0, 0]);
        assert!(gps_fix(&buf));
        assert!(!gps_fix(&buf[..23]), "exact length");
        buf[2] = 1;
        assert!(!gps_fix(&buf), "handle 0 only");
        buf[2] = 0;
        assert_eq!(validate(&buf, &Table::new(), 0), Err(Error::Malformed));
    }

    #[test]
    fn hpager() {
        let mut t = Table::new();
        assert_eq!(check(&[create(1, 10, 0), create(2, 9, 1), create(3, 9, 1), value(1, 2)]), Ok(()));
        // pages only, in a pager only
        assert_eq!(check(&[create(1, 10, 0), create(2, 0, 1)]), Err(Error::BadClass));
        assert_eq!(check(&[create(1, 0, 0), create(2, 9, 1)]), Err(Error::BadClass));
        assert!(!t.has(Class::PagerH));
        for r in records(&create(1, 10, 0)) {
            apply(&mut t, &r.unwrap());
        }
        assert!(t.has(Class::PagerH) && !t.has(Class::Pager));
        // the stock's three tiles: 2, 3, 1
        assert_eq!([hpage_dirs(0), hpage_dirs(1), hpage_dirs(2)], [(2, None), (1, Some(2)), (1, Some(3))]);
    }

    #[test]
    fn media_records() {
        assert_eq!(media_key(&[0x16, 1, 0, 0, 2]), Some(MEDIA_PAUSE));
        assert_eq!(media_key(&[0x16, 1, 0, 0, 0]), None);
        assert_eq!(media_key(&[0x16, 1, 0, 0, 5]), Some(MEDIA_VOLUME_UP));
        assert_eq!(media_key(&[0x16, 1, 0, 0, 6]), Some(MEDIA_VOLUME_DOWN));
        assert_eq!(media_key(&[0x16, 1, 0, 0, 7]), None);
        assert_eq!(media_bind(&[0x17, 5, 0, 0, 1, 3, 0, 4, 0]), Some((true, 3, 4)));
        assert_eq!(media_bind(&[0x17, 5, 0, 0, 2, 3, 0, 4, 0]), None);
        // on an older loader both are Malformed
        assert_eq!(check(&[vec![0x16, 1, 0, 0, 1]]), Err(Error::Malformed));
        assert_eq!(check(&[vec![0x17, 5, 0, 0, 1, 3, 0, 4, 0]]), Err(Error::Malformed));
        let mut t = Table::new();
        for r in records(&create(1, 1, 0)) {
            apply(&mut t, &r.unwrap());
        }
        assert!(media_label_ok(&t, 0) && media_label_ok(&t, 1) && !media_label_ok(&t, 2));
        let mut b = [0u8; 0x82];
        b[..4].copy_from_slice(b"Song");
        b[0x41..0x44].copy_from_slice(b"Art");
        assert_eq!(media_info(&b), (&b"Song"[..], &b"Art"[..]));
        b[..0x40].fill(b'x');
        b[0x3F] = 0xD0; // half a two-byte char at the end
        assert_eq!(media_info(&b).0.len(), 0x3F);
    }

    #[test]
    fn music_tile() {
        assert_eq!(check(&[create(1, 11, 0), value(1, 1), value(1, 0)]), Ok(()));
        assert_eq!(check(&[create(1, 11, 0), value(1, 2)]), Err(Error::BadValue));
        assert_eq!(check(&[create(1, 11, 0), create(2, 11, 0)]), Err(Error::BadValue), "one at a time");
        assert_eq!(check(&[create(1, 11, 0), r(2, 1, &[]), create(2, 11, 0)]), Ok(()), "again after DEL");
        assert_eq!(check(&[create(1, 10, 0), create(2, 11, 1)]), Err(Error::BadClass), "a pager holds pages");
        assert_eq!(check(&[create(1, 10, 0), create(2, 9, 1), create(3, 11, 2)]), Ok(()));
        assert_eq!([music_icon(false, true), music_icon(true, true), music_icon(true, false)], [IMG_PLAY_OFF, IMG_PAUSE, IMG_PLAY]);
        let mut c = (0, 0);
        assert!((0..5).all(|_| volume_knob(&mut c, 1, 3, 10).is_none()));
        assert_eq!(volume_knob(&mut c, 1, 3, 10), Some((MEDIA_VOLUME_UP, true)));
        assert_eq!(c, (0, 0));
        (0..5).for_each(|_| assert!(volume_knob(&mut c, -1, 0, 10).is_none()));
        assert_eq!(volume_knob(&mut c, 1, 0, 10), None); // a turn back starts over
        assert_eq!(c, (1, 0));
        (0..4).for_each(|_| assert!(volume_knob(&mut c, 1, 10, 10).is_none()));
        assert_eq!(volume_knob(&mut c, 1, 10, 10), Some((MEDIA_VOLUME_UP, false)), "at the top: sent, no buzz");
        (0..6).for_each(|_| _ = volume_knob(&mut c, -1, 0, 10));
        assert_eq!(c, (0, 0));
    }
}
