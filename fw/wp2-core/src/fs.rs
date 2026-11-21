// Zephyr fs calls of the stock image. Paths must be NUL-terminated.
use crate::layout::E_BAD;
use crate::stock::{FS_CLOSE, FS_FIND_MOUNT, FS_OPEN, FS_READ, FS_RENAME, FS_SEEK, FS_UNLINK, FS_WRITE};
use core::mem::transmute;

pub type File = [u8; 0x2C];
pub const READ: u8 = 1;
pub const WRITE: u8 = 2;
pub const CREATE: u8 = 0x10;
const ENOTSUP: i32 = -134;

fn terminated(path: &[u8]) -> bool {
    path.contains(&0)
}

pub fn open(file: &mut File, path: &[u8], flags: u8) -> i32 {
    if !terminated(path) {
        return E_BAD;
    }
    *file = [0; 0x2C];
    let f: extern "C" fn(*mut File, *const u8, u8) -> i32 = unsafe { transmute(FS_OPEN as usize) };
    f(file, path.as_ptr(), flags)
}

pub fn read(file: &mut File, buf: &mut [u8]) -> i32 {
    let f: extern "C" fn(*mut File, *mut u8, usize) -> i32 = unsafe { transmute(FS_READ as usize) };
    f(file, buf.as_mut_ptr(), buf.len())
}

pub fn write(file: &mut File, buf: &[u8]) -> i32 {
    let f: extern "C" fn(*mut File, *const u8, usize) -> i32 = unsafe { transmute(FS_WRITE as usize) };
    f(file, buf.as_ptr(), buf.len())
}

// from the start of the file (FS_SEEK_SET)
pub fn seek(file: &mut File, offset: u32) -> i32 {
    let f: extern "C" fn(*mut File, i32, i32) -> i32 = unsafe { transmute(FS_SEEK as usize) };
    f(file, offset as i32, 0)
}

pub fn close(file: &mut File) -> i32 {
    let f: extern "C" fn(*mut File) -> i32 = unsafe { transmute(FS_CLOSE as usize) };
    f(file)
}

pub fn unlink(path: &[u8]) -> i32 {
    if !terminated(path) {
        return E_BAD;
    }
    let f: extern "C" fn(*const u8) -> i32 = unsafe { transmute(FS_UNLINK as usize) };
    f(path.as_ptr())
}

pub fn rename(from: &[u8], to: &[u8]) -> i32 {
    if !terminated(from) || !terminated(to) {
        return E_BAD;
    }
    let f: extern "C" fn(*const u8, *const u8) -> i32 = unsafe { transmute(FS_RENAME as usize) };
    f(from.as_ptr(), to.as_ptr())
}

// The image has no fs_mkdir wrapper: find the mount point and call its file system's mkdir (vtable +0x3C).
pub fn mkdir(path: &[u8]) -> i32 {
    if !terminated(path) {
        return E_BAD;
    }
    let find: extern "C" fn(*mut u32, *const u8, *mut usize) -> i32 = unsafe { transmute(FS_FIND_MOUNT as usize) };
    let mut mp = 0u32;
    let rc = find(&mut mp, path.as_ptr(), core::ptr::null_mut());
    if rc < 0 {
        return rc;
    }
    let ops = unsafe { *((mp + 0x1C) as *const u32) };
    let mkdir = if ops == 0 { 0 } else { unsafe { *((ops + 0x3C) as *const u32) } };
    if mkdir == 0 {
        return ENOTSUP;
    }
    let f: extern "C" fn(u32, *const u8) -> i32 = unsafe { transmute(mkdir as usize) };
    f(mp, path.as_ptr())
}

pub fn read_exact(file: &mut File, buf: &mut [u8]) -> bool {
    let mut done = 0;
    while done < buf.len() {
        let n = read(file, &mut buf[done..]);
        if n <= 0 {
            return false;
        }
        done += n as usize;
    }
    true
}
