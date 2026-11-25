// The stock workout screen's music tile as Class::Music (docs/abi-1.4.md, "Music tile"): built with the calls of
// sport1_view's third tile (layout 0x10079148), with its images and "No music" from its scene and its fonts through
// the stock font open, filled from the phone once a second as its paint (0x101AF03C) does. Taps send the media keys
// here; the app gets them as LV CLICKED events. Volume opens the stock volume popup (0x100786DC) over the app.
// UI thread only.
use crate::{app, media};
use core::ffi::c_void;
use core::ptr::{addr_of_mut, null_mut};
use wp2_core::events::{Event, LV};
use wp2_core::lvcmd::*;
use crate::plat::stock::*;

type Obj = *mut c_void;

const E_IO: i32 = -5;
const CLICKED: u32 = 7;
const HIDDEN: u32 = 1;
const CLICKABLE: u32 = 2;
const SCROLLABLE: u32 = 1 << 4;
// LV_ALIGN_*
const TOP_MID: u32 = 2;
const BOTTOM_MID: u32 = 5;
const OUT_BOTTOM_MID: u32 = 0x0E;
const OUT_LEFT_MID: u32 = 0x11;
const OUT_RIGHT_MID: u32 = 0x14;
const SCROLL_CIRCULAR: u32 = 3;
const TEXT_ALIGN_CENTER: u32 = 2;
const LABEL_W: i32 = 266;
const ARTIST_COLOR: u32 = 0x9492;
const SCROLL_SPEED: u32 = 20;
const POLL_MS: u32 = 1000;
const TOP_LEFT: u32 = 1;
const CENTER: u32 = 9;
const CHECKABLE: u32 = 1 << 3;
const GESTURE_BUBBLE: u32 = 1 << 15;
const GESTURE: u32 = 0x0C;
const KNOB: u32 = 0x2D;
const DIR_RIGHT: u8 = 2;
const PART_INDICATOR: u32 = 0x20000;
const UP: usize = 1;
const DOWN: usize = 2;
const SANS: usize = 0;
const F32M: usize = 1;

struct Music {
    // the tile's handle, 0 = none
    h: u16,
    // the scene struct {id, node, res}
    scene: [u32; 3],
    imgs: [[u32; 3]; MUSIC_IMGS],
    // {char *, u32}
    text: [u32; 2],
    fonts: [[u32; 8]; 2],
    fonts_open: u8,
    title: Obj,
    artist: Obj,
    none: Obj,
    play: Obj,
    playing: bool,
    // (available, playing) applied, None: apply the next one
    last: Option<(bool, bool)>,
    info: [u8; 0x82],
    polled_at: u32,
    // the volume popup: overlay, icon, slider; null = closed
    popup: Obj,
    vol_icon: Obj,
    slider: Obj,
    // its crown counts (up, down); closing: hidden by the swipe, deleted on the next tick
    knob: (u8, u8),
    closing: bool,
}

static mut M: Music = Music {
    h: 0,
    scene: [0; 3],
    imgs: [[0; 3]; MUSIC_IMGS],
    text: [0; 2],
    fonts: [[0; 8]; 2],
    fonts_open: 0,
    title: null_mut(),
    artist: null_mut(),
    none: null_mut(),
    play: null_mut(),
    playing: false,
    last: None,
    info: [0; 0x82],
    polled_at: 0,
    popup: null_mut(),
    vol_icon: null_mut(),
    slider: null_mut(),
    knob: (0, 0),
    closing: false,
};

fn m() -> &'static mut Music {
    unsafe { &mut *addr_of_mut!(M) }
}

fn style(obj: Obj, prop: u32, value: u32) {
    style_at(obj, prop, value, 0)
}

fn style_at(obj: Obj, prop: u32, value: u32, sel: u32) {
    stock!(LV_OBJ_SET_LOCAL_STYLE_PROP, fn(Obj, u32, u32, u32))(obj, prop, value, sel)
}

fn align(obj: Obj, a: u32, x: i32, y: i32) {
    stock!(LV_OBJ_ALIGN, fn(Obj, u32, i32, i32))(obj, a, x, y)
}

