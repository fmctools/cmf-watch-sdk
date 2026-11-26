//! Model-generic versions of the stock-calling helpers in `fs.rs`, `heap.rs`, `load.rs` and `layout::stub_ram`, taking
//! `&'static Model` (as `wfguard` takes `&Device`). The Watch 3 Pro stub and loader use them with `model::WP3`; the WP2
//! stub and loader keep the WP2-only originals, so their builds stay byte-identical.
//!
//! Package header byte 0x38 (reserved, zero in every WP2 package) is the loader's model: `model::Model::model_id`
//! (2) for a Watch 3 Pro loader, 0 for a WP2 one (and for every app). A WP2 stub rejects any non-zero reserved byte (`pkg::parse_header`),
//! so it refuses a WP3 loader; `loader_model_ok` is the WP3 stub's side.
use crate::model::Model;
use crate::pkg::HEADER_SIZE;

/// Header offset of the loader model byte (the first reserved byte).
pub const MODEL_BYTE: usize = 0x38;

/// For a package header read from storage: whether it fits this model (a loader must carry its model byte, an app
/// none), and the header with the model byte cleared so `pkg::parse_header` accepts it.
pub fn loader_model_ok(m: &Model, raw: &[u8; HEADER_SIZE]) -> Option<[u8; HEADER_SIZE]> {
    let kind = raw[6];
    // WP2 loaders predate the byte and keep it 0
    let want = if kind == crate::pkg::KIND_LOADER && m.model_id != 1 { m.model_id as u8 } else { 0 };
    if raw[MODEL_BYTE] != want {
        return None;
    }
    let mut clean = *raw;
    clean[MODEL_BYTE] = 0;
    Some(clean)
}

/// Local seconds since 1970 to a calendar date (days-from-civil inverse, proleptic Gregorian).
pub fn civil(secs: i64) -> crate::events::Time {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    crate::events::Time {
        year: year as u16,
        month: month as u8,
        day: day as u8,
        hour: (rem / 3600) as u8,
        minute: (rem / 60 % 60) as u8,
        second: (rem % 60) as u8,
        // 1970-01-01 was a Thursday
        weekday: (days + 4).rem_euclid(7) as u8,
        millis: 0,
        utc_offset_min: 0,
    }
}

#[cfg(target_os = "none")]
pub use target::*;

#[cfg(target_os = "none")]
mod target {
    use super::*;
    use crate::heap::fits;
    use crate::layout::{slot_path, StubRam, E_BAD, E_IO, E_NOMEM};
    use crate::pkg::{check_slot, crc32, parse_header, relocate, Header};
    use core::arch::asm;
    use core::mem::transmute;

