// Boot watch face guard (wp2-core wfguard.rs). Replaces `bl WfmPara_Load` at 0x100DE068 in WFManager_Init.
use core::ffi::c_void;
use wp2_core::wfguard::{self, WP2};

pub extern "C" fn wp2_wf_para_load(para: *mut u8) -> i32 {
    unsafe { wfguard::para_load(&WP2, para, confirm) }
}

extern "C" fn confirm(timer: *mut c_void) {
    wfguard::confirmed(&WP2, timer);
}