fn flag(obj: Obj, f: u32, on: bool) {
    stock!(if on { LV_OBJ_ADD_FLAG } else { LV_OBJ_CLEAR_FLAG }, fn(Obj, u32))(obj, f);
}

fn img(parent: Obj, i: usize) -> Obj {
    let o = stock!(SIMPLE_IMG_CREATE, fn(Obj) -> Obj)(parent);
    stock!(SIMPLE_IMG_SET_SRC, fn(Obj, *const u32))(o, m().imgs[i].as_ptr());
    o
}

fn button(o: Obj, b: i16) {
    flag(o, CLICKABLE, true);
    stock!(LV_OBJ_ADD_EVENT_CB, fn(Obj, extern "C" fn(*mut c_void), u32, usize) -> *mut c_void)(o, on_click, CLICKED, b as usize);
}

// a one-line label of the stock tile: 266 px wide, one line of its font high, scrolling round
fn label(parent: Obj, font: usize) -> Obj {
    let s = m();
    let o = stock!(LV_LABEL_CREATE, fn(Obj) -> Obj)(parent);
    stock!(LV_OBJ_SET_SIZE, fn(Obj, i32, i32))(o, LABEL_W, s.fonts[font][2] as i32);
    style(o, LV_STYLE_TEXT_FONT, s.fonts[font].as_ptr() as u32);
    stock!(LV_LABEL_SET_LONG_MODE, fn(Obj, u32))(o, SCROLL_CIRCULAR);
    o
}

/// CREATE of a Music handle (validated: none lives). The container fills its parent's 466 × 466 like the stock tile.
pub fn create(parent: Obj, h: u16) -> Result<Obj, i32> { #[cfg(feature = "wp3")] return Err(crate::plat::E_NODEV); // sport1 tile: M3
    let s = m();
    let lang = stock!(SCENE_LANG, fn() -> u32)();
    if stock!(SCENE_OPEN, fn(u32, *mut u32, u32, u32, u32) -> i32)(SPORT1_SCENE, s.scene.as_mut_ptr(), SCENE_STY, SCENE_RES, lang) < 0 {
        s.scene = [0; 3];
        return Err(E_IO);
    }
    let ids = SPORT1_IMGS + 4 * MUSIC_IMG_FIRST as u32;
    let imgs = stock!(SCENE_IMGS, fn(*mut u32, u32, *mut u32, u32, u32) -> i32);
    if imgs(s.scene.as_mut_ptr(), ids, s.imgs.as_mut_ptr().cast(), 0, MUSIC_IMGS as u32) < 0 {
        stock!(SCENE_CLOSE, fn(*mut u32))(s.scene.as_mut_ptr());
        return Err(E_IO);
    }
    let texts = stock!(SCENE_TEXTS, fn(*mut u32, u32, *mut u32, u32) -> i32);
    if texts(s.scene.as_mut_ptr(), SPORT1_TEXTS + 4 * TEXT_NO_MUSIC as u32, s.text.as_mut_ptr(), 1) < 0 {
        s.text = [0; 2];
    }
    for (i, path) in [FONT_SANS32, FONT_32M].into_iter().enumerate() {
        if stock!(FONT_OPEN_STOCK, fn(*mut u32, u32) -> i32)(s.fonts[i].as_mut_ptr(), path) == 0 {
            s.fonts_open |= 1 << i;
        } else {
            s.fonts[i] = unsafe { (LV_FONT_DEFAULT as *const [u32; 8]).read() };
        }
    }
    s.h = h;
    let c = stock!(LV_OBJ_CREATE, fn(Obj) -> Obj)(parent);
    stock!(LV_OBJ_SET_SIZE, fn(Obj, i32, i32))(c, 466, 466);
    flag(c, SCROLLABLE | CLICKABLE, false);
    // the stock labels inherit centred text (emulator frame: "No music" at x 164..301)
    style(c, LV_STYLE_TEXT_ALIGN, TEXT_ALIGN_CENTER);
    // the stock order: play, previous, next, volume, title, artist, "No music"
    s.play = img(c, IMG_PLAY_OFF);
    stock!(LV_OBJ_ALIGN, fn(Obj, u32, i32, i32))(s.play, TOP_MID, 0, 173);
    button(s.play, MUSIC_PLAY);
    let align_to = stock!(LV_OBJ_ALIGN_TO, fn(Obj, Obj, u32, i32, i32));
    let prev = img(c, IMG_PREV);
    align_to(prev, s.play, OUT_LEFT_MID, -32, 0);
    button(prev, MUSIC_PREV);
    let next = img(c, IMG_NEXT);
    align_to(next, s.play, OUT_RIGHT_MID, 32, 0);
    button(next, MUSIC_NEXT);
    let vol = img(c, IMG_VOLUME);
    stock!(LV_OBJ_ALIGN, fn(Obj, u32, i32, i32))(vol, BOTTOM_MID, 0, -44);
    button(vol, MUSIC_VOLUME);
    s.title = label(c, SANS);
    stock!(LV_OBJ_ALIGN, fn(Obj, u32, i32, i32))(s.title, TOP_MID, 0, 44);
    style(s.title, LV_STYLE_ANIM_SPEED, SCROLL_SPEED);
    s.artist = label(c, SANS);
    style(s.artist, LV_STYLE_TEXT_COLOR, ARTIST_COLOR);
    align_to(s.artist, s.title, OUT_BOTTOM_MID, 0, 12);
    style(s.artist, LV_STYLE_ANIM_SPEED, SCROLL_SPEED);
    s.none = label(c, F32M);
    if s.text[0] != 0 {
        stock!(LV_LABEL_SET_TEXT, fn(Obj, *const u8))(s.none, s.text[0] as *const u8);
    }
    stock!(LV_OBJ_ALIGN, fn(Obj, u32, i32, i32))(s.none, TOP_MID, 0, 44);
    (s.last, s.info, s.polled_at) = (None, [0; 0x82], app::millis());
    refresh();
    Ok(c)
}

