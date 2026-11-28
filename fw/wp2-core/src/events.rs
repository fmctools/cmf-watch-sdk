use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicU32, AtomicU8, Ordering},
};

use crate::stock::{KEY_BACK, KEY_CROWN_CCW, KEY_CROWN_CW, KEY_CROWN_PRESS};

pub const TOUCH_DOWN: u8 = 1;
pub const TOUCH_UP: u8 = 2;
pub const CROWN: u8 = 3;
pub const BUTTON: u8 = 4;
pub const PAUSE: u8 = 5;
pub const RESUME: u8 = 6;
// ABI 1.1: a host message is in the inbox, x = its length
pub const MESSAGE: u8 = 7;
// ABI 1.2: an LVGL object event, x = handle, y = lv event code, delta = value (VALUE_CHANGED)
pub const LV: u8 = 8;
// loader of 2026-11-14, no ABI bump (docs/abi-1.4.md): x = 1 on the wrist, delta = bpm (0 = no reading)
pub const HEART: u8 = 9;
// loader of 2026-11-16, no ABI bump (docs/abi-1.4.md, "Music"): x = 1 playing, y = 1 phone media available
pub const MEDIA: u8 = 10;
// loader of 2026-11-19, no ABI bump (docs/abi-1.4.md, "GPS"): x = fix quality (0 = none), y = satellites
pub const GPS: u8 = 11;

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Event {
    pub kind: u8,
    pub _pad: u8,
    pub x: i16,
    pub y: i16,
    pub delta: i16,
}

const LEN: u8 = 16;

pub struct Queue {
    buf: UnsafeCell<[Event; LEN as usize]>,
    // low byte: index; above: bumped by every coalesce, so a pop that read a slot coalesce then rewrote retries
    head: AtomicU32,
    tail: AtomicU8,
}

// one producer (push, coalesce, clear) and one consumer (pop)
unsafe impl Sync for Queue {}

impl Queue {
    pub const fn new() -> Self {
        const EMPTY: Event = Event { kind: 0, _pad: 0, x: 0, y: 0, delta: 0 };
        Queue { buf: UnsafeCell::new([EMPTY; LEN as usize]), head: AtomicU32::new(0), tail: AtomicU8::new(0) }
    }

    fn slot(&self, i: u8) -> *mut Event {
        unsafe { (self.buf.get() as *mut Event).add(usize::from(i)) }
    }

    pub fn push(&self, e: Event) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let next = (tail + 1) % LEN;
        if next == self.head.load(Ordering::Acquire) as u8 {
            return false;
        }
        unsafe { self.slot(tail).write(e) };
        self.tail.store(next, Ordering::Release);
        true
    }

    /// Replaces a queued event with the same `kind`, `x` and `y` (the app sees only the latest value), else pushes.
    pub fn coalesce(&self, e: Event) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let mut i = self.head.load(Ordering::Acquire) as u8;
        while i != tail {
            let s = self.slot(i);
            let q = unsafe { s.read() };
            if (q.kind, q.x, q.y) == (e.kind, e.x, e.y) {
                unsafe { s.write(e) };
                self.head.fetch_add(0x100, Ordering::AcqRel);
                return true;
            }
            i = (i + 1) % LEN;
        }
        self.push(e)
    }

    pub fn pop(&self) -> Option<Event> {
        loop {
            let head = self.head.load(Ordering::Acquire);
            let i = head as u8;
            if i == self.tail.load(Ordering::Acquire) {
                return None;
            }
            let e = unsafe { self.slot(i).read() };
            // a clear() or coalesce() that preempted us changed head: drop e and read again
            let next = head & !0xFF | u32::from((i + 1) % LEN);
            if self.head.compare_exchange(head, next, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                return Some(e);
            }
        }
    }

    pub fn clear(&self) {
        let tail = self.tail.load(Ordering::Acquire);
        let _ = self.head.fetch_update(Ordering::AcqRel, Ordering::Acquire, |h| Some((h & !0xFF).wrapping_add(0x100) | u32::from(tail)));
    }
}

/// `BUTTON`'s `x`: 0 the crown press, 1 the back swipe (loader 3 on; before, both 0). An app that owns back may
/// close on the press and go back on the swipe.
pub const FROM_SWIPE: i16 = 1;

