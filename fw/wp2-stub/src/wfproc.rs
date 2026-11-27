// Procedural face widget types (wfc tag 0x82 + 0x5C, docs/recon.md "Watch 3 Pro faces"): the stub's table replaces
// the stock's at 0x101F3D6C through the five literals `wp2apps firmware` patches. The stock loops stop at 0xFF and
// its table ends with 0xFFFF, so a Watch 3 Pro type (4..7) ran past it into whatever followed. A type that is not
// listed here is skipped: the widget stays an empty object.
// Types 4..7 are the stub's own: wp2-core wfproc.rs says what to draw. 6 and 7 are scenes (`draw`), 4 lights a ring
// up under the finger (`touched`), 5 plays a clip and a tap starts the next one (`tapped`, `frame_due`).
use core::ffi::c_void;
use core::mem::{size_of, transmute};
use wp2_core::stock;
use wp2_core::wfproc::{self as panel, Clock, Scene};

#[repr(C)]
pub struct Type {
    kind: u32,
    // all four get the widget (its object at 0, type at +4, state at +0x24); the stock calls them unchecked
    create: *const c_void,
    destroy: *const c_void,
    update: *const c_void,
    tick: *const c_void,
    /// the only one the stock checks for null
    opt: *const c_void,
}

unsafe impl Sync for Type {}

// also the 0xFF entry's handlers: a face whose type is 0xFF matches it instead of stopping there
extern "C" fn skip(_widget: *mut c_void) -> i32 {
    0
}

const fn stock(kind: u32, f: [usize; 5]) -> Type {
    Type {
        kind,
        create: f[0] as *const c_void,
        destroy: f[1] as *const c_void,
        update: f[2] as *const c_void,
        tick: f[3] as *const c_void,
        opt: f[4] as *const c_void,
    }
}

const fn own(kind: u32) -> Type {
    Type {
        kind,
        create: create as *const c_void,
        destroy: destroy as *const c_void,
        update: update as *const c_void,
        tick: skip as *const c_void,
        opt: core::ptr::null(),
    }
}

const END: Type = Type {
    kind: 0xFF,
    create: skip as *const c_void,
    destroy: skip as *const c_void,
    update: skip as *const c_void,
    tick: skip as *const c_void,
    opt: core::ptr::null(),
};

#[used]
pub static TYPES: [Type; 9] = [
    stock(0, [0x100D_EEFD, 0x101B_9599, 0x101B_956D, 0x101B_9583, 0]),
    stock(1, [0x100D_F0D5, 0x101B_95E1, 0x101B_9607, 0x101B_975F, 0]),
    stock(2, [0x100D_F49D, 0x101B_9775, 0x100D_F5FD, 0x101B_97AB, 0x101B_97C1]),
    stock(3, [0x100D_F775, 0x101B_984F, 0x101B_9885, 0x101B_989B, 0]),
    own(LOLLIPOP as u32),
    own(BRIQBOT as u32),
    own(DATAPULSE as u32),
    own(BEAM as u32),
    END,
];

const LOLLIPOP: u16 = 4;
const BRIQBOT: u16 = 5;
const DATAPULSE: u16 = 6;
// (list, language group, index, *out) -> 0 or < 0: an entry of a face's image list
const IMAGE_LIST: u32 = 0x101B_7811;
const BEAM: u16 = 7;

