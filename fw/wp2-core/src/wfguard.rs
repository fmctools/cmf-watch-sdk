// Boot watch face guard (docs/recon.md, "Watch face boot guard"). The stub arms a NAND flag before the stock loads
// the active face at boot and clears it once the face has run for CONFIRM_MS; a flag still set at the next boot means
// the face took the watch down, and `fall_back` makes a built-in face active in WfmPara before the stock loads it.
// One hook for both models; `Device` holds what differs (addresses are for WP2 1.0.0.73 and Watch 3 Pro 1.0.1.77).
use core::ffi::c_void;
use core::mem::transmute;
use core::ptr::null_mut;

pub const FLAG: &[u8] = b"/NAND:/wp2wfgrd\0";
// the repro faults ~0.15 s after the load, a hung parser trips the watchdog at 11 s
pub const CONFIRM_MS: u32 = 20_000;
// data_storage type that holds the WfmPara mirror on both models
pub const STORAGE_SETTINGS: u32 = 1;

// WfmPara: u8 count at 4, u8 active index at 5, u8 att at 6, u8 kind at 7, u32 active id at 8, entries of 16 B at
// 0xC (u32 id, u8 att, photo data), checksum in the last 4 bytes
const ENTRIES: usize = 0xC;
// fs_open flags (fs.rs)
const FS_READ: u8 = 1;
const FS_WRITE_CREATE: u8 = 0x12;

/// Thumb addresses (| 1) of the stock calls the hook makes, per firmware build.
pub struct Device {
    /// WfmPara_Load(para): 0 when the saved list (RAM mirror) is valid
    pub para_load: u32,
    /// WfmPara_Save(para): magic, checksum, copy to the RAM mirror only (asserts on count 0)
    pub para_save: u32,
    /// (id) -> the att byte a list entry keeps (wfc header +0x17)
    pub face_att: u32,
    /// data_storage(type, -1): the type's RAM buffer as a new record of its NOR ring
    pub storage_write: u32,
    pub timer_create: u32,
    pub timer_del: u32,
    pub fs_open: u32,
    pub fs_close: u32,
    pub fs_unlink: u32,
    /// WfmPara size, checksum included
    pub para_size: usize,
    /// stock built-in faces, never deleted from NAND
    pub builtin: &'static [u32],
}

// JX402 1.0.0.73: WFManager_Init 0x100DE020 calls WfmPara_Load at 0x100DE068, para 0x3801AE50, mirror 0x3800BC9D in
// type 1 (0x3800ACD0, 0x12F8 B, NOR 0x488000); built-ins at 0x101F3D54
pub const WP2: Device = Device {
    para_load: 0x100D_EB35,
    para_save: 0x100D_EBC5,
    face_att: 0x100D_C6B1,
    storage_write: 0x101B_5A6F,
    timer_create: crate::stock::LV_TIMER_CREATE,
    timer_del: crate::stock::LV_TIMER_DEL,
    fs_open: crate::stock::FS_OPEN,
    fs_close: crate::stock::FS_CLOSE,
    fs_unlink: crate::stock::FS_UNLINK,
    para_size: 0x80,
    builtin: &[0x111, 0x112, 0x113, 0x114, 0x115, 0x118],
};

// Watch 3 Pro jx402_08 1.0.1.77: WFManager_Init 0x100D6DAC calls WfmPara_Load at 0x100D6DF4, para 0x38026CAC, mirror
// 0x3800F73A in type 1 (0x3800E1A0, 0x2430 B); built-ins at 0x10220810 (ids -1..-3 are the photo and other kinds)
pub const WP3: Device = Device {
    para_load: 0x100D_7991,
    para_save: 0x100D_7A25,
    face_att: 0x100D_4C71,
    storage_write: 0x101D_0ECD,
    timer_create: 0x1014_8E6D,
    timer_del: 0x1014_8F05,
    fs_open: 0x1011_5F19,
    fs_close: 0x1011_60C9,
    fs_unlink: 0x1011_6639,
    para_size: 0xF0,
    builtin: &[0x112, 0x114, 0x113, 0x335, 0x336, 0x337, 0x338],
};