fn set_icon(i: usize) {
    stock!(SIMPLE_IMG_SET_SRC, fn(Obj, *const u32))(m().play, m().imgs[i].as_ptr());
}

/// VALUE of the Music handle: the play button shows pause (1) or play (0) until the phone's state changes.
pub fn set_playing(on: bool) {
    m().playing = on;
    set_icon(music_icon(true, on));
}

fn set_text(obj: Obj, text: &[u8]) {
    let mut z = [0u8; 0x41];
    z[..text.len()].copy_from_slice(text);
    stock!(LV_LABEL_SET_TEXT, fn(Obj, *const u8))(obj, z.as_ptr());
}

// the stock paint: title and artist with phone media, else "No music"; the play button by the state
fn refresh() {
    let s = m();
    let state = (media::available(), media::playing());
    if s.last != Some(state) {
        s.last = Some(state);
        s.playing = state.1;
        set_icon(music_icon(state.0, state.1));
        for o in [s.title, s.artist] {
            flag(o, HIDDEN, !state.0);
        }
        flag(s.none, HIDDEN, state.0);
    }
    if !state.0 {
        return;
    }
    refresh_popup();
    let info = media::info();
    if info == &s.info {
        return;
    }
    s.info.copy_from_slice(info);
    let (title, artist) = media_info(&s.info);
    set_text(s.title, title);
    set_text(s.artist, artist);
}

/// UI tick.
pub fn tick() {
    let s = m();
    if s.closing {
        close();
    }
    if s.h == 0 || !app::running() || app::paused() || app::millis().wrapping_sub(s.polled_at) < POLL_MS {
        return;
    }
    s.polled_at = app::millis();
    refresh();
}