pub fn map_key(value: u32) -> Option<Event> {
    let (kind, delta, x) = match value {
        KEY_CROWN_CW => (CROWN, 1, 0),
        KEY_CROWN_CCW => (CROWN, -1, 0),
        KEY_CROWN_PRESS => (BUTTON, 0, 0),
        // the app leaves as on the button and the loader shows its menu, not the stock app list
        v if v & 0xFFFF == KEY_BACK => (BUTTON, 0, FROM_SWIPE),
        _ => return None,
    };
    Some(Event { kind, delta, x, ..Event::default() })
}

#[repr(C)]
pub struct Api {
    pub abi: u16,
    pub abi_minor: u16,
    pub width: u16,
    pub height: u16,
    pub fill_rect: extern "C" fn(i16, i16, u16, u16, u16),
    pub draw_text: extern "C" fn(i16, i16, *const u8, usize, u16),
    pub present: extern "C" fn(),
    pub next_event: extern "C" fn(*mut Event, u32) -> bool,
    pub motor: extern "C" fn(u16, u16) -> i32,
    pub millis: extern "C" fn() -> u32,
    pub exit: extern "C" fn(i32) -> !,
    // ABI 1.1 (docs/abi-1.1.md)
    pub send_msg: extern "C" fn(*const u8, usize) -> i32,
    pub recv_msg: extern "C" fn(*mut u8, usize) -> i32,
    pub load_data: extern "C" fn(*mut u8, usize) -> i32,
    pub save_data: extern "C" fn(*const u8, usize) -> i32,
    pub get_time: extern "C" fn(*mut Time) -> i32,
    pub get_battery: extern "C" fn() -> i32,
    pub screen_on: extern "C" fn(bool) -> i32,
    // ABI 1.2 (docs/specs/2026-11-11-lvgl-apps-design.md)
    pub lv_submit: extern "C" fn(*const u8, usize) -> i32,
    // ABI 1.3 (docs/abi-1.3.md)
    pub own_back: extern "C" fn(bool),
}

/// A running app is force-stopped this long after a BUTTON (`button_at`, millis | 1; 0 = none).
pub const ESCAPE_MS: u32 = 1000;
/// An app that owns "back" counts as hung once it has neither read the BUTTON nor called next_event for this long:
/// a view rebuild or a save may keep a live app away from its queue for more than ESCAPE_MS.
pub const HANG_MS: u32 = 5000;

/// Whether to force-stop the app now. An app that owns "back" (ABI 1.3) is stopped only if it did not read the
/// BUTTON and did not call next_event (`polled_at`, millis) for HANG_MS (hung); any other app always is.
pub fn escape_due(owned: bool, button_at: u32, consumed: bool, polled_at: u32, now: u32) -> bool {
    if button_at == 0 {
        return false;
    }
    if !owned {
        return now.wrapping_sub(button_at) >= ESCAPE_MS;
    }
    !consumed && now.wrapping_sub(button_at) >= HANG_MS && now.wrapping_sub(polled_at) >= HANG_MS
}

/// Stock msgbox id of the AOD overlay (`docs/recon.md`, "Screen-off and the app view"): the launcher opens it with
/// `msgbox_cache_popup(0x113, 0)` once the panel goes dark.
pub const AOD_MSGBOX: u16 = 0x113;