fn id_at(para: &[u8], i: usize) -> u32 {
    let o = ENTRIES + i * 16;
    u32::from_le_bytes([para[o], para[o + 1], para[o + 2], para[o + 3]])
}

/// Makes a built-in face active: the first listed built-in other than the active face, else the active entry
/// becomes builtin[0] (the other entries are downloads or the photo face and stay listed). Returns the new index,
/// None for an empty list. The caller fills the entry's att byte and saves (checksum).
pub fn fall_back(para: &mut [u8], builtin: &[u32]) -> Option<usize> {
    let n = usize::from(para[4]).min((para.len() - 4 - ENTRIES) / 16);
    if n == 0 {
        return None;
    }
    let cur = usize::from(para[5]) % n;
    let bad = id_at(para, cur);
    let pick = (0..n).find(|&i| i != cur && builtin.contains(&id_at(para, i)) && id_at(para, i) != bad);
    let i = pick.unwrap_or_else(|| {
        let o = ENTRIES + cur * 16;
        let id = if bad == builtin[0] { builtin[1] } else { builtin[0] };
        para[o..o + 16].fill(0);
        para[o..o + 4].copy_from_slice(&id.to_le_bytes());
        cur
    });
    para[5] = i as u8;
    para[7] = 0;
    let id = id_at(para, i);
    para[8..12].copy_from_slice(&id.to_le_bytes());
    Some(i)
}

macro_rules! call {
    ($addr:expr, fn($($t:ty),*) -> $r:ty) => {
        unsafe { transmute::<usize, extern "C" fn($($t),*) -> $r>($addr as usize) }
    };
}

/// Body of the stub's replacement for `bl WfmPara_Load` in WFManager_Init (once per boot, ui_service, before the
/// stock loads the active face). One flag create and one unlink per boot; `confirm` is the stub's lv_timer callback
/// that calls `confirmed`.
///
/// # Safety
/// `para` is the stock's WfmPara of `dev.para_size` bytes; the addresses in `dev` match the running firmware.
pub unsafe fn para_load(dev: &Device, para: *mut u8, confirm: extern "C" fn(*mut c_void)) -> i32 {
    let rc = call!(dev.para_load, fn(*mut u8) -> i32)(para);
    let mut file = [0u8; 0x2C];
    let open = call!(dev.fs_open, fn(*mut u8, *const u8, u8) -> i32);
    let close = call!(dev.fs_close, fn(*mut u8) -> i32);
    if open(file.as_mut_ptr(), FLAG.as_ptr(), FS_READ) >= 0 {
        // the last boot never confirmed its face: it faulted or hung loading it
        close(file.as_mut_ptr());
        let p = unsafe { core::slice::from_raw_parts_mut(para, dev.para_size) };
        if rc == 0
            && let Some(i) = fall_back(p, dev.builtin)
        {
            let o = ENTRIES + i * 16;
            p[o + 4] = call!(dev.face_att, fn(u32) -> u8)(id_at(p, i));
            p[6] = p[o + 4];
            call!(dev.para_save, fn(*mut u8) -> i32)(para);
            // to NOR now: the save only fills the RAM copy, and the confirm drops the flag
            call!(dev.storage_write, fn(u32, i32) -> i32)(STORAGE_SETTINGS, -1);
        }
    } else {
        file = [0; 0x2C];
        if open(file.as_mut_ptr(), FLAG.as_ptr(), FS_WRITE_CREATE) >= 0 {
            close(file.as_mut_ptr());
        }
    }
    // ponytail: confirms on time alone; a face that faults later than CONFIRM_MS (first minute tick) is not caught
    call!(dev.timer_create, fn(extern "C" fn(*mut c_void), u32, *mut c_void) -> *mut c_void)(confirm, CONFIRM_MS, null_mut());
    rc
}

