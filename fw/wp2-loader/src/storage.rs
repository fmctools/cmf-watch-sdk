//! JX402 1.0.0.73 read-only metadata service. UI thread only; see docs/storage-inspection.md.
use core::mem::transmute;
use crate::plat::{fs, stock}; use wp2_core::{layout, pkg, storage};

// WP2: verified from partition_init at 0x100f6f40; stock validates this ACPT pointer at boot (per model).
use crate::plat::obj::TABLE_PTR;
use crate::plat::obj::FAT_OPS;

fn table() -> Result<&'static [u8], i32> {
    let p = unsafe { (TABLE_PTR as *const u32).read_volatile() } as usize;
    if !((0x1000_0000..=0x11ff_f000).contains(&p) || (0x3800_0000..=0x387f_f000).contains(&p)) {
        return Err(-5);
    }
    let b = unsafe { core::slice::from_raw_parts(p as *const u8, storage::TABLE_BYTES) };
    storage::partition_count(b)?;
    Ok(b)
}
fn mount() -> Result<u32, i32> {
    let mut mp = 0;
    let find: extern "C" fn(*mut u32, *const u8, *mut usize) -> i32 =
        unsafe { transmute(stock::FS_FIND_MOUNT as usize) };
    let rc = find(&mut mp, c"/NAND:".as_ptr().cast(), core::ptr::null_mut());
    if rc < 0 {
        return Err(rc);
    }
    if !(0x3800_0000..0x3880_0000).contains(&mp) {
        return Err(-5);
    }
    if unsafe { ((mp + 0x1c) as *const u32).read() } != FAT_OPS {
        return Err(-95);
    }
    Ok(mp)
}
fn size(mp: u32, path: &[u8]) -> Result<u32, i32> {
    // Production dirent, see stock::FS_STAT.
    let mut entry = [0u32; 66];
    let stat: extern "C" fn(u32, *const u8, *mut u32) -> i32 =
        unsafe { transmute(stock::FS_STAT as usize) };
    let rc = stat(mp, path.as_ptr(), entry.as_mut_ptr());
    if rc == -2 {
        return Ok(0);
    }
    if rc < 0 {
        return Err(rc);
    }
    Ok(entry[65])
}
fn refresh() -> Result<[u32; 8], i32> {
    let mp = mount()?;
    let statvfs: extern "C" fn(u32, *const u8, *mut u32) -> i32 =
        unsafe { transmute(stock::FS_STATVFS as usize) };
    let mut raw = [0; 4];
    let rc = statvfs(mp, c"/NAND:".as_ptr().cast(), raw.as_mut_ptr());
    if rc < 0 {
        return Err(rc);
    }
    let (total, free) = storage::capacity(&raw)?;
    let (mut packages, mut data, mut widgets, mut slots) = (0u32, 0u32, 0u32, 0u32);
    for slot in 0..crate::plat::stub_ram().slots() {
        let n = size(mp, &layout::slot_path(slot, false))?;
        slots += (n > 0) as u32;
        packages = packages.checked_add(n).ok_or(-75)?;
        data = data
            .checked_add(size(mp, &layout::data_path(slot, false))?)
            .ok_or(-75)?;
        widgets = widgets
            .checked_add(size(mp, &layout::widget_path(slot))?)
            .ok_or(-75)?;
    }
    let loader = size(mp, &layout::slot_path(layout::LOADER_SLOT, false))?;
    Ok([total, free, packages, data, widgets, loader, slots, raw[1]])
}
// Bounded snapshot: one directory entry per step, one open directory, explicit partial/error state.
// FAT operation addresses and byte-sized fs_dirent type match the saved stock vtable audit.
const FILE_MAX: usize = 96;
const DIR_MAX: usize = 16;
const PATH_MAX: usize = 96;
#[derive(Clone, Copy)]
struct FileInfo {
    path: [u8; PATH_MAX],
    size: u32,
}
#[derive(Clone, Copy)]
struct AppInfo {
    name: [u8; 24],
    version: u32,
    valid: bool,
    icon: bool,
}
struct ScanBuffer {
    files: [FileInfo; FILE_MAX],
    dirs: [[u8; PATH_MAX]; DIR_MAX],
}
static mut SCAN_BUFFER: *mut ScanBuffer = core::ptr::null_mut();
fn buffer() -> &'static mut ScanBuffer {
    unsafe { &mut *SCAN_BUFFER }
}
fn allocate_scan() -> Result<(), i32> {
    unsafe {
        if SCAN_BUFFER.is_null() {
            let n = core::mem::size_of::<ScanBuffer>();
            let p = crate::plat::heap::alloc(crate::plat::heap::RES, n as u32, 64);
            if p.is_null() {
                return Err(-12);
            }
            p.write_bytes(0, n);
            SCAN_BUFFER = p.cast();
        }
    }
    Ok(())
}
static mut DIR: [u32; 4] = [0; 4];
static mut DIR_OPEN: bool = false;
static mut DIR_COUNT: usize = 0;
static mut DIR_AT: usize = 0;
static mut FILE_COUNT: usize = 0;
static mut PARTIAL: bool = false;
static mut SCAN_ERROR: i32 = 0;
static mut SCAN_ACTIVE: bool = false;
fn read_app(slot: u8) -> AppInfo {
    let mut info = AppInfo {
        name: [0; 24],
        version: 0,
        valid: false,
        icon: false,
    };
    let mut file = [0; 0x2c];
    if fs::open(&mut file, &layout::slot_path(slot, false), fs::READ) >= 0 {
        let mut raw = [0; pkg::HEADER_SIZE];
        let ok = fs::read_exact(&mut file, &mut raw);
        let mut desc = [0; pkg::ICON_DESC];
        let icon_read = fs::read_exact(&mut file, &mut desc);
        fs::close(&mut file);
        if ok {
            if let Ok(h) = pkg::parse_header(&raw) {
                if pkg::check_slot(&h, slot).is_ok() {
                    info = AppInfo {
                        name: h.name,
                        version: h.version,
                        valid: true,
                        icon: icon_read && pkg::icon_ok(&desc, h.image_size),
                    };
                }
            }
        }
    }
    info
}
fn close_dir() {
    if unsafe { DIR_OPEN } {
        let close: extern "C" fn(*mut u32) -> i32 =
            unsafe { transmute(stock::FS_CLOSEDIR as usize) };
        let rc = close(core::ptr::addr_of_mut!(DIR).cast());
        unsafe {
            DIR_OPEN = false;
            if rc < 0 {
                PARTIAL = true;
                SCAN_ERROR = rc;
            }
        }
    }
}
fn begin_scan() -> Result<i32, i32> {
    mount()?;
    close_dir();
    allocate_scan()?;
    unsafe {
        SCAN_ACTIVE = true;
        FILE_COUNT = 0;
        DIR_COUNT = 1;
        DIR_AT = 0;
        PARTIAL = false;
        SCAN_ERROR = 0;
        buffer().dirs.fill([0; PATH_MAX]);
        buffer().dirs[0][..6].copy_from_slice(b"/NAND:");
    }
    Ok(0)
}
fn status() -> i32 {
    unsafe {
        FILE_COUNT as i32
            | if DIR_AT >= DIR_COUNT { 0x10000 } else { 0 }
            | if PARTIAL { 0x20000 } else { 0 }
    }
}
fn scan_step() -> Result<i32, i32> {
    if unsafe { DIR_AT >= DIR_COUNT } {
        return Ok(status());
    }
    if !unsafe { DIR_OPEN } {
        let open: extern "C" fn(*mut u32, *const u8) -> i32 =
            unsafe { transmute(stock::FS_OPENDIR as usize) };
        unsafe {
            DIR = [0; 4];
        }
        let rc = open(core::ptr::addr_of_mut!(DIR).cast(), unsafe {
            buffer().dirs[DIR_AT].as_ptr()
        });
        if rc < 0 {
            unsafe {
                PARTIAL = true;
                SCAN_ERROR = rc;
                DIR_AT += 1;
            }
            return Ok(status());
        }
        unsafe {
            DIR_OPEN = true;
        }
    }
    let mut entry = [0u32; 66];
    let read: extern "C" fn(*mut u32, *mut u32) -> i32 =
        unsafe { transmute(stock::FS_READDIR as usize) };
    let rc = read(core::ptr::addr_of_mut!(DIR).cast(), entry.as_mut_ptr());
    let raw = unsafe { core::slice::from_raw_parts(entry.as_ptr().cast::<u8>(), 264) };
    if rc < 0 || raw[1] == 0 {
        close_dir();
        unsafe {
            DIR_AT += 1;
            if rc < 0 {
                PARTIAL = true;
                SCAN_ERROR = rc;
            }
        }
        return Ok(status());
    }
    let n = raw[1..257].iter().position(|&c| c == 0).unwrap_or(256);
    let name = &raw[1..1 + n];
    if name == b"." || name == b".." {
        return Ok(status());
    }
    let parent = unsafe { buffer().dirs[DIR_AT] };
    let len = parent.iter().position(|&c| c == 0).unwrap_or(PATH_MAX);
    if len + 1 + n >= PATH_MAX {
        unsafe {
            PARTIAL = true;
        }
        return Ok(status());
    }
    let mut path = [0; PATH_MAX];
    path[..len].copy_from_slice(&parent[..len]);
    path[len] = b'/';
    path[len + 1..len + 1 + n].copy_from_slice(name);
    unsafe {
        if raw[0] == 1 {
            if DIR_COUNT < DIR_MAX {
                buffer().dirs[DIR_COUNT] = path;
                DIR_COUNT += 1;
            } else {
                PARTIAL = true;
            }
        } else if FILE_COUNT < FILE_MAX {
            buffer().files[FILE_COUNT] = FileInfo {
                path,
                size: entry[65],
            };
            FILE_COUNT += 1;
        } else {
            PARTIAL = true;
        }
    }
    Ok(status())
}
/// All handles belong to the running app. The UI thread calls this only after aborting that thread.
pub fn stop() { #[cfg(feature = "wp3")] return;
    close_dir();
    unsafe {
        FILE_COUNT = 0;
        DIR_COUNT = 0;
        DIR_AT = 0;
        SCAN_ACTIVE = false;
        crate::plat::heap::free(crate::plat::heap::RES, SCAN_BUFFER.cast());
        SCAN_BUFFER = core::ptr::null_mut();
    }
}

