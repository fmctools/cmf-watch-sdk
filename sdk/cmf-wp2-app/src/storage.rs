//! Read-only storage and installed-app metadata. See `docs/storage-inspection.md` for the wire contract.
//! Calls run synchronously on the app thread. An older loader returns `UNSUPPORTED` at `open`.
use crate::Api;
use core::sync::atomic::{AtomicBool, Ordering};
pub const UNSUPPORTED: i32 = -95;
pub const MAX_FILES: usize = 96;
pub const PATH_BYTES: usize = 96;
static BUSY: AtomicBool = AtomicBool::new(false);
static mut REQUEST: [u8; 132] = [0; 132];

pub struct Storage<'a>(&'a Api);
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub version: u16,
    pub app_slots: u8,
    pub max_partitions: u8,
    pub max_files: u16,
    pub max_directories: u16,
    pub path_bytes: u16,
    /// bit 0: 64px, bit 1: native 96px installed-app icons.
    pub icon_sizes: u8,
}
#[derive(Clone, Copy, Default, Debug)]
pub struct Partition {
    pub name: [u8; 8],
    pub kind: u8,
    pub file_id: u8,
    pub device: u8,
    pub mirror: u8,
    pub offset: u32,
    pub allocated: u32,
}
impl Partition {
    pub fn name(&self) -> &str {
        text(&self.name)
    }
}
#[derive(Clone, Copy, Default, Debug)]
pub struct Filesystem {
    /// FAT data-cluster capacity; excludes FAT metadata and firmware reservations.
    pub total: u32,
    pub free: u32,
    /// Logical file lengths, not cluster-chain allocations. These are subsets of used capacity.
    pub apps: u32,
    pub app_data: u32,
    pub widgets: u32,
    pub loader: u32,
    pub installed_apps: u32,
    pub allocation_unit: u32,
}
#[derive(Clone, Copy, Default, Debug)]
pub struct InstalledApp {
    pub slot: u8,
    pub name: [u8; 24],
    pub version: u32,
    pub has_icon: bool,
}
impl InstalledApp {
    pub fn name(&self) -> &str {
        text(&self.name)
    }
}
#[derive(Clone, Copy, Debug)]
pub struct File {
    pub path: [u8; PATH_BYTES],
    pub size: u32,
}
impl File {
    pub fn path(&self) -> &str {
        text(&self.path)
    }
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct ScanProgress {
    pub files: u16,
    pub complete: bool,
    pub partial: bool,
    /// Recorded filesystem error, or 0. A scan can still be partial because a limit was reached.
    pub error: i32,
}
fn text(bytes: &[u8]) -> &str {
    let n = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    core::str::from_utf8(&bytes[..n]).unwrap_or("Invalid UTF-8")
}
fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
impl<'a> Storage<'a> {
    pub fn open(api: &'a Api) -> Result<Self, i32> {
        if api.abi_minor < 2 {
            return Err(UNSUPPORTED);
        }
        let s = Self(api);
        match s.capabilities() {
            Ok(c) if c.version == 1 => Ok(s),
            Ok(_) | Err(-22) | Err(UNSUPPORTED) => Err(UNSUPPORTED),
            Err(e) => Err(e),
        }
    }
    // The buffer must live in the app image; the loader rejects arbitrary stack/host pointers.
    fn call(&self, op: u8, index: u16) -> Result<[u8; 124], i32> {
        if BUSY.swap(true, Ordering::Acquire) {
            return Err(-16);
        }
        let p = core::ptr::addr_of_mut!(REQUEST);
        unsafe {
            p.write([0; 132]);
            let b = &mut *p;
            b[..4].copy_from_slice(&[0x19, 128, 0, 0]);
            b[4] = op;
            b[5..7].copy_from_slice(&index.to_le_bytes());
        }
        let rc = (self.0.lv_submit)(p.cast(), 132);
        let mut out = [0u8; 124];
        if rc >= 0 {
            unsafe {
                out.copy_from_slice(&(&*p)[8..]);
            }
        }
        BUSY.store(false, Ordering::Release);
        if rc < 0 { Err(rc) } else { Ok(out) }
    }
    pub fn capabilities(&self) -> Result<Capabilities, i32> {
        let b = self.call(0, 0)?;
        Ok(Capabilities {
            version: u16_at(&b, 0),
            app_slots: b[2],
            max_partitions: b[3],
            max_files: u16_at(&b, 4),
            max_directories: u16_at(&b, 6),
            path_bytes: u16_at(&b, 8),
            icon_sizes: b[10],
        })
    }
    pub fn partition_count(&self) -> Result<u8, i32> {
        Ok(self.call(2, 0)?[0])
    }
    pub fn partition(&self, index: u8) -> Result<Partition, i32> {
        let b = self.call(3, index as u16)?;
        let mut name = [0; 8];
        name.copy_from_slice(&b[..8]);
        Ok(Partition {
            name,
            kind: b[8],
            file_id: b[9],
            device: b[10],
            mirror: b[11],
            offset: u32_at(&b, 12),
            allocated: u32_at(&b, 16),
        })
    }
    /// Refresh capacity once, on demand. Other writers can change storage during a query or scan.
    pub fn filesystem(&self) -> Result<Filesystem, i32> {
        let b = self.call(1, 0)?;
        Ok(Filesystem {
            total: u32_at(&b, 0),
            free: u32_at(&b, 4),
            apps: u32_at(&b, 8),
            app_data: u32_at(&b, 12),
            widgets: u32_at(&b, 16),
            loader: u32_at(&b, 20),
            installed_apps: u32_at(&b, 24),
            allocation_unit: u32_at(&b, 28),
        })
    }
    /// Slot identity is stable across menu reordering. An empty or invalid package returns None.
    pub fn installed_app(&self, slot: u8) -> Result<Option<InstalledApp>, i32> {
        let b = self.call(4, slot as u16)?;
        if b[0] == 0 {
            return Ok(None);
        }
        let mut name = [0; 24];
        name.copy_from_slice(&b[4..28]);
        Ok(Some(InstalledApp {
            slot: b[2],
            name,
            version: u32_at(&b, 28),
            has_icon: b[1] != 0,
        }))
    }
    /// Start/restart the running app's single bounded recursive scan of /NAND:.
    /// Starting again invalidates previous file indices. Cancel on normal exit; the loader also cleans up on stop.
    pub fn begin_scan(&self) -> Result<(), i32> {
        self.call(5, 0).map(|_| ())
    }
    /// Process at most one directory entry. Call between events; do not poll in a busy loop.
    pub fn scan_step(&self) -> Result<ScanProgress, i32> {
        let b = self.call(6, 0)?;
        Ok(ScanProgress {
            files: u16_at(&b, 0),
            complete: b[2] != 0,
            partial: b[3] != 0,
            error: u32_at(&b, 4) as i32,
        })
    }
    pub fn cancel_scan(&self) -> Result<(), i32> {
        self.call(7, 0).map(|_| ())
    }
    /// Read an entry already visited by the active scan. Paths are bounded, NUL-terminated and never truncated silently.
    pub fn file(&self, index: u16) -> Result<File, i32> {
        let b = self.call(8, index)?;
        let mut path = [0; PATH_BYTES];
        path.copy_from_slice(&b[4..100]);
        Ok(File {
            path,
            size: u32_at(&b, 0),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicI32, Ordering};
    static MODE: AtomicI32 = AtomicI32::new(0);
    extern "C" fn reply(p: *const u8, n: usize) -> i32 {
        let mode = MODE.load(Ordering::Relaxed);
        if mode < 0 {
            return mode;
        }
        assert_eq!(n, 132);
        let b = unsafe { core::slice::from_raw_parts_mut(p as *mut u8, n) };
        assert_eq!(&b[..4], &[0x19, 128, 0, 0]);
        let op = b[4];
        let index = u16_at(b, 5);
        let out = &mut b[8..];
        fn put(b: &mut [u8], at: usize, v: u32) {
            b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        match op {
            0 => {
                out[0] = 1;
                out[2] = 16;
                out[3] = 30;
                out[4] = 96;
                out[6] = 16;
                out[8] = 96;
                out[10] = 3;
            }
            1 => {
                put(out, 0, 0xf0000000);
                put(out, 4, 0x80000000);
                put(out, 24, 16);
                put(out, 28, 2048);
            }
            2 => out[0] = 22,
            3 => {
                out[..4].copy_from_slice(b"test");
                out[8] = 2;
                out[9] = 40;
                out[10] = 2;
                out[11] = 1;
                put(out, 12, 0xff000000);
                put(out, 16, 4096);
            }
            4 => {
                if index < 16 {
                    out[0] = 1;
                    out[1] = 1;
                    out[2] = index as u8;
                    out[4..8].copy_from_slice(b"Test");
                    put(out, 28, u32::MAX);
                } else {
                    return -22;
                }
            }
            6 => {
                out[..2].copy_from_slice(&96u16.to_le_bytes());
                out[2] = 1;
                out[3] = 1;
                put(out, 4, (-5i32) as u32);
            }
            8 => {
                put(out, 0, u32::MAX);
                out[4..14].copy_from_slice(b"/NAND:/a.x");
            }
            _ => {}
        }
        0
    }
    #[test]
    fn typed_reads_unsigned_data_errors_limits_and_unsupported_loader() {
        let api = crate::tests::table(crate::ABI, 3);
        api.lv_submit = reply;
        MODE.store(-22, Ordering::Relaxed);
        assert!(matches!(Storage::open(api), Err(UNSUPPORTED)));
        MODE.store(0, Ordering::Relaxed);
        let s = Storage::open(api).unwrap();
        assert_eq!(s.capabilities().unwrap().app_slots, 16);
        let f = s.filesystem().unwrap();
        assert_eq!(
            (f.total, f.free, f.installed_apps),
            (0xf0000000, 0x80000000, 16)
        );
        let p = s.partition(21).unwrap();
        assert_eq!(
            (p.name(), p.offset, p.device, p.mirror),
            ("test", 0xff000000, 2, 1)
        );
        assert_eq!(s.partition_count(), Ok(22));
        let a = s.installed_app(15).unwrap().unwrap();
        assert_eq!(
            (a.slot, a.name(), a.version, a.has_icon),
            (15, "Test", u32::MAX, true)
        );
        assert!(matches!(s.installed_app(16), Err(-22)));
        s.begin_scan().unwrap();
        assert_eq!(
            s.scan_step().unwrap(),
            ScanProgress {
                files: 96,
                complete: true,
                partial: true,
                error: -5
            }
        );
        let f = s.file(95).unwrap();
        assert_eq!((f.path(), f.size), ("/NAND:/a.x", u32::MAX));
        s.cancel_scan().unwrap();
        MODE.store(-5, Ordering::Relaxed);
        assert!(matches!(s.filesystem(), Err(-5)));
        MODE.store(0, Ordering::Relaxed);
        assert!(s.filesystem().is_ok(), "lock released on error");
        BUSY.store(true, Ordering::Release);
        assert!(matches!(s.filesystem(), Err(-16)));
        BUSY.store(false, Ordering::Release);
        let api = crate::tests::table(crate::ABI, 1);
        assert!(matches!(Storage::open(api), Err(UNSUPPORTED)));
    }
}
