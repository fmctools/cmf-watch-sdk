//! The watch's own emoji: the stock font `/NAND:C/emoji28.font` the notifications draw with, read in place by the
//! loader (`Api::emoji_read`, nothing copied into the app): 52 emoji, 30 × 30 px ARGB6666 (3 bytes a pixel, A R G B
//! 6 bits each from the top of a little-endian u24). Once [`init`] ran, [`crate::font`] draws emoji code points in
//! text at [`SIZE`] px; without it (or on a loader before 6) they stay boxes.
//!
//! File: u32 0, u32 count, 8 bytes, then per emoji 16 bytes (`cp:u32 w:u16 h:u16 offset:u32 u16 u16`), ascending
//! code points. A picture is made smaller on first use and kept in one of a few slots of memory the app gives
//! ([`ram_for`]); the font has no seek, so emoji not kept are read again when drawn.

use cmf_wp2_app::Api;
use core::ptr::{addr_of_mut, null_mut};

/// An emoji's size in text, px.
pub const SIZE: i32 = 22;
const SRC: usize = 30;
const MAX: usize = 64;
const MAX_SLOTS: usize = 24;
const PX: usize = (SIZE * SIZE) as usize;
const RAW: usize = SRC * SRC * 3;
/// A pixel not drawn.
const CLEAR: u16 = 0xF81F;

/// Bytes of memory [`init`] needs to keep `slots` emoji at once (a keyboard page shows 18; 8 is enough for text).
pub const fn ram_for(slots: usize) -> usize {
    RAW + 1 + slots * PX * 2
}

struct Font {
    api: Option<&'static Api>,
    n: usize,
    cp: [u32; MAX],
    off: [u32; MAX],
    /// what each slot holds (0 none) and when it was last drawn
    slot_cp: [u32; MAX_SLOTS],
    used: [u32; MAX_SLOTS],
    tick: u32,
    slots: usize,
    raw: *mut u8,
    px: *mut u16,
}

static mut FONT: Font = Font {
    api: None,
    n: 0,
    cp: [0; MAX],
    off: [0; MAX],
    slot_cp: [0; MAX_SLOTS],
    used: [0; MAX_SLOTS],
    tick: 0,
    slots: 0,
    raw: null_mut(),
    px: null_mut(),
};

fn font() -> &'static mut Font {
    // one app thread
    unsafe { &mut *addr_of_mut!(FONT) }
}

/// Reads the font's table and has [`crate::font`] draw emoji with it. `ram` keeps the pictures: a `static mut`, or
/// the icon's memory once the app runs (`app_icon_ram()`, as Chats does). Returns how many emoji the watch has: 0
/// on a loader before 6, or when `ram` is smaller than [`ram_for`]`(1)`.
pub fn init(api: &'static Api, ram: &'static mut [u8]) -> usize {
    let f = font();
    f.api = Some(api);
    f.n = 0;
    f.slot_cp = [0; MAX_SLOTS];
    f.used = [0; MAX_SLOTS];
    f.slots = 0;
    f.raw = null_mut();
    f.px = null_mut();
    if ram.len() < ram_for(1) {
        return 0;
    }
    let base = ram.as_mut_ptr();
    let px = unsafe { base.add(RAW) };
    let px = unsafe { px.add(px.align_offset(2)) } as *mut u16;
    let room = ram.len() - (px as usize - base as usize);
    f.slots = (room / (PX * 2)).min(MAX_SLOTS);
    f.raw = base;
    f.px = px;
    let mut head = [0u8; 16 + MAX * 16];
    let got = api.emoji_read(0, &mut head);
    if got < 16 {
        return 0;
    }
    let u32_at = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let n = (u32_at(&head, 4) as usize).min(MAX).min((got as usize - 16) / 16);
    for i in 0..n {
        let e = &head[16 + i * 16..32 + i * 16];
        if u16::from_le_bytes([e[4], e[5]]) as usize != SRC || u16::from_le_bytes([e[6], e[7]]) as usize != SRC {
            continue;
        }
        // U+1F53E is an empty frame in the stock font: not offered
        if u32_at(e, 0) == 0x1F53E {
            continue;
        }
        f.cp[f.n] = u32_at(e, 0);
        f.off[f.n] = u32_at(e, 8);
        f.n += 1;
    }
    if f.n > 0 {
        crate::font::set_emoji(draw);
    }
    f.n
}

/// The emoji the watch has, in code point order (empty before [`init`]).
pub fn all() -> &'static [u32] {
    let f = font();
    &f.cp[..f.n]
}

