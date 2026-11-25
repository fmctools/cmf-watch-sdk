// Zephyr fs calls of the Watch 3 Pro image (model::WP3), as wp2-core fs.rs does for the WP2 image. Kept here, not
// made generic in wp2-core, so the WP2 stub and loader stay byte-identical; paths must be NUL-terminated.
use core::mem::transmute;
use wp2_core::layout::E_BAD;
use wp2_core::model::WP3 as M;

pub type File = [u8; M.fs_file as usize];
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
    *file = [0; M.fs_file as usize];
    let f: extern "C" fn(*mut File, *const u8, u8) -> i32 = unsafe { transmute(M.fs_open as usize) };
    f(file, path.as_ptr(), flags)
}

pub fn read(file: &mut File, buf: &mut [u8]) -> i32 {
    let f: extern "C" fn(*mut File, *mut u8, usize) -> i32 = unsafe { transmute(M.fs_read as usize) };
    f(file, buf.as_mut_ptr(), buf.len())
}

pub fn write(file: &mut File, buf: &[u8]) -> i32 {
    let f: extern "C" fn(*mut File, *const u8, usize) -> i32 = unsafe { transmute(M.fs_write as usize) };
    f(file, buf.as_ptr(), buf.len())
}

// from the start of the file (FS_SEEK_SET)
pub fn seek(file: &mut File, offset: u32) -> i32 {
    let f: extern "C" fn(*mut File, i32, i32) -> i32 = unsafe { transmute(M.fs_seek as usize) };
    f(file, offset as i32, 0)
}

pub fn close(file: &mut File) -> i32 {
    let f: extern "C" fn(*mut File) -> i32 = unsafe { transmute(M.fs_close as usize) };
    f(file)
}

pub fn unlink(path: &[u8]) -> i32 {
    if !terminated(path) {
        return E_BAD;
    }
    let f: extern "C" fn(*const u8) -> i32 = unsafe { transmute(M.fs_unlink as usize) };
    f(path.as_ptr())
}

pub fn rename(from: &[u8], to: &[u8]) -> i32 {
    if !terminated(from) || !terminated(to) {
        return E_BAD;
    }
    let f: extern "C" fn(*const u8, *const u8) -> i32 = unsafe { transmute(M.fs_rename as usize) };
    f(from.as_ptr(), to.as_ptr())
}

// No fs_mkdir wrapper: find the mount point and call its file system's mkdir (vtable `fs_ops_mkdir`, +0x40 here).
pub fn mkdir(path: &[u8]) -> i32 {
    if !terminated(path) {
        return E_BAD;
    }
    let find: extern "C" fn(*mut u32, *const u8, *mut usize) -> i32 = unsafe { transmute(M.fs_find_mount as usize) };
    let mut mp = 0u32;
    let rc = find(&mut mp, path.as_ptr(), core::ptr::null_mut());
    if rc < 0 {
        return rc;
    }
    let ops = unsafe { *((mp + M.fs_mount_ops) as *const u32) };
    let mkdir = if ops == 0 { 0 } else { unsafe { *((ops + M.fs_ops_mkdir) as *const u32) } };
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
