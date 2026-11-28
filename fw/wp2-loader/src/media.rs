// Phone music for apps (docs/abi-1.4.md, "Music"), as the stock music view and workout screen do it: MEDIA_KEY sends
// a play/pause/previous/next/volume command (ux2sys 0x4E) when the phone link and its media are up; MEDIA_BIND has the UI
// tick write the track title and artist into two of the app's labels once a second and queue a MEDIA event when
// playing/available change. The stock calls run on the UI thread.
use crate::app::{millis, QUEUE};
use crate::lv;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU8, Ordering};
use wp2_core::events::{Event, MEDIA};
use wp2_core::lvcmd;
use wp2_core::stock::*;

const E_NODEV: i32 = -19;
const POLL_MS: u32 = 1000;
const WAITING: i32 = i32::MIN;

// MEDIA_KEY mailbox: the app thread waits for the UI tick (it waits in lv_submit the same way)
static CMD: AtomicU8 = AtomicU8::new(0);
static RESULT: AtomicI32 = AtomicI32::new(0);
static BOUND: AtomicBool = AtomicBool::new(false);
// title << 16 | artist
static LABELS: AtomicU32 = AtomicU32::new(0);

struct State {
    polled_at: u32,
    // (playing, available) last sent, None: send the first one
    last: Option<(bool, bool)>,
    info: [u8; 0x82],
}
static mut STATE: State = State { polled_at: 0, last: None, info: [0; 0x82] };
// the getter's out buffer: the stock reads into 504 B for every property. From the RES heap at the first read while
// an app uses music, back in `stop` (the app ended), so the loader's 64 KiB block doesn't carry it.
static mut OUT: *mut [u32; 128] = core::ptr::null_mut();
// what a read answers when the heap had no room: nothing playing, no link
static NONE: [u32; 1] = [0];

/// The property's words; when the heap has no room, only word 0 (0) is meaningful.
pub fn get(prop: u32) -> &'static [u32] {
    unsafe {
        if OUT.is_null() {
            OUT = wp2_core::heap::alloc(wp2_core::heap::RES, 512, 4).cast();
        }
        let Some(out) = OUT.as_mut() else { return &NONE };
        out[0] = 0;
        stock!(UX2SYS_GET, fn(u32, *mut u32))(prop, out.as_mut_ptr());
        out
    }
}

/// UI thread: the phone link and its media are up.
pub fn available() -> bool {
    get(PROP_PHONE_LINK)[0] != 0 && get(PROP_MEDIA_AVAILABLE)[0] != 0
}

/// UI thread: the phone's player plays.
pub fn playing() -> bool {
    get(PROP_MEDIA_PLAYING)[0] != 0
}

/// UI thread: ux2sys 0x4F (title, artist; `lvcmd::media_info`).
pub fn info() -> &'static [u8; 0x82] {
    let w = get(PROP_MEDIA_INFO);
    if w.len() * 4 < 0x82 {
        return unsafe { &*addr_of_mut!(STATE.info) };
    }
    unsafe { &*(w.as_ptr() as *const [u8; 0x82]) }
}

/// UI thread: sends a media key (the caller checked `available`).
pub fn send(cmd: u8) {
    stock!(UX2SYS_SET, fn(u32, *const u32) -> i32)(PROP_MEDIA_CMD, &u32::from(cmd));
}

// app thread (lv_submit): 0, or -19 without a phone link or phone media
pub fn key(cmd: u8) -> i32 {
    RESULT.store(WAITING, Ordering::Relaxed);
    CMD.store(cmd, Ordering::Release);
    loop {
        let r = RESULT.load(Ordering::Acquire);
        if r != WAITING {
            return r;
        }
        stock!(OS_SLEEP, fn(u32))(5);
    }
}

// app thread (lv_submit), labels checked
pub fn bind(on: bool, title: u16, artist: u16) {
    LABELS.store(u32::from(title) << 16 | u32::from(artist), Ordering::Relaxed);
    if on {
        unsafe { (*addr_of_mut!(STATE)).last = None };
    }
    BOUND.store(on, Ordering::Release);
}

fn set_text(h: u16, text: &[u8]) {
    let obj = lv::label(h);
    if obj.is_null() {
        return; // deleted since the bind
    }
    let mut z = [0u8; 0x41];
    z[..text.len()].copy_from_slice(text);
    stock!(LV_LABEL_SET_TEXT, fn(*mut core::ffi::c_void, *const u8))(obj, z.as_ptr());
}

// UI tick
pub fn tick(running: bool, paused: bool) {
    let cmd = CMD.swap(0, Ordering::Acquire);
    if cmd != 0 {
        let r = if available() {
            send(cmd);
            0
        } else {
            E_NODEV
        };
        RESULT.store(r, Ordering::Release);
    }
    let s = unsafe { &mut *addr_of_mut!(STATE) };
    let now = millis();
    if !running || paused || !BOUND.load(Ordering::Acquire) || (s.last.is_some() && now.wrapping_sub(s.polled_at) < POLL_MS) {
        return;
    }
    s.polled_at = now;
    let state = (playing(), available());
    if s.last != Some(state) {
        s.last = Some(state);
        QUEUE.coalesce(Event { kind: MEDIA, x: state.0.into(), y: state.1.into(), ..Event::default() });
    }
    let bytes = info();
    if bytes == &s.info {
        return;
    }
    s.info.copy_from_slice(bytes);
    let (title, artist) = lvcmd::media_info(&s.info);
    let labels = LABELS.load(Ordering::Relaxed);
    set_text((labels >> 16) as u16, title);
    set_text(labels as u16, artist);
}

// UI thread, app thread aborted
/// UI thread, the app gone (lv::teardown, after the music tile's objects): the getter's buffer back to the heap.
pub fn release() {
    unsafe {
        let p = core::mem::replace(&mut *addr_of_mut!(OUT), core::ptr::null_mut());
        wp2_core::heap::free(wp2_core::heap::RES, p.cast());
    }
}

pub fn stop() {
    BOUND.store(false, Ordering::Release);
    CMD.store(0, Ordering::Relaxed);
    unsafe { (*addr_of_mut!(STATE)).info = [0; 0x82] };
}