// the ui_mem the stock's own types keep their state in (kind 2), and the stock's helpers for a face's images:
// load(cache, list, first, count, dsc) keeps what it loaded in the widget's cache, free(cache) drops all of it
const MEM: u32 = 2;
const UI_MEM_ALLOC: u32 = 0x1018_8235; // (kind, size, caller), as the stock's types use it
const IMAGES_LOAD: u32 = 0x101B_94A3;
const IMAGES_FREE: u32 = 0x100D_BB01;
// (cache, count, dsc): drops those images only and clears their dsc
const IMAGES_RELEASE: u32 = 0x101B_9533;
// (img, tenths of a degree): clockwise around the pivot, which set_src puts at the centre
const SIMPLE_IMG_SET_ANGLE: u32 = 0x1005_A1F1;
// () -> the temperature unit is Fahrenheit (ux2sys 0x7D)
const FAHRENHEIT: u32 = 0x101A_FC29;
const PROP_12H: u32 = 0x7C;
const PROP_WEATHER: u32 = 0x14;
const PROP_HEART: u32 = 0x0D;
const PROP_SPO2: u32 = 0x17;
const PROP_STRESS: u32 = 0x80;
const PROP_SLEEP: u32 = 0x16;
// the daily totals (0x1F4 B, kcal +0x1A4) and goals (kcal +8), as the stock activity view 0x10081AA4 reads them
const PROP_TOTALS: u32 = 0x15;
const PROP_GOALS: u32 = 0x33;
const TOTALS_LEN: usize = 0x1F4;
const EV_PRESSED: u32 = 1;
const EV_PRESS_LOST: u32 = 3;
const EV_CLICKED: u32 = 7;
const EV_RELEASED: u32 = 8;
const HIDDEN: u32 = 1;
const STYLE_BORDER_COLOR: u32 = 0x30;
const STYLE_BORDER_OPA: u32 = 0x31;
const STYLE_BORDER_WIDTH: u32 = 0x32;
const CIRCLE: u32 = 0x7FFF;
// lv_timer_t
const TIMER_USER_DATA: usize = 0xC;
const OPAQUE: u32 = 0xFF;
const EVENT_BUBBLE: u32 = 0x4000;
const CLICKABLE: u32 = 2;
const SCROLLABLE: u32 = 0x10;
const FACE: i32 = 466;

type Obj = *mut u8;

macro_rules! stock {
    ($addr:expr, fn($($t:ty),*) -> $r:ty) => {
        unsafe { transmute::<usize, extern "C" fn($($t),*) -> $r>($addr as usize) }
    };
}

/// WFManager's 0x30-byte record of a kind 6 item.
#[repr(C)]
struct Widget {
    obj: Obj,
    kind: u16,
    _06: [u8; 0x12],
    /// 0x5C payload +0x14: the panel
    sub: u8,
    _19: [u8; 0x0B],
    state: *mut State,
    /// the image list of the widget's tag 1
    list: *const u8,
    /// head of the loaded images
    cache: u32,
}
const _: () = assert!(size_of::<*mut u8>() != 4 || size_of::<Widget>() == 0x30);

struct State {
    root: Obj,
    /// type 4: the band; type 5: the picture
    part: Obj,
    timer: *mut u8,
    frame: u8,
    clip: u8,
    digest: u32,
    loaded: [bool; panel::IMAGES],
    images: [[u32; 3]; panel::IMAGES],
    scene: Scene,
}

fn prop(id: u32, out: *mut u8) {
    stock!(stock::UX2SYS_GET, fn(u32, *mut u8) -> u32)(id, out);
}

fn value(id: u32) -> u32 {
    let mut v = [0u32; 2];
    prop(id, v.as_mut_ptr().cast());
    v[0]
}

/// The history a pointer property points at.
fn history<const N: usize>(id: u32) -> Option<&'static [u8; N]> {
    unsafe { (value(id) as usize as *const [u8; N]).as_ref() }
}

fn fill(scene: &mut Scene, sub: u8) {
    scene.len = 0;
    let mut time = [0u32; 3];
    stock!(stock::RTC_GET_TIME, fn(*mut u32) -> ())(time.as_mut_ptr());
    let clock = Clock { hour: (time[0] >> 16) as u8, twelve: value(PROP_12H) == 1 };
    match sub {
        0 => {
            let mut data = [0u8; panel::WEATHER_LEN];
            prop(PROP_WEATHER, data.as_mut_ptr());
            let fahrenheit = stock!(FAHRENHEIT, fn() -> u32)() & 0xFF != 0;
            panel::weather(scene, &data, clock, fahrenheit, value(stock::PROP_PHONE_LINK) != 0);
        }
        1 => {
            if let Some(data) = history(PROP_HEART) {
                panel::heart(scene, data, clock);
            }
        }
        2 => {
            if let Some(data) = history(PROP_SPO2) {
                panel::spo2(scene, data, clock);
            }
        }
        3 => {
            if let Some(data) = history(PROP_STRESS) {
                panel::stress(scene, data, clock);
            }
        }
        4 => {
            if let Some(data) = history(PROP_SLEEP) {
                panel::sleep(scene, data, stock!(stock::TIME_ZONE, fn() -> i32)());
            }
        }
        _ => {}
    }
}