/// Whether a stock popup covers the app: `flags` = u8 `0x3800FAF4`, bit 0 with id `ids[0]` (`0x3800FAF6`), bit 1
/// with `ids[1]` (`0x3800FAF8`). The AOD overlay does not: the app keeps running (and measuring) in AOD.
pub fn popup_covers(flags: u8, ids: [u16; 2]) -> bool {
    (flags & 1 != 0 && ids[0] != AOD_MSGBOX) || (flags & 2 != 0 && ids[1] != AOD_MSGBOX)
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Time {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub weekday: u8,
    pub millis: u16,
    pub utc_offset_min: i16,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    #[test]
    fn queue_spsc() {
        let q = Queue::new();
        assert_eq!(q.pop(), None);
        for i in 0..15 {
            assert!(q.push(Event { kind: 1, x: i, ..Event::default() }));
        }
        assert!(!q.push(Event::default()));
        for i in 0..15 {
            assert_eq!(q.pop().map(|e| e.x), Some(i));
        }
        assert_eq!(q.pop(), None);
        for _ in 0..40 {
            assert!(q.push(Event::default()));
            assert!(q.pop().is_some());
        }
        q.push(Event::default());
        q.clear();
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn escape() {
        // any other app: 1 s after the press, read or not
        assert!(!escape_due(false, 1, true, 900, 1000));
        assert!(escape_due(false, 1, true, 1000, 1001));
        assert!(!escape_due(true, 0, false, 0, 99_999));
        // own_back: a read BUTTON never stops it
        assert!(!escape_due(true, 1, true, 0, 99_999));
        // busy (no next_event) for 2 s after the press: alive
        assert!(!escape_due(true, 1001, false, 900, 3001));
        // polling, BUTTON still behind other events: alive
        assert!(!escape_due(true, 1001, false, 9000, 10_000));
        // neither read nor polled for HANG_MS: hung
        assert!(escape_due(true, 1001, false, 900, 1001 + HANG_MS));
        assert!(escape_due(true, u32::MAX - 100, false, u32::MAX - 200, HANG_MS));
    }

    #[test]
    fn aod_is_no_popup() {
        assert!(!popup_covers(0, [262, 262]));
        assert!(popup_covers(1, [262, 0]));
        assert!(popup_covers(2, [0, 246]));
        assert!(!popup_covers(1, [AOD_MSGBOX, 0]));
        assert!(!popup_covers(2, [0, AOD_MSGBOX]));
        // a call box over AOD still pauses
        assert!(popup_covers(3, [AOD_MSGBOX, 246]));
    }

    #[test]
    fn value_events_coalesce() {
        let q = Queue::new();
        for v in 0..40 {
            q.coalesce(Event { kind: LV, x: 3, y: 28, delta: v, ..Event::default() });
        }
        q.push(Event { kind: BUTTON, ..Event::default() });
        assert_eq!(q.pop(), Some(Event { kind: LV, x: 3, y: 28, delta: 39, ..Event::default() }));
        assert_eq!(q.pop().map(|e| e.kind), Some(BUTTON));
        assert_eq!(q.pop(), None);
        // another handle is its own event; a popped event is not replaced
        q.coalesce(Event { kind: LV, x: 3, delta: 1, ..Event::default() });
        q.coalesce(Event { kind: LV, x: 4, delta: 2, ..Event::default() });
        assert_eq!(q.pop().map(|e| e.delta), Some(1));
        q.coalesce(Event { kind: LV, x: 3, delta: 5, ..Event::default() });
        assert_eq!(q.pop().map(|e| (e.x, e.delta)), Some((4, 2)));
        assert_eq!(q.pop().map(|e| (e.x, e.delta)), Some((3, 5)));
        // same handle, another event code: PRESSED stays, VALUE_CHANGED queues behind it
        q.push(Event { kind: LV, x: 3, y: 1, ..Event::default() });
        q.coalesce(Event { kind: LV, x: 3, y: 28, delta: 7, ..Event::default() });
        assert_eq!(q.pop().map(|e| (e.y, e.delta)), Some((1, 0)));
        assert_eq!(q.pop().map(|e| (e.y, e.delta)), Some((28, 7)));
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn key_mapping() {
        assert_eq!(map_key(0xFFFF_006F).map(|e| (e.kind, e.delta)), Some((3, 1)));
        assert_eq!(map_key(0xFFFF_006E).map(|e| (e.kind, e.delta)), Some((3, -1)));
        assert_eq!(map_key(0x0400_0001).map(|e| (e.kind, e.x)), Some((4, 0)));
        assert_eq!(map_key(0xFFFF_0071).map(|e| (e.kind, e.x)), Some((4, FROM_SWIPE)));
        assert_eq!(map_key(0x2000_0001), None);
    }

    #[test]
    fn api_layout_matches_sdk() {
        assert_eq!(size_of::<Api>(), size_of::<cmf_wp2_app::Api>());
        assert_eq!(offset_of!(Api, motor), offset_of!(cmf_wp2_app::Api, motor));
        assert_eq!(offset_of!(Api, exit), offset_of!(cmf_wp2_app::Api, exit));
        assert_eq!(offset_of!(Api, send_msg), offset_of!(cmf_wp2_app::Api, send_msg));
        assert_eq!(offset_of!(Api, screen_on), offset_of!(cmf_wp2_app::Api, screen_on));
        assert_eq!(offset_of!(Api, lv_submit), offset_of!(cmf_wp2_app::Api, lv_submit));
        assert_eq!(offset_of!(Api, own_back), offset_of!(cmf_wp2_app::Api, own_back));
        assert_eq!(size_of::<Time>(), size_of::<cmf_wp2_app::Time>());
        assert_eq!(offset_of!(Time, utc_offset_min), offset_of!(cmf_wp2_app::Time, utc_offset_min));
        assert_eq!(size_of::<Event>(), size_of::<cmf_wp2_app::Event>());
        assert_eq!(HEART, cmf_wp2_app::HEART);
    }
}
