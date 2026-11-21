// Boot watch face guard (wp2-core wfguard.rs). Replaces `bl WfmPara_Load` at 0x100DE068 in WFManager_Init, which
// runs once on ui_service before the stock loads the active face. One flag create and one unlink per boot.
use core::ffi::c_void;
use core::mem::transmute;
use core::ptr::null_mut;
use wp2_core::fs::{self, File};
use wp2_core::stock::{
    DATA_STORAGE_WRITE, LV_TIMER_CREATE, LV_TIMER_DEL, STORAGE_SETTINGS, WFM_FACE_ATT, WFM_PARA_LOAD, WFM_PARA_SAVE,
};
use wp2_core::wfguard::{fall_back, CONFIRM_MS, FLAG, PARA_SIZE};

macro_rules! stock {
    ($addr:expr, fn($($t:ty),*) -> $r:ty) => {
        unsafe { transmute::<usize, extern "C" fn($($t),*) -> $r>($addr as usize) }
    };
}

pub extern "C" fn wp2_wf_para_load(para: *mut [u8; PARA_SIZE]) -> i32 {
    let rc = stock!(WFM_PARA_LOAD, fn(*mut [u8; PARA_SIZE]) -> i32)(para);
    let mut file: File = [0; 0x2C];
    if fs::open(&mut file, FLAG, fs::READ) >= 0 {
        // the last boot never confirmed its face: it faulted or hung loading it
        fs::close(&mut file);
        let p = unsafe { &mut *para };
        if rc == 0 {
            if let Some(i) = fall_back(p) {
                let o = 0xC + i * 16;
                let id = u32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]]);
                p[o + 4] = stock!(WFM_FACE_ATT, fn(u32) -> u8)(id);
                p[6] = p[o + 4];
                stock!(WFM_PARA_SAVE, fn(*mut [u8; PARA_SIZE]) -> i32)(para);
                // to NOR now: the save only fills the RAM copy, and the confirm below drops the flag
                stock!(DATA_STORAGE_WRITE, fn(u32, i32) -> i32)(STORAGE_SETTINGS, -1);
            }
        }
    } else if fs::open(&mut file, FLAG, fs::WRITE | fs::CREATE) >= 0 {
        fs::close(&mut file);
    }
    // ponytail: confirms on time alone; a face that faults later than CONFIRM_MS (first minute tick) is not caught
    stock!(LV_TIMER_CREATE, fn(extern "C" fn(*mut c_void), u32, *mut c_void) -> *mut c_void)(confirm, CONFIRM_MS, null_mut());
    rc
}

extern "C" fn confirm(timer: *mut c_void) {
    fs::unlink(FLAG);
    stock!(LV_TIMER_DEL, fn(*mut c_void) -> ())(timer);
}
