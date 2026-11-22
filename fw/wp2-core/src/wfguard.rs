// Watch face guard (docs/recon.md, "Watch face boot guard"). The stub keeps a NAND flag {id, stage, deaths} for the
// active face from the moment it is picked or booted until it has run for SURVIVED_MS:
// - "picked" until it is parsed (a pick from the phone with the screen off loads at the next wake);
// - a boot that finds the flag at "loading" means the face took the watch down while it loaded: fall back now;
// - at "loaded" (LOADED_MS passed) only the MAX_DEATHS-th death in a row falls back. lv_timers stand still while the
//   screen is off, so this stage can last until the next wake and one unrelated reset must not cost the user's face;
// - power off, the menu reboot and the OTA reboot drop the flag.
// One set of hooks for both models; `Device` holds what differs (WP2 1.0.0.73 and Watch 3 Pro 1.0.1.77).
use core::ffi::c_void;
use core::mem::transmute;

pub const FLAG: &[u8] = b"/NAND:/wp2wfgrd\0";
// The repro faults ~0.15 s after the load and a hung parser blocks the timer too. Has to stay under the shortest
// screen timeout (5 s, the default): a later timer does not run before the next wake.
pub const LOADED_MS: u32 = 3_000;
// covers the first minute tick
pub const SURVIVED_MS: u32 = 65_000;
pub const MAX_DEATHS: u8 = 3;
// data_storage type that holds the WfmPara mirror on both models
pub const STORAGE_SETTINGS: u32 = 1;

// WfmPara: u8 count at 4, u8 active index at 5, u8 att at 6, u8 kind at 7, u32 active id at 8, entries of 16 B at
// 0xC (u32 id, u8 att, photo data), checksum in the last 4 bytes
const ENTRIES: usize = 0xC;
// fs_open flags (fs.rs)
const FS_READ: u8 = 1;
const FS_WRITE_CREATE: u8 = 0x12;
const FILE_SIZE: usize = 0x2C;
// lv_timer_t.user_data (both builds' lv_timer_create: str r5, [r4, #0xc])
const TIMER_USER_DATA: usize = 0xC;

/// Thumb addresses (| 1) of the stock calls the hook makes, per firmware build.
pub struct Device {
    /// WfmPara of WFManager
    pub para: u32,
    /// its RAM mirror inside data_storage type 1 (unaligned): the list as last saved
    pub para_mirror: u32,
    /// (para) refreshes the entries' att bytes: the last call before WFManager_Init takes the active entry, after
    /// both the saved list and the default one
    pub para_refresh: u32,
    /// WfmPara_Save(para): magic, checksum, copy to the RAM mirror only (asserts on count 0)
    pub para_save: u32,
    /// (id) -> the att byte a list entry keeps (wfc header +0x17)
    pub face_att: u32,
    /// WFManager_Parser(file, &face), as WFManager_Load_ByIdx calls it for a face it has not loaded yet
    pub parser: u32,
    /// data_storage(type, -1): the type's RAM buffer as a new record of its NOR ring
    pub storage_write: u32,
    pub timer_create: u32,
    pub timer_del: u32,
    pub fs_open: u32,
    pub fs_close: u32,
    pub fs_read: u32,
    pub fs_write: u32,
    pub fs_unlink: u32,
    /// called on every power off and menu reboot, factory reset included
    pub power_prepare: u32,
    /// save_all_data, called by the OTA's forced reboot
    pub save_all: u32,
    /// WfmPara size, checksum included
    pub para_size: usize,
    /// stock built-in faces, never deleted from NAND
    pub builtin: &'static [u32],
}

// JX402 1.0.0.73: WFManager_Init 0x100DE020, para 0x3801AE50, mirror 0x3800BC9D in type 1 (0x3800ACD0, 0x12F8 B,
// NOR 0x488000); built-ins at 0x101F3D54
pub const WP2: Device = Device {
    para: 0x3801_AE50,
    para_mirror: 0x3800_BC9D,
    para_refresh: 0x101B_93B7,
    para_save: 0x100D_EBC5,
    face_att: 0x100D_C6B1,
    parser: 0x100D_C3C1,
    storage_write: 0x101B_5A6F,
    timer_create: crate::stock::LV_TIMER_CREATE,
    timer_del: crate::stock::LV_TIMER_DEL,
    fs_open: crate::stock::FS_OPEN,
    fs_close: crate::stock::FS_CLOSE,
    fs_read: crate::stock::FS_READ,
    fs_write: crate::stock::FS_WRITE,
    fs_unlink: crate::stock::FS_UNLINK,
    power_prepare: 0x1014_A8FD,
    save_all: 0x100C_6235,
    para_size: 0x80,
    builtin: &[0x111, 0x112, 0x113, 0x114, 0x115, 0x118],
};