// user_data = the button
extern "C" fn on_click(e: *mut c_void) {
    let b = stock!(LV_EVENT_GET_USER_DATA, fn(*mut c_void) -> usize)(e) as i16;
    let s = m();
    if s.h == 0 || !app::running() || app::paused() {
        return;
    }
    // as the stock tile: the keys only with phone media (its "connect the phone" popup is left out)
    let cmd = match b {
        MUSIC_PLAY if s.playing => MEDIA_PAUSE,
        MUSIC_PLAY => MEDIA_PLAY,
        MUSIC_PREV => MEDIA_PREV,
        MUSIC_NEXT => MEDIA_NEXT,
        _ => 0,
    };
    if b == MUSIC_VOLUME && media::available() {
        open_popup();
    }
    if cmd != 0 && media::available() {
        media::send(cmd);
        if b == MUSIC_PLAY {
            set_playing(!s.playing);
        }
    }
    app::QUEUE.push(Event { kind: LV, x: s.h as i16, y: CLICKED as i16, delta: b, ..Event::default() });
}

// ux2sys 0x50 as the stock reads it (0x10078230): (max, current), current 0 without a max
fn volume() -> (u8, u8) {
    let [max, cur, ..] = media::get(PROP_VOLUME)[0].to_le_bytes();
    (max, if max == 0 { 0 } else { cur })
}

// the stock popup: a black 466 × 466 overlay with the volume icon, a 12 × 148 image slider and two invisible 120 × 96
// buttons above and below it (+ and −); the crown steps the volume, a right swipe closes it. On the app's root, not
// on the tile: a pager page would scroll under it.
fn open_popup() {
    let s = m();
    let root = crate::lv::root();
    if !s.popup.is_null() || root.is_null() {
        return;
    }
    let (max, cur) = volume();
    let p = stock!(LV_OBJ_CREATE, fn(Obj) -> Obj)(root);
    stock!(LV_OBJ_SET_SIZE, fn(Obj, i32, i32))(p, 466, 466);
    style(p, LV_STYLE_BG_COLOR, 0);
    style(p, LV_STYLE_BG_OPA, 0xFF);
    align(p, CENTER, 0, 0);
    flag(p, GESTURE_BUBBLE, false);
    let icon = stock!(LV_IMG_CREATE, fn(Obj) -> Obj)(p);
    align(icon, TOP_LEFT, 173, 45);
    let slider = stock!(LV_SLIDER_CREATE, fn(Obj) -> Obj)(p);
    stock!(LV_OBJ_SET_SIZE, fn(Obj, i32, i32))(slider, 12, 148);
    style_at(slider, LV_STYLE_BG_IMG_SRC, s.imgs[IMG_TRACK].as_ptr() as u32, 0);
    style_at(slider, LV_STYLE_BG_IMG_SRC, s.imgs[IMG_FILL].as_ptr() as u32, PART_INDICATOR);
    align(slider, CENTER, 0, 0);
    stock!(LV_BAR_SET_RANGE, fn(Obj, i32, i32))(slider, 0, max.into());
    flag(slider, CLICKABLE, false);
    for (ud, a, y) in [(UP, TOP_MID, 45), (DOWN, BOTTOM_MID, -45)] {
        let b = stock!(LV_BTN_CREATE, fn(Obj) -> Obj)(p);
        stock!(LV_OBJ_SET_SIZE, fn(Obj, i32, i32))(b, 120, 96);
        flag(b, CHECKABLE, true);
        stock!(LV_OBJ_ADD_EVENT_CB, fn(Obj, extern "C" fn(*mut c_void), u32, usize) -> *mut c_void)(b, on_volume, CLICKED, ud);
        align(b, a, 0, y);
    }
    // the KNOB filter makes it the knob object; its delete gives the crown back
    stock!(LV_OBJ_ADD_EVENT_CB, fn(Obj, extern "C" fn(*mut c_void), u32, usize) -> *mut c_void)(p, on_volume, KNOB, 0);
    stock!(LV_OBJ_ADD_EVENT_CB, fn(Obj, extern "C" fn(*mut c_void), u32, usize) -> *mut c_void)(p, on_gesture, GESTURE, 0);
    flag(p, CLICKABLE, true);
    flag(p, GESTURE_BUBBLE | SCROLLABLE, false);
    (s.popup, s.vol_icon, s.slider, s.knob, s.closing) = (p, icon, slider, (0, 0), false);
    show_volume(cur);
}