/// The app block was range-checked by lv::submit. UI thread only; no caller pointers are stored.
pub fn execute(buf: &mut [u8]) -> i32 { #[cfg(feature = "wp3")] return -95; // ENOTSUP: dirent 0x10C, partition table not checked: M3
    let Some((op, index)) = storage::request(buf) else {
        return -22;
    };
    let out = &mut buf[8..];
    out.fill(0);
    fn put(out: &mut [u8], at: usize, v: u32) {
        out[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    let result = (|| -> Result<(), i32> {
        let i = index as usize;
        match op {
            0 if i == 0 => {
                out[..2].copy_from_slice(&(storage::VERSION as u16).to_le_bytes());
                out[2] = crate::plat::stub_ram().slots();
                out[3] = storage::MAX_PARTITIONS as u8;
                out[4..6].copy_from_slice(&(FILE_MAX as u16).to_le_bytes());
                out[6..8].copy_from_slice(&(DIR_MAX as u16).to_le_bytes());
                out[8..10].copy_from_slice(&(PATH_MAX as u16).to_le_bytes());
                out[10] = 3;
            }
            1 if i == 0 => {
                for (n, v) in refresh()?.into_iter().enumerate() {
                    put(out, n * 4, v);
                }
            }
            2 if i == 0 => {
                out[0] = storage::partition_count(table()?)? as u8;
            }
            3 => {
                let t = table()?;
                let m = storage::partition(t, i, 0)?;
                put(out, 0, storage::partition(t, i, 3)?);
                put(out, 4, storage::partition(t, i, 4)?);
                out[8] = m as u8;
                out[9] = (m >> 8) as u8;
                out[10] = (m >> 20) as u8;
                out[11] = ((m >> 16) & 15) as u8;
                put(out, 12, storage::partition(t, i, 1)?);
                put(out, 16, storage::partition(t, i, 2)?);
            }
            4 if i < (crate::plat::stub_ram().slots() as usize) => {
                let a = read_app(i as u8);
                out[0] = a.valid as u8;
                out[1] = a.icon as u8;
                out[2] = i as u8;
                out[4..28].copy_from_slice(&a.name);
                put(out, 28, a.version);
            }
            5 if i == 0 => {
                begin_scan()?;
            }
            6 if i == 0 && unsafe { SCAN_ACTIVE } => {
                let n = scan_step()?;
                out[..2].copy_from_slice(&(n as u16).to_le_bytes());
                out[2] = ((n >> 16) & 1) as u8;
                out[3] = ((n >> 17) & 1) as u8;
                put(out, 4, unsafe { SCAN_ERROR } as u32);
            }
            7 if i == 0 => {
                stop();
            }
            8 if unsafe { SCAN_ACTIVE } && i < unsafe { FILE_COUNT } => {
                let f = buffer().files[i];
                put(out, 0, f.size);
                out[4..4 + PATH_MAX].copy_from_slice(&f.path);
            }
            _ => return Err(-22),
        }
        Ok(())
    })();
    result.map_or_else(|e| e, |_| 0)
}