/// The scene's image, loaded on first use; None when the face's list has none.
fn image(w: &mut Widget, s: &mut State, img: u8) -> Option<*const u32> {
    let i = usize::from(img);
    if i >= panel::IMAGES || w.list.is_null() || img >= unsafe { *w.list.add(1) } {
        return None;
    }
    let dsc = s.images[i].as_mut_ptr();
    if !s.loaded[i] {
        let load = stock!(IMAGES_LOAD, fn(*mut u32, *const u8, u32, u32, *mut u32) -> i32);
        if load(&mut w.cache, w.list, i as u32, 1, dsc) != 0 {
            return None;
        }
        s.loaded[i] = true;
    }
    Some(dsc)
}

fn fill_calories(scene: &mut Scene) {
    scene.len = 0;
    // both getters take the stock's 500-byte buffer
    let mut data = [0u32; TOTALS_LEN / 4];
    prop(PROP_GOALS, data.as_mut_ptr().cast());
    let goal = data[2];
    prop(PROP_TOTALS, data.as_mut_ptr().cast());
    panel::calories(scene, goal, data[0x1A4 / 4]);
}

fn draw(w: &mut Widget, s: &mut State) {
    match w.kind {
        DATAPULSE => fill(&mut s.scene, w.sub),
        BEAM => fill_calories(&mut s.scene),
        _ => return,
    }
    let digest = s.scene.digest();
    if digest == s.digest {
        return;
    }
    s.digest = digest;
    let count = stock!(stock::LV_OBJ_GET_CHILD_CNT, fn(Obj) -> u32);
    let child = stock!(stock::LV_OBJ_GET_CHILD, fn(Obj, i32) -> Obj);
    let del = stock!(stock::LV_OBJ_DEL, fn(Obj) -> ());
    for _ in 0..count(s.root) {
        del(child(s.root, -1));
    }
    for i in 0..s.scene.len.min(panel::ITEMS) {
        let item = s.scene.items[i];
        let obj = if item.img == panel::RECT {
            let Some(obj) = plain(s.root, item.w.into(), item.h.into()) else { return };
            let style = stock!(stock::LV_OBJ_SET_LOCAL_STYLE_PROP, fn(Obj, u32, u32, u32) -> ());
            style(obj, stock::LV_STYLE_BG_COLOR, item.color.into(), 0);
            style(obj, stock::LV_STYLE_BG_OPA, OPAQUE, 0);
            style(obj, stock::LV_STYLE_RADIUS, item.radius.into(), 0);
            obj
        } else {
            let Some(dsc) = image(w, s, item.img) else { continue };
            let obj = stock!(stock::SIMPLE_IMG_CREATE, fn(Obj) -> Obj)(s.root);
            if obj.is_null() {
                return;
            }
            stock!(stock::SIMPLE_IMG_SET_SRC, fn(Obj, *const u32) -> ())(obj, dsc);
            if item.angle != 0 {
                stock!(SIMPLE_IMG_SET_ANGLE, fn(Obj, i32) -> ())(obj, item.angle.into());
            }
            stock!(stock::LV_OBJ_ADD_FLAG, fn(Obj, u32) -> ())(obj, EVENT_BUBBLE);
            obj
        };
        stock!(stock::LV_OBJ_SET_POS, fn(Obj, i32, i32) -> ())(obj, item.x.into(), item.y.into());
    }
}

/// An object that only draws: taps and swipes stay with the face.
fn plain(parent: Obj, w: i32, h: i32) -> Option<Obj> {
    let obj = stock!(stock::LV_OBJ_CREATE, fn(Obj) -> Obj)(parent);
    if obj.is_null() {
        return None;
    }
    stock!(stock::LV_OBJ_SET_SIZE, fn(Obj, i32, i32) -> ())(obj, w, h);
    stock!(stock::LV_OBJ_CLEAR_FLAG, fn(Obj, u32) -> ())(obj, CLICKABLE | SCROLLABLE);
    stock!(stock::LV_OBJ_ADD_FLAG, fn(Obj, u32) -> ())(obj, EVENT_BUBBLE);
    Some(obj)
}