/// The confirm timer's body: the face ran CONFIRM_MS, drop the flag and the timer.
pub fn confirmed(dev: &Device, timer: *mut c_void) {
    call!(dev.fs_unlink, fn(*const u8) -> i32)(FLAG.as_ptr());
    call!(dev.timer_del, fn(*mut c_void) -> ())(timer);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(size: usize, ids: &[u32], idx: u8) -> Vec<u8> {
        let mut p = vec![0u8; size];
        p[4] = ids.len() as u8;
        p[5] = idx;
        for (i, id) in ids.iter().enumerate() {
            let o = ENTRIES + i * 16;
            p[o..o + 4].copy_from_slice(&id.to_le_bytes());
            p[o + 4] = 0xC;
        }
        p
    }

    fn ids(p: &[u8]) -> Vec<u32> {
        (0..usize::from(p[4])).map(|i| id_at(p, i)).collect()
    }

    #[test]
    fn switches_to_another_builtin() {
        let mut p = para(WP2.para_size, &[0x4000_0001, 0x112, 0x111], 0);
        assert_eq!(fall_back(&mut p, WP2.builtin), Some(1));
        assert_eq!((p[5], id_at(&p, 1), ids(&p)), (1, 0x112, vec![0x4000_0001, 0x112, 0x111]));
        assert_eq!(&p[8..12], &0x112u32.to_le_bytes());
    }

    #[test]
    fn bad_builtin_skipped() {
        // the emulator repro: the default face file itself is the bad one
        let mut p = para(WP2.para_size, &[0x111, 0x112, 0x113], 0);
        assert_eq!(fall_back(&mut p, WP2.builtin), Some(1));
        let mut p = para(WP2.para_size, &[0x111, 0xFFFF_FFFF, 0x112], 2);
        assert_eq!(fall_back(&mut p, WP2.builtin), Some(0));
        // Watch 3 Pro: 274 is the default, 273 is no built-in there
        let mut p = para(WP3.para_size, &[0x112, 0x111, 0x335], 0);
        assert_eq!(fall_back(&mut p, WP3.builtin), Some(2));
    }

    #[test]
    fn no_builtin_listed_replaces_the_active_entry() {
        let mut p = para(WP2.para_size, &[0xFFFF_FFFF, 0x4000_0001, 0x4000_0002], 1);
        assert_eq!(fall_back(&mut p, WP2.builtin), Some(1));
        assert_eq!(ids(&p), vec![0xFFFF_FFFF, 0x111, 0x4000_0002]);
        assert_eq!(p[ENTRIES + 16 + 4], 0);
        let mut p = para(WP2.para_size, &[0x111], 0);
        assert_eq!(fall_back(&mut p, WP2.builtin), Some(0));
        assert_eq!(ids(&p), vec![0x112]);
        let mut p = para(WP3.para_size, &[0x4000_0001], 0);
        assert_eq!(fall_back(&mut p, WP3.builtin), Some(0));
        assert_eq!(ids(&p), vec![0x112]);
    }

    #[test]
    fn index_wraps_and_count_is_clamped() {
        let mut p = para(WP2.para_size, &[0x4000_0001, 0x113], 2);
        assert_eq!(fall_back(&mut p, WP2.builtin), Some(1));
        assert_eq!(fall_back(&mut para(WP2.para_size, &[], 0), WP2.builtin), None);
        // past the 7 (WP2) or 14 (Watch 3 Pro) entries that fit before the checksum
        let mut p = para(WP2.para_size, &[0x4000_0001; 7], 0);
        p[4] = 10;
        assert_eq!(fall_back(&mut p, WP2.builtin), Some(0));
        let mut p = para(WP3.para_size, &[0x4000_0001; 14], 13);
        p[4] = 20;
        assert_eq!(fall_back(&mut p, WP3.builtin), Some(13));
        assert_eq!(id_at(&p, 13), 0x112);
    }
}
