#![no_std]
#![no_main]
// Watch 3 Pro (jx402_08 1.0.1.77): only the watch face guard (wp2-core wfguard.rs). `wp2apps firmware --model wp3`
// appends this at 0x102BD280 and points the stock's calls at the header's hooks (src/wp2apps/firmware.py, GUARD_WP3).
use core::ffi::c_void;
use wp2_core::wfguard::{self, Timers, WP3};

#[repr(C)]
pub struct Header {
    magic: [u8; 4],
    version: u32,
    wf_boot: extern "C" fn(*mut u8) -> i32,
    wf_saved: extern "C" fn() -> i32,
    power_down: extern "C" fn(),
    forced_reboot: extern "C" fn(),
}

#[used]
#[unsafe(link_section = ".header")]
pub static HEADER: Header = Header {
    magic: *b"WP3S",
    version: 1,
    wf_boot: wp3_wf_boot,
    wf_saved: wp3_wf_saved,
    power_down: wp3_power_down,
    forced_reboot: wp3_forced_reboot,
};

#[unsafe(no_mangle)]
pub extern "C" fn wp3_wf_boot(para: *mut u8) -> i32 {
    unsafe { wfguard::boot(&WP3, para, TIMERS) }
}

pub extern "C" fn wp3_wf_saved() -> i32 {
    unsafe { wfguard::list_saved(&WP3, TIMERS) }
}

pub extern "C" fn wp3_power_down() {
    unsafe { wfguard::power_down(&WP3) }
}

pub extern "C" fn wp3_forced_reboot() {
    unsafe { wfguard::forced_reboot(&WP3) }
}

const TIMERS: Timers = Timers { loaded, survived };

extern "C" fn loaded(timer: *mut c_void) {
    unsafe { wfguard::loaded(&WP3, timer) }
}

extern "C" fn survived(timer: *mut c_void) {
    unsafe { wfguard::survived(&WP3, timer) }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { core::arch::asm!("udf #0", options(noreturn)) }
}