/// The emoji's slot, read and made smaller if it isn't kept.
fn slot(cp: u32) -> Option<usize> {
    let f = font();
    let i = f.cp[..f.n].binary_search(&cp).ok()?;
    f.tick = f.tick.wrapping_add(1);
    if let Some(s) = f.slot_cp[..f.slots].iter().position(|&c| c == cp) {
        f.used[s] = f.tick;
        return Some(s);
    }
    // an empty slot, else the one drawn longest ago
    let s = (0..f.slots).min_by_key(|&s| if f.slot_cp[s] == 0 { 0 } else { f.used[s].max(1) })?;
    let raw = unsafe { core::slice::from_raw_parts_mut(f.raw, RAW) };
    if f.api?.emoji_read(f.off[i] as usize, raw) != RAW as i32 {
        return None;
    }
    shrink(raw, pixels(s));
    f.slot_cp[s] = cp;
    f.used[s] = f.tick;
    Some(s)
}

fn pixels(s: usize) -> &'static mut [u16] {
    unsafe { core::slice::from_raw_parts_mut(font().px.add(s * PX), PX) }
}

/// 30 × 30 ARGB6666 → SIZE × SIZE RGB565, each pixel the mean of the source pixels under it (by alpha), edges
/// blended onto a dark ground, the faint ones left out.
fn shrink(raw: &[u8], out: &mut [u16]) {
    let n = SIZE as usize;
    for dy in 0..n {
        let (y0, y1) = (dy * SRC / n, ((dy + 1) * SRC).div_ceil(n));
        for dx in 0..n {
            let (x0, x1) = (dx * SRC / n, ((dx + 1) * SRC).div_ceil(n));
            let (mut a, mut r, mut g, mut b, mut k) = (0u32, 0u32, 0u32, 0u32, 0u32);
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = (y * SRC + x) * 3;
                    let v = u32::from(raw[p]) | u32::from(raw[p + 1]) << 8 | u32::from(raw[p + 2]) << 16;
                    let pa = (v >> 18) & 63;
                    a += pa;
                    r += ((v >> 12) & 63) * pa;
                    g += ((v >> 6) & 63) * pa;
                    b += (v & 63) * pa;
                    k += 1;
                }
            }
            out[dy * n + dx] = if a == 0 || a * 4 < k * 63 {
                CLEAR
            } else {
                // mean colour (0..63), then over the ground by the mean alpha
                let (r, g, b) = (r / a, g / a, b / a);
                let al = a / k;
                let mix = |c: u32| (c * al + 4 * (63 - al)) / 63;
                let (r, g, b) = (mix(r), mix(g), mix(b));
                ((r >> 1) << 11 | g << 5 | (b >> 1)) as u16
            };
        }
    }
}

/// [`crate::font`]'s emoji hook: the picture as runs of one colour.
fn draw(cp: u32, x: i32, y: i32, rect: &mut dyn FnMut(i32, i32, i32, i32, u16)) -> bool {
    draw_scaled(cp, x, y, 1, rect)
}

/// The emoji `scale` times as big (the emoji keyboard); false when the watch doesn't have it.
pub fn draw_scaled(cp: u32, x: i32, y: i32, scale: i32, rect: &mut dyn FnMut(i32, i32, i32, i32, u16)) -> bool {
    let Some(s) = slot(cp) else { return false };
    let px = pixels(s);
    let n = SIZE as usize;
    for row in 0..n {
        let line = &px[row * n..row * n + n];
        let mut i = 0;
        while i < n {
            let c = line[i];
            let mut j = i + 1;
            while j < n && line[j] == c {
                j += 1;
            }
            if c != CLEAR {
                rect(x + i as i32 * scale, y + row as i32 * scale, (j - i) as i32 * scale, scale, c);
            }
            i = j;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argb(a: u32, r: u32, g: u32, b: u32) -> [u8; 3] {
        let v = a << 18 | r << 12 | g << 6 | b;
        [v as u8, (v >> 8) as u8, (v >> 16) as u8]
    }

    #[test]
    fn shrink_keeps_opaque_colour_and_drops_clear() {
        let mut raw = [0u8; RAW];
        // left half opaque red, right half clear
        for y in 0..SRC {
            for x in 0..SRC / 2 {
                raw[(y * SRC + x) * 3..(y * SRC + x) * 3 + 3].copy_from_slice(&argb(63, 63, 0, 0));
            }
        }
        let mut out = [0u16; PX];
        shrink(&raw, &mut out);
        assert_eq!(out[0], 0xF800, "opaque red stays red");
        assert_eq!(out[PX - 1], CLEAR, "clear stays undrawn");
    }

    #[test]
    fn ram_for_fits_its_slots() {
        // the page of 18 a keyboard shows fits in what ram_for(18) asks for, whatever the alignment
        assert!(ram_for(18) - RAW - 1 >= 18 * PX * 2);
        assert!(ram_for(1) < 4096);
    }
}
