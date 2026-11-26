// Heart rate for apps (docs/abi-1.4.md, "Heart rate"): the app's SENSOR request is a wish; the UI tick starts the
// stock measurement while the app runs in the foreground, the panel dark (AOD) included, and stops it on pause, exit,
// crash and force stop. The app thread hands out the readings: the UI tick does not run while the panel is dark. Start and stop as the stock heart view does them (docs/recon.md, "Heart rate").
use crate::app::{millis, QUEUE};
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use wp2_core::events::Event;
use wp2_core::heart::{self, Heart, SENSOR_HEART};
use crate::plat::stock::{HR_BPM, HR_OWNER, HR_WEAR, PROP_HR_MODE, UX2SYS_SET};

const E_INVAL: i32 = -22;

static WANT: AtomicBool = AtomicBool::new(false);
static ON: AtomicBool = AtomicBool::new(false);
static EMIT_AT: AtomicU32 = AtomicU32::new(0);
static mut HEART: Heart = Heart::new();

fn byte(addr: u32) -> u8 {
    unsafe { (addr as *const u8).read_volatile() }
}

// app thread (lv_submit)
pub fn request(sensor: u8, on: bool) -> i32 { #[cfg(feature = "wp3")] return crate::plat::E_NODEV; // heart rate cells unverified: M3
    if sensor != SENSOR_HEART {
        return E_INVAL;
    }
    WANT.store(on, Ordering::Release);
    0
}

fn set_mode(on: u32) {
    stock!(UX2SYS_SET, fn(u32, *const u32) -> i32)(PROP_HR_MODE, &on);
}

// UI thread
pub fn tick(running: bool, paused: bool) { #[cfg(feature = "wp3")] return;
    let want = WANT.load(Ordering::Acquire) && running && !paused;
    let s = unsafe { (*addr_of_mut!(HEART)).tick(millis(), want, byte(HR_OWNER) != 0) };
    if s.start {
        // as the stock heart view: no stale reading from an earlier measurement
        unsafe { (HR_BPM as *mut u8).write_volatile(0) };
        set_mode(1);
    }
    ON.store(want, Ordering::Release);
    if s.stop {
        set_mode(0);
        if running {
            // no reading while it is off: the app must not keep the last one
            QUEUE.coalesce(heart::event(byte(HR_WEAR), 0));
        }
    }
}

// app thread (next_event): a HEART event every EMIT_MS while it measures, lit or dark (the UI tick stops with the
// panel). Handed to the app directly: QUEUE has one producer, the UI thread, and a coalesce from here could
// overwrite an event it just pushed.
pub fn due() -> Option<Event> { #[cfg(feature = "wp3")] return None;
    let now = millis();
    if !ON.load(Ordering::Acquire) || now.wrapping_sub(EMIT_AT.load(Ordering::Relaxed)) < heart::EMIT_MS {
        return None;
    }
    EMIT_AT.store(now, Ordering::Relaxed);
    Some(heart::event(byte(HR_WEAR), byte(HR_BPM)))
}

// UI thread, app thread aborted
pub fn stop() { #[cfg(feature = "wp3")] return;
    WANT.store(false, Ordering::Release);
    tick(false, false);
}