fn on_event(obj: Obj, cb: extern "C" fn(*mut c_void), code: u32, widget: *mut c_void) {
    let add = stock!(stock::LV_OBJ_ADD_EVENT_CB, fn(Obj, extern "C" fn(*mut c_void), u32, *mut c_void) -> ());
    add(obj, cb, code, widget);
}

fn event_widget<'a>(event: *mut c_void) -> Option<(&'a mut Widget, &'a mut State)> {
    let widget = stock!(stock::LV_EVENT_GET_USER_DATA, fn(*mut c_void) -> *mut Widget)(event);
    let w = unsafe { widget.as_mut() }?;
    let s = unsafe { w.state.as_mut() }?;
    Some((w, s))
}

extern "C" fn touched(event: *mut c_void) {
    let Some((_, s)) = event_widget(event) else { return };
    let mut hit = None;
    if stock!(stock::LV_EVENT_GET_CODE, fn(*mut c_void) -> u32)(event) == EV_PRESSED {
        // the event's param is the indev
        let indev = stock!(stock::LV_EVENT_GET_PARAM, fn(*mut c_void) -> *mut c_void)(event);
        if !indev.is_null() {
            let mut at = [0i32; 2];
            stock!(stock::LV_INDEV_GET_POINT, fn(*mut c_void, *mut i32) -> ())(indev, at.as_mut_ptr());
            hit = panel::ring(at[0], at[1]);
        }
    }
    let Some(ring) = hit else {
        stock!(stock::LV_OBJ_ADD_FLAG, fn(Obj, u32) -> ())(s.part, HIDDEN);
        return;
    };
    let (xy, size, color) = panel::ring_band(ring);
    stock!(stock::LV_OBJ_SET_POS, fn(Obj, i32, i32) -> ())(s.part, xy, xy);
    stock!(stock::LV_OBJ_SET_SIZE, fn(Obj, i32, i32) -> ())(s.part, size, size);
    stock!(stock::LV_OBJ_SET_LOCAL_STYLE_PROP, fn(Obj, u32, u32, u32) -> ())(s.part, STYLE_BORDER_COLOR, color.into(), 0);
    stock!(stock::LV_OBJ_CLEAR_FLAG, fn(Obj, u32) -> ())(s.part, HIDDEN);
}

extern "C" fn tapped(event: *mut c_void) {
    if let Some((_, s)) = event_widget(event) {
        s.clip = panel::next_clip(s.clip);
    }
}

extern "C" fn frame_due(timer: *mut c_void) {
    let widget = unsafe { *timer.cast::<u8>().add(TIMER_USER_DATA).cast::<*mut Widget>() };
    let Some(w) = (unsafe { widget.as_mut() }) else { return };
    let Some(s) = (unsafe { w.state.as_mut() }) else { return };
    let next = panel::next_frame(s.frame, s.clip);
    if next != s.frame {
        s.frame = next;
        show_frame(w, s);
    }
}

/// Type 5 keeps one decoded frame: they are full-screen and there are 159.
fn show_frame(w: &mut Widget, s: &mut State) {
    if w.list.is_null() || s.frame >= unsafe { *w.list.add(1) } {
        return;
    }
    let dsc = s.images[0].as_mut_ptr();
    if s.loaded[0] {
        // released first: a second frame may not fit the full-screen heap
        stock!(IMAGES_RELEASE, fn(*mut u32, i32, *mut u32) -> i32)(&mut w.cache, 1, dsc);
        s.loaded[0] = false;
    }
    let load = stock!(IMAGES_LOAD, fn(*mut u32, *const u8, u32, u32, *mut u32) -> i32);
    s.loaded[0] = load(&mut w.cache, w.list, s.frame.into(), 1, dsc) == 0;
    let src = if s.loaded[0] { dsc as *const u32 } else { core::ptr::null() };
    stock!(stock::SIMPLE_IMG_SET_SRC, fn(Obj, *const u32) -> ())(s.part, src);
}

