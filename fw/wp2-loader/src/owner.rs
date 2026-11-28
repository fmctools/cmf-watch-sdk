// An app's named files (FILE: `/NAND:/wp2apps/<d>_<name>`, svc::file) belong to the app that wrote them, not to the
// slot: `<d>.own` holds that app's package name. The stub's DELETE and END clear `.dat`, `.wgt` and the start blob
// but know nothing of named files, so the loader settles it:
// - `adopt` when an app starts (svc::start) and for each app the menu lists: no marker → this app's (files kept, as
//   for files written before markers); the same name → kept (an update of the same app); another name → the old
//   app's files deleted, the marker rewritten.
// - `sweep` when the menu lists the apps: a marker in a slot with no app → its files and the marker deleted.
use wp2_core::fs;
use wp2_core::layout::{owner_path, DIR};
use wp2_core::stock;

/// The slot's app now is `name` (its package header's name field, NUL-padded).
pub fn adopt(slot: u8, name: &[u8; 24]) {
    let path = owner_path(slot);
    let mut f: fs::File = [0; 0x2C];
    let mut was = [0u8; 24];
    if fs::open(&mut f, &path, fs::READ) >= 0 {
        let ok = fs::read_exact(&mut f, &mut was);
        fs::close(&mut f);
        if ok && &was == name {
            return;
        }
        if ok {
            delete_files(1 << slot);
        }
    }
    fs::unlink(&path);
    if fs::open(&mut f, &path, fs::WRITE | fs::CREATE) >= 0 {
        fs::write(&mut f, name);
        fs::close(&mut f);
    }
}

/// The menu found apps in the slots of `present` (bits): the named files of a slot left empty go.
pub fn sweep(present: u16, slots: u8) {
    let mut f: fs::File = [0; 0x2C];
    let mut gone = 0u16;
    for slot in 0..slots.min(16) {
        if present >> slot & 1 == 0 && fs::open(&mut f, &owner_path(slot), fs::READ) >= 0 {
            fs::close(&mut f);
            fs::unlink(&owner_path(slot));
            gone |= 1 << slot;
        }
    }
    if gone != 0 {
        delete_files(gone);
    }
}

const BATCH: usize = 4;
const NAME: usize = 24;

/// Deletes `<d>_*` for each slot d in `slots` (bits): a directory pass collects up to BATCH names, they go, again
/// till a pass finds none (no unlink while the directory is open).
fn delete_files(slots: u16) {
    type Open = extern "C" fn(*mut u32, *const u8) -> i32;
    type Read = extern "C" fn(*mut u32, *mut u32) -> i32;
    type Close = extern "C" fn(*mut u32) -> i32;
    let open: Open = unsafe { core::mem::transmute(stock::FS_OPENDIR as usize) };
    let read: Read = unsafe { core::mem::transmute(stock::FS_READDIR as usize) };
    let close: Close = unsafe { core::mem::transmute(stock::FS_CLOSEDIR as usize) };
    let dir_len = DIR.len() - 1;
    // a pass at most per file and slot's worth: never loops for good on a file that won't go
    for _ in 0..64 {
        let mut names = [[0u8; NAME]; BATCH];
        let mut n = 0;
        let mut dir = [0u32; 4];
        if open(dir.as_mut_ptr(), DIR.as_ptr()) < 0 {
            return;
        }
        let mut entry = [0u32; 66];
        while n < BATCH && read(dir.as_mut_ptr(), entry.as_mut_ptr()) >= 0 {
            let raw = unsafe { core::slice::from_raw_parts(entry.as_ptr().cast::<u8>(), 264) };
            let len = raw[1..257].iter().position(|&c| c == 0).unwrap_or(256);
            if len == 0 {
                break;
            }
            let name = &raw[1..1 + len];
            let d = match name[0] {
                c @ b'0'..=b'9' => c - b'0',
                c @ b'A'..=b'F' => c - b'A' + 10,
                _ => continue,
            };
            if len >= 3 && len < NAME && name[1] == b'_' && slots >> d & 1 != 0 {
                names[n][..len].copy_from_slice(name);
                n += 1;
            }
        }
        close(dir.as_mut_ptr());
        if n == 0 {
            return;
        }
        for name in &names[..n] {
            let len = name.iter().position(|&c| c == 0).unwrap_or(NAME);
            // "/NAND:/wp2apps/" + name + NUL
            let mut path = [0u8; 48];
            path[..dir_len].copy_from_slice(&DIR[..dir_len]);
            path[dir_len] = b'/';
            path[dir_len + 1..dir_len + 1 + len].copy_from_slice(&name[..len]);
            fs::unlink(&path);
        }
    }
}
