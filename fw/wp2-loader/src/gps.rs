// GPS for apps (docs/abi-1.4.md, "GPS"), the heart rate's pattern (hr.rs): the app's SENSOR request is a wish; the
// UI tick sets ux2sys 0x63 to 1 while the app runs in the foreground (panel dark included) and back to 0 on pause,
// exit, crash and force stop. The stock keeps no request count for the GPS (one mode, last writer wins), so a GPS
// already on at the app's start (a stock workout, its list) is only read. The app thread hands out GPS events and
// fixes. Coordinates are never logged.
use crate::app::{millis, QUEUE};
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use wp2_core::events::Event;
use wp2_core::gps::{self, Hold, FIX_LEN, MODE_ON, RAW_LEN};
use crate::plat::stock::{GPS_INFO, GPS_STATE, PROP_GPS_MODE, UX2SYS_SET};

const E_PERM: i32 = -1;

static WANT: AtomicBool = AtomicBool::new(false);
static ON: AtomicBool = AtomicBool::new(false);
static EMIT_AT: AtomicU32 = AtomicU32::new(0);
static mut HOLD: Hold = Hold::new();

// app thread (lv_submit)
pub fn request(on: bool) -> i32 { #[cfg(feature = "wp3")] return crate::plat::E_NODEV; // GPS: M3
    WANT.store(on, Ordering::Release);
    0
}

fn set_mode(mode: u32) {
    stock!(UX2SYS_SET, fn(u32, *const u32) -> i32)(PROP_GPS_MODE, &mode);
}

fn stock_on() -> bool {
    unsafe { ((GPS_STATE + 1) as *const u8).read_volatile() != 0 }
}

// UI thread
pub fn tick(running: bool, paused: bool) { #[cfg(feature = "wp3")] return;
    let want = WANT.load(Ordering::Acquire) && running && !paused;
    let s = unsafe { (*addr_of_mut!(HOLD)).tick(millis(), want, stock_on()) };
    if s.start {
        set_mode(MODE_ON);
    }
    ON.store(want, Ordering::Release);
    if s.stop {
        set_mode(0);
        if running {
            // no fix while it is off: the app must not keep the last one
            QUEUE.coalesce(gps::event(&[0; FIX_LEN]));
        }
    }
}

// The parser (gps_service) clears and refills the block at every GGA: read until two copies agree.
fn read() -> [u8; FIX_LEN] {
    let copy = || {
        let mut r = [0u8; RAW_LEN];
        for (i, b) in r.iter_mut().enumerate() {
            *b = unsafe { ((GPS_INFO as usize + i) as *const u8).read_volatile() };
        }
        r
    };
    let mut a = copy();
    for _ in 0..3 {
        let b = copy();
        if a == b {
            break;
        }
        a = b;
    }
    gps::fix(&a, millis())
}

// app thread (lv_submit, GPS_FIX record in the app block): overwrites the payload with the latest fix
pub fn fix(out: *mut u8) -> i32 { #[cfg(feature = "wp3")] return crate::plat::E_NODEV;
    if !ON.load(Ordering::Acquire) {
        return E_PERM;
    }
    let f = read();
    unsafe { core::ptr::copy_nonoverlapping(f.as_ptr(), out, FIX_LEN) };
    0
}

// app thread (next_event): a GPS event every EMIT_MS while the app holds it, lit or dark
pub fn due() -> Option<Event> { #[cfg(feature = "wp3")] return None;
    let now = millis();
    if !ON.load(Ordering::Acquire) || now.wrapping_sub(EMIT_AT.load(Ordering::Relaxed)) < gps::EMIT_MS {
        return None;
    }
    EMIT_AT.store(now, Ordering::Relaxed);
    Some(gps::event(&read()))
}

// UI thread, app thread aborted
pub fn stop() { #[cfg(feature = "wp3")] return;
    WANT.store(false, Ordering::Release);
    tick(false, false);
}