/// What type 4 and 5 have besides the root.
fn start(w: &mut Widget, s: &mut State, widget: *mut c_void) {
    match w.kind {
        LOLLIPOP => {
            let Some(band) = plain(s.root, 0, 0) else { return };
            stock!(stock::LV_OBJ_ADD_FLAG, fn(Obj, u32) -> ())(band, HIDDEN);
            let style = stock!(stock::LV_OBJ_SET_LOCAL_STYLE_PROP, fn(Obj, u32, u32, u32) -> ());
            style(band, stock::LV_STYLE_RADIUS, CIRCLE, 0);
            style(band, STYLE_BORDER_WIDTH, panel::RING_W as u32, 0);
            style(band, STYLE_BORDER_OPA, OPAQUE, 0);
            s.part = band;
            for code in [EV_PRESSED, EV_PRESS_LOST, EV_RELEASED] {
                on_event(w.obj, touched, code, widget);
            }
        }
        BRIQBOT => {
            let img = stock!(stock::SIMPLE_IMG_CREATE, fn(Obj) -> Obj)(s.root);
            if img.is_null() {
                return;
            }
            stock!(stock::LV_OBJ_ADD_FLAG, fn(Obj, u32) -> ())(img, EVENT_BUBBLE);
            stock!(stock::LV_OBJ_SET_POS, fn(Obj, i32, i32) -> ())(img, 0, 0);
            s.part = img;
            show_frame(w, s);
            on_event(w.obj, tapped, EV_CLICKED, widget);
            let timer = stock!(stock::LV_TIMER_CREATE, fn(extern "C" fn(*mut c_void), u32, *mut c_void) -> *mut u8);
            s.timer = timer(frame_due, panel::FRAME_MS, widget);
        }
        _ => {}
    }
}

// Replaces `bl 0x101B7810` at 0x100DB378: the parser of 0x82 looks the widget's first image up and drops a widget
// without one (-19), but type 4 has none. The parser keeps the type at its sp + 0x48; the group it passes is always 0,
// so the type travels in its register.
#[unsafe(naked)]
pub unsafe extern "C" fn wp2_proc_list() {
    core::arch::naked_asm!("ldrh.w r1, [sp, #0x48]", "b.w {lookup}", lookup = sym first_image)
}

extern "C" fn first_image(list: *const u8, kind: u32, index: u32, out: *mut u32) -> i32 {
    if kind == u32::from(LOLLIPOP) {
        unsafe { *out = 0 };
        return 0;
    }
    stock!(IMAGE_LIST, fn(*const u8, u32, u32, *mut u32) -> i32)(list, 0, index, out)
}

extern "C" fn create(widget: *mut c_void) -> i32 {
    let Some(w) = (unsafe { widget.cast::<Widget>().as_mut() }) else { return -2 };
    if !w.state.is_null() {
        return -0x11;
    }
    let len = size_of::<State>() as u32;
    let s = stock!(UI_MEM_ALLOC, fn(u32, u32, *const c_void) -> *mut State)(MEM, len, widget);
    let Some(state) = (unsafe { s.as_mut() }) else { return -2 };
    unsafe { core::ptr::write_bytes(s, 0, 1) };
    // WFManager calls create again while its object has no child, so the root comes even with nothing to show
    let Some(root) = plain(w.obj, FACE, FACE) else {
        stock!(stock::UI_MEM_FREE, fn(u32, *mut State) -> ())(MEM, s);
        return -2;
    };
    stock!(stock::LV_OBJ_SET_POS, fn(Obj, i32, i32) -> ())(root, 0, 0);
    state.root = root;
    w.state = s;
    start(w, state, widget);
    draw(w, state);
    0
}

/// WFManager deletes the objects right after it.
extern "C" fn destroy(widget: *mut c_void) -> i32 {
    let Some(w) = (unsafe { widget.cast::<Widget>().as_mut() }) else { return -2 };
    if w.state.is_null() {
        return -0x11;
    }
    let timer = unsafe { (*w.state).timer };
    if !timer.is_null() {
        stock!(stock::LV_TIMER_DEL, fn(*mut u8) -> ())(timer);
    }
    stock!(IMAGES_FREE, fn(*mut u32) -> i32)(&mut w.cache);
    stock!(stock::UI_MEM_FREE, fn(u32, *mut State) -> ())(MEM, w.state);
    w.state = core::ptr::null_mut();
    0
}

extern "C" fn update(widget: *mut c_void) -> i32 {
    let Some(w) = (unsafe { widget.cast::<Widget>().as_mut() }) else { return -2 };
    let Some(state) = (unsafe { w.state.as_mut() }) else { return -0x11 };
    draw(w, state);
    0
}