fn show_volume(cur: u8) {
    let s = m();
    stock!(LV_IMG_SET_SRC, fn(Obj, *const u32))(s.vol_icon, s.imgs[if cur == 0 { IMG_MUTED } else { IMG_VOLUME_ON }].as_ptr());
    stock!(LV_BAR_SET_VALUE, fn(Obj, i32, u32))(s.slider, cur.into(), 0);
}

// the stock paint: an open popup follows the phone's volume
fn refresh_popup() {
    if !m().popup.is_null() && !m().closing {
        show_volume(volume().1);
    }
}

/// The volume popup is open: the right swipe closes it (main.rs key), not the app.
pub fn popup() -> bool {
    !m().popup.is_null()
}

// buttons: user_data 1 up, 2 down; the overlay: the crown (0x10078FE4)
extern "C" fn on_volume(e: *mut c_void) {
    let s = m();
    if s.popup.is_null() || s.closing {
        return;
    }
    let cmd = match stock!(LV_EVENT_GET_CODE, fn(*mut c_void) -> u32)(e) {
        CLICKED => match stock!(LV_EVENT_GET_USER_DATA, fn(*mut c_void) -> usize)(e) {
            UP => MEDIA_VOLUME_UP,
            _ => MEDIA_VOLUME_DOWN,
        },
        KNOB => {
            let step = crate::ui::knob_step(e);
            let (max, cur) = volume();
            let Some((cmd, buzz)) = volume_knob(&mut s.knob, step, cur, max) else { return };
            if buzz {
                stock!(UX2SYS_SET_LEN, fn(u32, *const u32, u32) -> i32)(PROP_VIBRATE, &8, 4);
            }
            cmd
        }
        _ => return,
    };
    media::send(cmd);
}

// 0x101AEFF8: a right swipe hides it; deleted from the tick, not inside its own event
extern "C" fn on_gesture(e: *mut c_void) {
    let indev = stock!(LV_EVENT_GET_PARAM, fn(*mut c_void) -> Obj)(e);
    if stock!(LV_INDEV_GET_GESTURE_DIR, fn(Obj) -> u8)(indev) != DIR_RIGHT || m().popup.is_null() {
        return;
    }
    flag(m().popup, HIDDEN, true);
    stock!(LV_INDEV_WAIT_RELEASE, fn(Obj))(indev);
    m().closing = true;
}

/// Deletes the volume popup (before its root goes, or once the swipe hid it).
pub fn close() {
    let s = m();
    if !s.popup.is_null() {
        stock!(LV_OBJ_DEL, fn(Obj))(s.popup);
    }
    (s.popup, s.vol_icon, s.slider, s.closing) = (null_mut(), null_mut(), null_mut(), false);
}

/// After its objects are gone (DEL of the tile or of an ancestor, teardown): frees its images, text, fonts, scene.
pub fn free() {
    let s = m();
    if s.h == 0 {
        return;
    }
    close();
    for i in 0..2 {
        if s.fonts_open >> i & 1 != 0 {
            stock!(LVGL_BITMAP_FONT_CLOSE, fn(*mut u32))(s.fonts[i].as_mut_ptr());
        }
    }
    if s.text[0] != 0 {
        stock!(SCENE_TEXTS_FREE, fn(*mut u32, u32))(s.text.as_mut_ptr(), 1);
    }
    stock!(SCENE_IMGS_FREE, fn(*mut u32, u32))(s.imgs.as_mut_ptr().cast(), MUSIC_IMGS as u32);
    stock!(SCENE_CLOSE, fn(*mut u32))(s.scene.as_mut_ptr());
    (s.h, s.fonts_open, s.text, s.last) = (0, 0, [0; 2], None);
    (s.title, s.artist, s.none, s.play) = (null_mut(), null_mut(), null_mut(), null_mut());
}

/// Handle of the live tile, 0 = none.
pub fn handle() -> u16 {
    m().h
}
