// Data sources of Watch 3 Pro faces that WP2's dispatcher 0x100DAA90 lacks (its table ends at 0x8D; docs/recon.md
// "Watch 3 Pro faces"): 0x99, the colour theme of the active face, picks an image of the face by the theme's number.
use core::mem::transmute;

const THEME: u32 = 0x99;
// (index in the face list, *out) -> 0, or < 0 when the face has no theme of its own
const FACE_THEME: u32 = 0x100D_D56D;
// WfmPara +5: the active entry's index
const ACTIVE: u32 = 0x3801_AE55;

// Replaces `mvn r0, #0xf` at 0x100DAACC, the dispatcher's branch for a source past its table: the source is in r5,
// out in r1, *digits in r4. The frame is the dispatcher's {r0, r1, r2, r4, r5, lr}.
#[unsafe(naked)]
pub unsafe extern "C" fn wp2_source() {
    core::arch::naked_asm!("mov r0, r5", "mov r2, r4", "bl {f}", "add sp, #0xc", "pop {{r4, r5, pc}}", f = sym source)
}

extern "C" fn source(id: u32, out: *mut u32, digits: *mut u16) -> i32 {
    if id != THEME {
        return -0x10;
    }
    let mut theme = 0u8;
    unsafe {
        let get = transmute::<usize, extern "C" fn(u32, *mut u8) -> i32>(FACE_THEME as usize);
        if get(u32::from((ACTIVE as usize as *const u8).read_volatile()), &mut theme) != 0 {
            theme = 0;
        }
        out.write_unaligned(u32::from(theme));
        if !digits.is_null() {
            digits.write_unaligned(0);
        }
    }
    0
}
