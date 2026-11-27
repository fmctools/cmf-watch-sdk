// Image format 28 of Watch 3 Pro faces (ETC2 RGBA8, wp2-core etc2.rs), which the stock's face manager does not know:
// two calls of its image loader 0x100DB750 and one of its header reader 0x101B8E28 come here (docs/recon.md "Watch 3
// Pro faces"). Such an image becomes a cf 5 one, so the size, the cache and the draw stay the stock's.
use core::mem::transmute;
use core::slice;
use wp2_core::{etc2, stock};

// (cf of the file) -> cf in RAM
const MAP_CF: u32 = 0x101B_8FD3;
// LZ4 (src, dst, packed, capacity) -> bytes or < 0
const UNPACK: u32 = 0x1018_22C1;
// (address in the face, dst, n) -> n
const FACE_READ: u32 = 0x100D_B43D;
const MEM: u32 = 2;
const ALIGN: u32 = 0x40;
const CF_ETC2: u32 = 28;
const CF_RGB565_ALPHA: u32 = 5;
// the loader's node: the image's address in the face, its header word in RAM
const NODE_ADDR: usize = 4;
const NODE_HEADER: usize = 0x18;

macro_rules! stock {
    ($addr:expr, fn($($t:ty),*) -> $r:ty) => {
        unsafe { transmute::<usize, extern "C" fn($($t),*) -> $r>($addr as usize) }
    };
}

/// Replaces `bl 0x101B8FD2` at 0x100DB7B6 (loader) and 0x101B8E5E (header reader: the format an item is drawn with).
pub extern "C" fn wp2_image_cf(cf: u32) -> u32 {
    if cf == CF_ETC2 {
        CF_RGB565_ALPHA
    } else {
        stock!(MAP_CF, fn(u32) -> u32)(cf)
    }
}

// Replaces `bl 0x101822C0` at 0x100DB9F0: the loader keeps its node in r4, `unpack` takes it as the fifth argument.
#[unsafe(naked)]
pub unsafe extern "C" fn wp2_image_unpack() {
    core::arch::naked_asm!("push {{r4, lr}}", "bl {unpack}", "pop {{r4, pc}}", unpack = sym unpack)
}

extern "C" fn unpack(src: *const u8, dst: *mut u8, packed: u32, cap: u32, node: *const u8) -> i32 {
    let lz4 = stock!(UNPACK, fn(*const u8, *mut u8, u32, u32) -> i32);
    let (addr, ram) = unsafe { (node.add(NODE_ADDR).cast::<u32>().read_unaligned(), node.add(NODE_HEADER).cast::<u32>().read_unaligned()) };
    let mut file = 0u32;
    // the node no longer has the file's format: a genuine cf 5 goes straight on
    if ram & 0x1F != CF_RGB565_ALPHA
        || stock!(FACE_READ, fn(u32, *mut u32, u32) -> i32)(addr, &mut file, 4) != 4
        || file & 0x1F != CF_ETC2
    {
        return lz4(src, dst, packed, cap);
    }
    let (w, h) = ((ram >> 10 & 0x7FF) as usize, (ram >> 21) as usize);
    let len = etc2::packed_len(w, h);
    if len == 0 || cap as usize != w * h * etc2::PIXEL {
        return -1;
    }
    let alloc = stock!(stock::UI_MEM_ALIGNED_ALLOC, fn(u32, u32, u32, *const u8) -> *mut u8);
    let blocks = alloc(MEM, ALIGN, len as u32, node);
    if blocks.is_null() {
        return -1;
    }
    let done = lz4(src, blocks, packed, len as u32) == len as i32
        && unsafe { etc2::decode(slice::from_raw_parts(blocks, len), w, h, slice::from_raw_parts_mut(dst, cap as usize)) };
    stock!(stock::UI_MEM_FREE, fn(u32, *mut u8) -> ())(MEM, blocks);
    if done {
        cap as i32
    } else {
        -1
    }
}