    pub fn stub_ram(m: &Model) -> &'static mut StubRam {
        unsafe { &mut *(m.stub_ram as *mut StubRam) }
    }

    // --- fs (as fs.rs)
    pub type File = [u8; 0x2C];
    pub const READ: u8 = 1;
    pub const WRITE: u8 = 2;
    pub const CREATE: u8 = 0x10;
    const ENOTSUP: i32 = -134;

    fn terminated(path: &[u8]) -> bool {
        path.contains(&0)
    }

    pub fn open(m: &Model, file: &mut File, path: &[u8], flags: u8) -> i32 {
        if !terminated(path) {
            return E_BAD;
        }
        *file = [0; 0x2C];
        let f: extern "C" fn(*mut File, *const u8, u8) -> i32 = unsafe { transmute(m.fs_open as usize) };
        f(file, path.as_ptr(), flags)
    }

    pub fn read(m: &Model, file: &mut File, buf: &mut [u8]) -> i32 {
        let f: extern "C" fn(*mut File, *mut u8, usize) -> i32 = unsafe { transmute(m.fs_read as usize) };
        f(file, buf.as_mut_ptr(), buf.len())
    }

    pub fn write(m: &Model, file: &mut File, buf: &[u8]) -> i32 {
        let f: extern "C" fn(*mut File, *const u8, usize) -> i32 = unsafe { transmute(m.fs_write as usize) };
        f(file, buf.as_ptr(), buf.len())
    }

    pub fn seek(m: &Model, file: &mut File, offset: u32) -> i32 {
        let f: extern "C" fn(*mut File, i32, i32) -> i32 = unsafe { transmute(m.fs_seek as usize) };
        f(file, offset as i32, 0)
    }

    pub fn close(m: &Model, file: &mut File) -> i32 {
        let f: extern "C" fn(*mut File) -> i32 = unsafe { transmute(m.fs_close as usize) };
        f(file)
    }

    pub fn unlink(m: &Model, path: &[u8]) -> i32 {
        if !terminated(path) {
            return E_BAD;
        }
        let f: extern "C" fn(*const u8) -> i32 = unsafe { transmute(m.fs_unlink as usize) };
        f(path.as_ptr())
    }

    pub fn rename(m: &Model, from: &[u8], to: &[u8]) -> i32 {
        if !terminated(from) || !terminated(to) {
            return E_BAD;
        }
        let f: extern "C" fn(*const u8, *const u8) -> i32 = unsafe { transmute(m.fs_rename as usize) };
        f(from.as_ptr(), to.as_ptr())
    }

    /// No fs_mkdir wrapper in either image: the mount point's ops vtable entry `fs_ops_mkdir`.
    pub fn mkdir(m: &Model, path: &[u8]) -> i32 {
        if !terminated(path) {
            return E_BAD;
        }
        let find: extern "C" fn(*mut u32, *const u8, *mut usize) -> i32 =
            unsafe { transmute(m.fs_find_mount as usize) };
        let mut mp = 0u32;
        let rc = find(&mut mp, path.as_ptr(), core::ptr::null_mut());
        if rc < 0 {
            return rc;
        }
        let ops = unsafe { *((mp + m.fs_mount_ops) as *const u32) };
        let mkdir = if ops == 0 { 0 } else { unsafe { *((ops + m.fs_ops_mkdir) as *const u32) } };
        if mkdir == 0 {
            return ENOTSUP;
        }
        let f: extern "C" fn(u32, *const u8) -> i32 = unsafe { transmute(mkdir as usize) };
        f(mp, path.as_ptr())
    }

    pub fn read_exact(m: &Model, file: &mut File, buf: &mut [u8]) -> bool {
        let mut done = 0;
        while done < buf.len() {
            let n = read(m, file, &mut buf[done..]);
            if n <= 0 {
                return false;
            }
            done += n as usize;
        }
        true
    }

    // --- heap (as heap.rs)
    const MAX_STEPS: u32 = 4096;

    struct Basepri(u32);

    impl Basepri {
        fn raise() -> Self {
            let old: u32;
            unsafe {
                asm!("mrs {}, basepri", "msr basepri_max, {}", out(reg) old, in(reg) 0x20u32, options(nostack));
            }
            Basepri(old)
        }
    }

    impl Drop for Basepri {
        fn drop(&mut self) {
            unsafe { asm!("msr basepri, {}", in(reg) self.0, options(nostack)) }
        }
    }

    pub fn alloc(m: &Model, ty: u32, size: u32, align: u32) -> *mut u8 {
        let limit = stub_ram(m).alloc_limit;
        if limit != 0 && size > limit {
            return core::ptr::null_mut();
        }
        let _guard = Basepri::raise();
        let free = unsafe { *((m.heap + m.heap_free) as *const u32) };
        let mut node = unsafe { *((m.heap + m.heap_free_list) as *const u32) };
        let mut steps = 0;
        let mut sizes = core::iter::from_fn(|| {
            if node == 0 {
                return None;
            }
            steps += 1;
            if steps > MAX_STEPS {
                return None;
            }
            let s = unsafe { *((node + 4) as *const u32) } & 0xFF_FFFF;
            node = unsafe { *(node as *const u32) };
            Some(s)
        });
        let ok = fits(free, &mut sizes, size, align);
        drop(sizes);
        if steps > MAX_STEPS || !ok {
            return core::ptr::null_mut();
        }
        let f: extern "C" fn(u32, u32, u32, *const u8) -> *mut u8 =
            unsafe { transmute(m.ui_mem_aligned_alloc as usize) };
        f(ty, align, size, b"wp2\0".as_ptr())
    }

    /// # Safety
    /// `p` is NULL or a block of type `ty` from `alloc`.
    pub unsafe fn free(m: &Model, ty: u32, p: *mut u8) {
        if p.is_null() {
            return;
        }
        let f: extern "C" fn(u32, *mut u8) = unsafe { transmute(m.ui_mem_free as usize) };
        f(ty, p)
    }

    // --- load (as load.rs, with the model byte check)
    pub fn sync_psram(m: &Model, block: *mut u8, len: u32) {
        let f: extern "C" fn(*mut u8, u32) -> i32 = unsafe { transmute(m.psram_cache_flush_invalidate as usize) };
        unsafe { asm!("dsb sy", options(nostack)) };
        f(block, len);
        unsafe { asm!("dsb sy", "isb sy", options(nostack)) };
    }

    /// Reads the package of `slot` into a fresh RES heap block and relocates it there.
    pub fn load(m: &Model, slot: u8, file: &mut File, scratch: &mut [u8]) -> Result<(*mut u8, Header), i32> {
        let path = slot_path(slot, false);
        if open(m, file, &path, READ) < 0 {
            return Err(E_IO);
        }
        let result = load_open(m, slot, file, scratch);
        close(m, file);
        result
    }

    fn load_open(m: &Model, slot: u8, file: &mut File, scratch: &mut [u8]) -> Result<(*mut u8, Header), i32> {
        let mut raw = [0u8; HEADER_SIZE];
        if !read_exact(m, file, &mut raw) {
            return Err(E_IO);
        }
        let clean = loader_model_ok(m, &raw).ok_or(E_BAD)?;
        let header = parse_header(&clean).map_err(|_| E_BAD)?;
        check_slot(&header, slot).map_err(|_| E_BAD)?;
        let block = alloc(m, crate::heap::RES, header.image_size, 8);
        if block.is_null() {
            return Err(E_NOMEM);
        }
        let image = unsafe { core::slice::from_raw_parts_mut(block, header.image_size as usize) };
        match fill(m, file, image, scratch, &header) {
            Ok(()) => {
                sync_psram(m, block, header.image_size);
                Ok((block, header))
            }
            Err(e) => {
                unsafe { free(m, crate::heap::RES, block) };
                Err(e)
            }
        }
    }

    fn fill(m: &Model, file: &mut File, image: &mut [u8], scratch: &mut [u8], header: &Header) -> Result<(), i32> {
        if !read_exact(m, file, image) {
            return Err(E_IO);
        }
        let mut crc = crc32(0, image);
        let base = image.as_ptr() as u32;
        let step = scratch.len() & !3;
        let mut left = 4 * header.reloc_count as usize;
        let mut next = 0;
        while left > 0 {
            let chunk = &mut scratch[..left.min(step)];
            if !read_exact(m, file, chunk) {
                return Err(E_IO);
            }
            crc = crc32(crc, chunk);
            relocate(image, base, chunk, &mut next).map_err(|_| E_BAD)?;
            left -= chunk.len();
        }
        if crc != header.body_crc { Err(E_BAD) } else { Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{WP2, WP3};
    use crate::pkg::{KIND_APP, KIND_LOADER};

    fn header(kind: u8, model: u8) -> [u8; HEADER_SIZE] {
        let mut h = [0u8; HEADER_SIZE];
        h[6] = kind;
        h[MODEL_BYTE] = model;
        h
    }

    #[test]
    fn civil_dates() {
        let t = civil(0);
        assert_eq!((t.year, t.month, t.day, t.weekday), (1970, 1, 1, 4));
        // 2026-11-25 04:30:15, a Wednesday
        let t = civil(1_795_581_015);
        assert_eq!((t.year, t.month, t.day, t.hour, t.minute, t.second, t.weekday), (2026, 11, 25, 4, 30, 15, 3));
        // leap day
        let t = civil(951_782_400);
        assert_eq!((t.year, t.month, t.day), (2000, 2, 29));
    }

    #[test]
    fn model_byte() {
        assert!(loader_model_ok(&WP3, &header(KIND_LOADER, 2)).is_some_and(|h| h[MODEL_BYTE] == 0));
        assert!(loader_model_ok(&WP3, &header(KIND_LOADER, 0)).is_none(), "a WP2 loader on WP3");
        assert!(loader_model_ok(&WP3, &header(KIND_APP, 0)).is_some(), "apps are model-free");
        assert!(loader_model_ok(&WP3, &header(KIND_APP, 2)).is_none());
        assert!(loader_model_ok(&WP2, &header(KIND_LOADER, 0)).is_some());
        assert!(loader_model_ok(&WP2, &header(KIND_LOADER, 2)).is_none());
        // a WP3 loader on a WP2 stub: the reserved byte fails parse_header
        let mut raw = header(KIND_LOADER, 2);
        raw[..4].copy_from_slice(b"WP2A");
        assert_eq!(crate::pkg::parse_header(&raw), Err(crate::pkg::PkgError::Magic));
    }
}
