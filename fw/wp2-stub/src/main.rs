#![no_std]
#![no_main]

mod applist;
mod ble;
mod card;
mod view;
mod wfguard;

use core::ffi::c_void;
use wp2_core::layout::{stub_ram, StubRam};

#[repr(C)]
pub struct StubHeader {
    magic: [u8; 4],
    version: u32,
    boot: unsafe extern "C" fn(),
    ble: unsafe extern "C" fn(),
    view_proc: extern "C" fn(u32, u32, *mut c_void) -> i32,
    view_name: *const u8,
    applist_layout: extern "C" fn(*mut u8) -> i32,
    /// wgt_reg entry for the free widget id 0x40 (card.rs); was reserved (0) before
    widget: &'static card::WgtReg,
    /// version 2: the watch face guard's four hooks (wfguard.rs)
    wf_boot: extern "C" fn(*mut u8) -> i32,
    wf_saved: extern "C" fn() -> i32,
    power_down: extern "C" fn(),
    forced_reboot: extern "C" fn(),
}

unsafe impl Sync for StubHeader {}

#[unsafe(link_section = ".header")]
#[used]
static HEADER: StubHeader = StubHeader {
    magic: *b"WP2S",
    version: 2,
    boot: wp2_boot,
    ble: ble::wp2_ble_default,
    view_proc: view::wp2_view_proc,
    view_name: b"wp2_view\0".as_ptr(),
    applist_layout: applist::wp2_applist_layout,
    widget: &card::WIDGET,
    wf_boot: wfguard::wp2_wf_boot,
    wf_saved: wfguard::wp2_wf_saved,
    power_down: wfguard::wp2_power_down,
    forced_reboot: wfguard::wp2_forced_reboot,
};

// Replaces `bl 0x10146AA4` (ResFbHeap init, 4 of its 8 arguments on the stack) at 0x101884FA.
#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "C" fn wp2_boot() {
    core::arch::naked_asm!(
        "push {{r0-r4, lr}}",
        "bl {init}",
        "pop {{r0-r4, lr}}",
        "b.w stock_heap_init",
        init = sym wp2_boot_c,
    )
}

extern "C" fn wp2_boot_c() {
    let ram = stub_ram();
    unsafe { core::ptr::write_bytes(ram as *mut StubRam, 0, 1) };
    ram.magic = 0x5750_3253;
    ram.running_slot = 0xFF;
    ram.api = 1;
    ram.kick = ble::wp2_kick as usize as u32;
    ram.kick_magic = wp2_core::layout::KICK_MAGIC;
    ram.stub_version = wp2_core::layout::STUB_VERSION;
    ram.version_magic = wp2_core::layout::VERSION_MAGIC;
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    // a fault goes through the Zephyr fatal handler and reboots the watch instead of hanging a stock thread
    unsafe { core::arch::asm!("udf #0", options(noreturn)) }
}
