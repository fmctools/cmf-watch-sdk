#![no_std]
#![no_main]
// Watch 3 Pro (jx402_08 1.0.1.77): the watch face guard (wp2-core wfguard.rs), an "Apps" entry in the stock app
// list (applist.rs) and the stub's view `model::WP3.menu_view` (menu.rs). `wp2apps firmware --model wp3` appends this
// at 0x102BD280, points the stock's calls and app-list update slots at the header's hooks and adds a view-table
// record for the view (src/wp2apps/firmware.py, GUARD_WP3, WP3_APPLIST, add_stub_wp3).
mod applist;
mod menu;

use core::ffi::c_void;
use wp2_core::model;
use wp2_core::wfguard::{self, Timers, WP3};

#[repr(C)]
pub struct Header {
    magic: [u8; 4],
    version: u32,
    wf_boot: extern "C" fn(*mut u8) -> i32,
    wf_saved: extern "C" fn() -> i32,
    power_down: extern "C" fn(),
    forced_reboot: extern "C" fn(),
    wf_parse: extern "C" fn(*mut u8, *mut u8) -> i32,
    /// version 2: the update handlers of the app list's List and Grid style (applist.rs)
    applist_list: extern "C" fn(*mut u8),
    applist_grid: extern "C" fn(*mut u8),
    /// version 2: the view proc and the name of the view-table record for `model::WP3.menu_view` (menu.rs)
    view_proc: extern "C" fn(u32, u32, *mut u8) -> i32,
    view_name: &'static [u8; 13],
}

unsafe impl Sync for Header {}

#[used]
#[unsafe(link_section = ".header")]
pub static HEADER: Header = Header {
    magic: *b"WP3S",
    version: 2,
    wf_boot: wp3_wf_boot,
    wf_saved: wp3_wf_saved,
    power_down: wp3_power_down,
    forced_reboot: wp3_forced_reboot,
    wf_parse: wp3_wf_parse,
    applist_list: applist::wp3_applist_list,
    applist_grid: applist::wp3_applist_grid,
    view_proc: menu::wp3_view_proc,
    view_name: &menu::NAME,
};

// ResFbHeap blocks through the stock UI allocator (type 2), as the stock app list allocates its icons
const RES: u32 = 2;

pub(crate) fn alloc(size: u32) -> *mut u8 {
    let f: extern "C" fn(u32, u32, u32, *const u8) -> *mut u8 =
        unsafe { core::mem::transmute(model::WP3.ui_mem_aligned_alloc as usize) };
    f(RES, 0x40, size, b"wp3s\0".as_ptr())
}

pub(crate) fn free(block: *mut u8) {
    let f: extern "C" fn(u32, *mut u8) = unsafe { core::mem::transmute(model::WP3.ui_mem_free as usize) };
    f(RES, block)
}

#[unsafe(no_mangle)]
pub extern "C" fn wp3_wf_boot(para: *mut u8) -> i32 {
    unsafe { wfguard::boot(&WP3, para, TIMERS) }
}

pub extern "C" fn wp3_wf_saved() -> i32 {
    unsafe { wfguard::list_saved(&WP3) }
}

pub extern "C" fn wp3_wf_parse(file: *mut u8, out: *mut u8) -> i32 {
    unsafe { wfguard::parse(&WP3, file, out, TIMERS) }
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
