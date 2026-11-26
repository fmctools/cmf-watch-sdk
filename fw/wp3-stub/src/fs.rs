// The Watch 3 Pro's Zephyr fs calls (wp2-core port.rs with model::WP3), under the names wp2-core fs.rs uses.
use wp2_core::model::WP3 as M;
use wp2_core::port;
pub use wp2_core::port::{File, CREATE, READ, WRITE};

pub fn open(file: &mut File, path: &[u8], flags: u8) -> i32 {
    port::open(&M, file, path, flags)
}

pub fn read(file: &mut File, buf: &mut [u8]) -> i32 {
    port::read(&M, file, buf)
}

pub fn write(file: &mut File, buf: &[u8]) -> i32 {
    port::write(&M, file, buf)
}

pub fn seek(file: &mut File, offset: u32) -> i32 {
    port::seek(&M, file, offset)
}

pub fn close(file: &mut File) -> i32 {
    port::close(&M, file)
}

pub fn unlink(path: &[u8]) -> i32 {
    port::unlink(&M, path)
}

pub fn rename(from: &[u8], to: &[u8]) -> i32 {
    port::rename(&M, from, to)
}

pub fn mkdir(path: &[u8]) -> i32 {
    port::mkdir(&M, path)
}

pub fn read_exact(file: &mut File, buf: &mut [u8]) -> bool {
    port::read_exact(&M, file, buf)
}