// Watch 3 Pro jx402_08 1.0.1.77: WFManager_Init 0x100D6DAC, para 0x38026CAC, mirror 0x3800F73A in type 1
// (0x3800E1A0, 0x2430 B); built-ins at 0x10220810 (ids -1..-3 are the photo and other kinds)
pub const WP3: Device = Device {
    para: 0x3802_6CAC,
    para_mirror: 0x3800_F73A,
    para_refresh: 0x101D_5A03,
    para_save: 0x100D_7A25,
    face_att: 0x100D_4C71,
    parser: 0x100D_48B5,
    storage_write: 0x101D_0ECD,
    timer_create: 0x1014_8E6D,
    timer_del: 0x1014_8F05,
    fs_open: 0x1011_5F19,
    fs_close: 0x1011_60C9,
    fs_read: 0x1011_6121,
    fs_write: 0x1011_617D,
    fs_unlink: 0x1011_6639,
    power_prepare: 0x1015_70C1,
    save_all: 0x100B_A94D,
    para_size: 0xF0,
    builtin: &[0x112, 0x114, 0x113, 0x335, 0x336, 0x337, 0x338],
};

/// The stub's two lv_timer callbacks, which call `loaded` and `survived`.
#[derive(Clone, Copy)]
pub struct Timers {
    pub loaded: extern "C" fn(*mut c_void),
    pub survived: extern "C" fn(*mut c_void),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// made active, not parsed yet
    Picked,
    /// parsed less than LOADED_MS ago
    Loading,
    Loaded,
}

/// The flag file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Watch {
    pub id: u32,
    pub stage: Stage,
    /// boots in a row that this face did not survive after it had loaded
    pub deaths: u8,
}

impl Watch {
    const TAG: u8 = b'G';

    pub fn encode(self) -> [u8; 8] {
        let id = self.id.to_le_bytes();
        [Self::TAG, self.stage as u8, self.deaths, 0, id[0], id[1], id[2], id[3]]
    }

    pub fn decode(raw: &[u8]) -> Option<Self> {
        match *raw {
            [Self::TAG, stage @ 0..=2, deaths, 0, a, b, c, d] => {
                let stage = [Stage::Picked, Stage::Loading, Stage::Loaded][usize::from(stage)];
                Some(Self { id: u32::from_le_bytes([a, b, c, d]), stage, deaths })
            }
            _ => None,
        }
    }
}

/// At boot, with `active` about to load: the flag to set, None to fall back. A flag of another face is stale: the
/// change to that face was lost with the reset, so `active` is the one from before it.
pub fn at_boot(flag: Option<Watch>, active: u32) -> Option<Watch> {
    match flag.filter(|w| w.id == active) {
        None => Some(Watch { id: active, stage: Stage::Loading, deaths: 0 }),
        Some(w) if w.stage == Stage::Picked => Some(Watch { stage: Stage::Loading, ..w }),
        Some(w) if w.stage == Stage::Loading || w.deaths.saturating_add(1) >= MAX_DEATHS => None,
        Some(w) => Some(Watch { stage: Stage::Loading, deaths: w.deaths + 1, ..w }),
    }
}

fn id_at(para: &[u8], i: usize) -> u32 {
    let o = ENTRIES + i * 16;
    u32::from_le_bytes([para[o], para[o + 1], para[o + 2], para[o + 3]])
}

