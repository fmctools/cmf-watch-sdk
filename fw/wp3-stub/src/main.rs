#![no_std]
#![no_main]
// Watch 3 Pro (jx402_08 1.0.1.77): only the boot watch face guard (wp2-core wfguard.rs). `wp2apps firmware --model
// wp3` appends this at 0x102BD280 and points `bl WfmPara_Load` at 0x100D6DF4 (WFManager_Init) to `wf_para_load`.
use core::ffi::c_void;
use wp2_core::wfguard::{self, WP3};

#[repr(C)]
pub struct Header {
    magic: [u8; 4],
    version: u32,
    wf_para_load: extern "C" fn(*mut u8) -> i32,
}

#[used]
#[unsafe(link_section = ".header")]
pub static HEADER: Header = Header { magic: *b"WP3S", version: 1, wf_para_load: wp3_wf_para_load };

#[unsafe(no_mangle)]
pub extern "C" fn wp3_wf_para_load(para: *mut u8) -> i32 {
    unsafe { wfguard::para_load(&WP3, para, confirm) }
}

extern "C" fn confirm(timer: *mut c_void) {
    wfguard::confirmed(&WP3, timer);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { core::arch::asm!("udf #0", options(noreturn)) }
}
