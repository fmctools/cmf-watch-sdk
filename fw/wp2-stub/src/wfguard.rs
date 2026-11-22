// Watch face guard (wp2-core wfguard.rs): the bodies behind the stub header's version 2 hooks. `wp2apps firmware`
// points the stock's calls at them (src/wp2apps/firmware.py, GUARD_WP2).
use core::ffi::c_void;
use wp2_core::wfguard::{self, Timers, WP2};

pub extern "C" fn wp2_wf_boot(para: *mut u8) -> i32 {
    unsafe { wfguard::boot(&WP2, para, TIMERS) }
}

pub extern "C" fn wp2_wf_saved() -> i32 {
    unsafe { wfguard::list_saved(&WP2) }
}

pub extern "C" fn wp2_wf_parse(file: *mut u8, out: *mut u8) -> i32 {
    unsafe { wfguard::parse(&WP2, file, out, TIMERS) }
}

pub extern "C" fn wp2_power_down() {
    unsafe { wfguard::power_down(&WP2) }
}

pub extern "C" fn wp2_forced_reboot() {
    unsafe { wfguard::forced_reboot(&WP2) }
}

const TIMERS: Timers = Timers { loaded, survived };

extern "C" fn loaded(timer: *mut c_void) {
    unsafe { wfguard::loaded(&WP2, timer) }
}

extern "C" fn survived(timer: *mut c_void) {
    unsafe { wfguard::survived(&WP2, timer) }
}