/// Makes a built-in face active: the next listed built-in after the active entry, round the list, else the active
/// entry becomes the built-in after it in `builtin` (the other entries are downloads or the photo face and stay
/// listed). Going round means a series of bad built-ins ends at a good one. Returns the new index, None for an empty
/// list. The caller fills the entry's att byte and saves (checksum).
pub fn fall_back(para: &mut [u8], builtin: &[u32]) -> Option<usize> {
    let n = usize::from(para[4]).min((para.len() - 4 - ENTRIES) / 16);
    if n == 0 {
        return None;
    }
    let cur = usize::from(para[5]) % n;
    let bad = id_at(para, cur);
    let pick = (1..n).map(|k| (cur + k) % n).find(|&i| builtin.contains(&id_at(para, i)) && id_at(para, i) != bad);
    let i = pick.unwrap_or_else(|| {
        let o = ENTRIES + cur * 16;
        let id = builtin.iter().position(|&b| b == bad).map_or(builtin[0], |at| builtin[(at + 1) % builtin.len()]);
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

fn read_flag(dev: &Device) -> Option<Watch> {
    let mut file = [0u8; FILE_SIZE];
    if call!(dev.fs_open, fn(*mut u8, *const u8, u8) -> i32)(file.as_mut_ptr(), FLAG.as_ptr(), FS_READ) < 0 {
        return None;
    }
    let mut raw = [0u8; 8];
    let n = call!(dev.fs_read, fn(*mut u8, *mut u8, u32) -> i32)(file.as_mut_ptr(), raw.as_mut_ptr(), 8);
    call!(dev.fs_close, fn(*mut u8) -> i32)(file.as_mut_ptr());
    if n == 8 { Watch::decode(&raw) } else { None }
}

// always 8 bytes from offset 0, so an existing file needs no truncate
fn write_flag(dev: &Device, watch: Watch) {
    let mut file = [0u8; FILE_SIZE];
    if call!(dev.fs_open, fn(*mut u8, *const u8, u8) -> i32)(file.as_mut_ptr(), FLAG.as_ptr(), FS_WRITE_CREATE) >= 0 {
        let raw = watch.encode();
        call!(dev.fs_write, fn(*mut u8, *const u8, u32) -> i32)(file.as_mut_ptr(), raw.as_ptr(), 8);
        call!(dev.fs_close, fn(*mut u8) -> i32)(file.as_mut_ptr());
    }
}

fn drop_flag(dev: &Device) {
    call!(dev.fs_unlink, fn(*const u8) -> i32)(FLAG.as_ptr());
}

fn arm(dev: &Device, watch: Watch, timers: Timers) {
    write_flag(dev, watch);
    let create = call!(dev.timer_create, fn(extern "C" fn(*mut c_void), u32, *mut c_void) -> *mut c_void);
    // the newest timer runs first: after a wake with both due, `survived` drops the flag and `loaded` finds none
    create(timers.loaded, LOADED_MS, watch.id as usize as *mut c_void);
    create(timers.survived, SURVIVED_MS, watch.id as usize as *mut c_void);
}

/// Replaces `bl para_refresh` in WFManager_Init (once per boot, ui_service): the list is final here, whether it was
/// saved or is the default one, and the active face is not loaded yet.
///
/// # Safety
/// `para` is the stock's WfmPara of `dev.para_size` bytes; the addresses in `dev` match the running firmware.
pub unsafe fn boot(dev: &Device, para: *mut u8, timers: Timers) -> i32 {
    let rc = call!(dev.para_refresh, fn(*mut u8) -> i32)(para);
    let p = unsafe { core::slice::from_raw_parts_mut(para, dev.para_size) };
    let n = usize::from(p[4]).min((p.len() - 4 - ENTRIES) / 16);
    if n == 0 {
        return rc;
    }
    let active = id_at(p, usize::from(p[5]) % n);
    let watch = match at_boot(read_flag(dev), active) {
        Some(watch) => watch,
        None => {
            let Some(i) = fall_back(p, dev.builtin) else { return rc };
            let id = id_at(p, i);
            let o = ENTRIES + i * 16;
            p[o + 4] = call!(dev.face_att, fn(u32) -> u8)(id);
            p[6] = p[o + 4];
            call!(dev.para_save, fn(*mut u8) -> i32)(para);
            // to NOR now: the save only fills the RAM copy, and the next reset may be one that skips the flush
            call!(dev.storage_write, fn(u32, i32) -> i32)(STORAGE_SETTINGS, -1);
            Watch { id, stage: Stage::Loading, deaths: 0 }
        }
    };
    arm(dev, watch, timers);
    rc
}

/// Replaces `bl` of the stock's "save the list" wrapper at its call sites after boot (ui_service, each before the
/// load): a newly picked face is under watch before it is parsed. Only a change of the active id counts, a list edit
/// must not restart the watch of a face that runs.
///
/// # Safety
/// The addresses in `dev` match the running firmware.
pub unsafe fn list_saved(dev: &Device) -> i32 {
    let para = dev.para as usize as *mut u8;
    let active = |list: *const u8| unsafe { list.add(8).cast::<u32>().read_unaligned() };
    let (id, before) = (active(para), active(dev.para_mirror as usize as *const u8));
    call!(dev.para_save, fn(*mut u8) -> i32)(para);
    // ponytail: new content under the id that is already active is not re-armed, it costs one more fault
    if id != before {
        write_flag(dev, Watch { id, stage: Stage::Picked, deaths: 0 });
    }
    0
}

/// Replaces `bl parser` in WFManager_Load_ByIdx: a picked face's watch starts as it is parsed.
// ponytail: the photo face does not go through the parser, picked it costs one more fault
///
/// # Safety
/// `file` and `out` are the stock's arguments; the addresses in `dev` match the running firmware.
pub unsafe fn parse(dev: &Device, file: *mut u8, out: *mut u8, timers: Timers) -> i32 {
    let active = unsafe { (dev.para as usize as *const u8).add(8).cast::<u32>().read_unaligned() };
    if let Some(watch) = read_flag(dev).filter(|w| w.id == active && w.stage == Stage::Picked) {
        arm(dev, Watch { stage: Stage::Loading, ..watch }, timers);
    }
    call!(dev.parser, fn(*mut u8, *mut u8) -> i32)(file, out)
}

fn timer_face(timer: *mut c_void) -> u32 {
    unsafe { timer.cast::<u8>().add(TIMER_USER_DATA).cast::<u32>().read() }
}

/// LOADED_MS timer body: the face got through its load.
///
/// # Safety
/// `timer` is the lv_timer that runs this callback.
pub unsafe fn loaded(dev: &Device, timer: *mut c_void) {
    let id = timer_face(timer);
    if let Some(watch) = read_flag(dev).filter(|w| w.id == id && w.stage == Stage::Loading) {
        write_flag(dev, Watch { stage: Stage::Loaded, ..watch });
    }
    call!(dev.timer_del, fn(*mut c_void) -> ())(timer);
}

/// SURVIVED_MS timer body. A timer of a face picked before the active one leaves the flag.
///
/// # Safety
/// `timer` is the lv_timer that runs this callback.
pub unsafe fn survived(dev: &Device, timer: *mut c_void) {
    if read_flag(dev).is_some_and(|w| w.id == timer_face(timer) && w.stage != Stage::Picked) {
        drop_flag(dev);
    }
    call!(dev.timer_del, fn(*mut c_void) -> ())(timer);
}

/// Replaces `bl power_prepare` in power off and the menu reboot (after their factory reset branch).
///
/// # Safety
/// The addresses in `dev` match the running firmware.
pub unsafe fn power_down(dev: &Device) {
    drop_flag(dev);
    call!(dev.power_prepare, fn() -> ())()
}

/// Replaces `bl save_all` in the OTA's forced reboot.
///
/// # Safety
/// The addresses in `dev` match the running firmware.
pub unsafe fn forced_reboot(dev: &Device) {
    drop_flag(dev);
    call!(dev.save_all, fn() -> ())()
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

    #[test]
    fn bad_builtins_never_loop() {
        // two bad built-ins used to swap with each other for ever, the good one was never tried
        for dev in [&WP2, &WP3] {
            let mut p = para(dev.para_size, &dev.builtin[..3], 0);
            fall_back(&mut p, dev.builtin);
            assert_eq!(fall_back(&mut p, dev.builtin), Some(2));
            let mut p = para(dev.para_size, &dev.builtin[..1], 0);
            let mut tried = vec![];
            for _ in dev.builtin {
                fall_back(&mut p, dev.builtin);
                tried.push(id_at(&p, 0));
            }
            tried.sort();
            let mut all = dev.builtin.to_vec();
            all.sort();
            assert_eq!(tried, all);
        }
    }

    #[test]
    fn a_death_while_loading_falls_back_at_once() {
        assert_eq!(at_boot(Some(Watch { id: 0x112, stage: Stage::Loading, deaths: 0 }), 0x112), None);
    }

    #[test]
    fn deaths_after_the_load_fall_back_on_the_third() {
        let mut watch = at_boot(None, 0x112).unwrap();
        for deaths in 1..MAX_DEATHS {
            watch = at_boot(Some(Watch { stage: Stage::Loaded, ..watch }), 0x112).unwrap();
            assert_eq!(watch, Watch { id: 0x112, stage: Stage::Loading, deaths });
        }
        assert_eq!(at_boot(Some(Watch { stage: Stage::Loaded, ..watch }), 0x112), None);
    }

    #[test]
    fn a_flag_of_another_face_is_stale() {
        // the change to the bad face was lost with the reset, the watch is back on the face before it
        let flag = Watch { id: 0x4000_0001, stage: Stage::Loading, deaths: 2 };
        assert_eq!(at_boot(Some(flag), 0x112), Some(Watch { id: 0x112, stage: Stage::Loading, deaths: 0 }));
    }

    #[test]
    fn a_pick_that_never_loaded_is_no_death() {
        // picked over BLE with the screen off, reset before the next wake
        let flag = Watch { id: 0x335, stage: Stage::Picked, deaths: 0 };
        assert_eq!(at_boot(Some(flag), 0x335), Some(Watch { stage: Stage::Loading, ..flag }));
    }

    #[test]
    fn flag_round_trip() {
        for watch in [Watch { id: 0xFFFF_FFFF, stage: Stage::Loading, deaths: 0 }, Watch { id: 0x112, stage: Stage::Loaded, deaths: 2 }] {
            assert_eq!(Watch::decode(&watch.encode()), Some(watch));
        }
        assert_eq!(Watch::decode(&[]), None);
        assert_eq!(Watch::decode(&[0; 8]), None);
    }
}
