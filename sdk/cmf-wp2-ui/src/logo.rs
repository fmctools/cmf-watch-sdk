//! The app's own icon, small, beside a title in the body font: every app's top looks the same (WhatsApp, Mail,
//! Recordings, History...). `load` takes the icon file once at start (`cmf_wp2_app::app_icon!`'s
//! `app_icon_bytes()`), before an app uses that memory as RAM; `title` draws it.

use crate::color::{Rgb, BLACK};
use crate::draw::disc;
use crate::{font, Surface, C};

/// The icon's side on screen, px.
pub const SIDE: i32 = 36;
const N: usize = (SIDE * SIDE) as usize;

static mut PX: [u16; N] = [0; N];
/// the small icon's pixels (RGB565 LE), null: none
static mut AT: *const u8 = core::ptr::null();

/// The icon file (16 B descriptor, 96 × 96 RGB565 LE + A8) → SIDE × SIDE on black, each pixel the mean of its
/// area. A file that is not one leaves the fallback disc.
pub fn load(icon: &[u8]) {
    let out = unsafe { &mut *core::ptr::addr_of_mut!(PX) };
    let ok = shrink(icon, out);
    unsafe { *core::ptr::addr_of_mut!(AT) = if ok { out.as_ptr().cast() } else { core::ptr::null() } };
}

/// As `load`, the small icon kept in the icon's own first bytes (no memory of its own: for apps that do not use
/// the icon's memory as RAM). Safe in place: each pixel's area lies after every byte written before it.
pub fn load_in_place(icon: &'static mut [u8]) {
    let ok = icon.len() >= 16 + 96 * 96 * 3;
    if ok {
        let p = icon.as_mut_ptr();
        shrink_with(|i| unsafe { *p.add(16 + i) }, |o, c| unsafe {
            *p.add(2 * o) = c as u8;
            *p.add(2 * o + 1) = (c >> 8) as u8;
        });
    }
    unsafe { *core::ptr::addr_of_mut!(AT) = if ok { icon.as_ptr() } else { core::ptr::null() } };
}

fn shrink(icon: &[u8], out: &mut [u16; N]) -> bool {
    let px = icon.get(16..).unwrap_or(&[]);
    if px.len() < 96 * 96 * 3 {
        return false;
    }
    shrink_with(|i| px[i], |o, c| out[o] = c);
    true
}

/// `get`: the 96 × 96 pixels' bytes (3 each); `put`: the small icon's pixel `o`, in order.
fn shrink_with(get: impl Fn(usize) -> u8, mut put: impl FnMut(usize, u16)) {
    let span = |k: i32| (k * 96 / SIDE, ((k + 1) * 96 / SIDE).max(k * 96 / SIDE + 1));
    for y in 0..SIDE {
        let (y0, y1) = span(y);
        for x in 0..SIDE {
            let (x0, x1) = span(x);
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = ((sy * 96 + sx) * 3) as usize;
                    let c = u32::from(u16::from_le_bytes([get(i), get(i + 1)]));
                    let a = u32::from(get(i + 2));
                    r += (c >> 11 & 31) * a;
                    g += (c >> 5 & 63) * a;
                    b += (c & 31) * a;
                    n += 255;
                }
            }
            put((y * SIDE + x) as usize, ((r / n) << 11 | (g / n) << 5 | b / n) as u16);
        }
    }
}

/// The icon with its top left at (x, y); without one, a disc of `fallback`.
pub fn draw(s: &mut impl Surface, x: i32, y: i32, fallback: Rgb) {
    let at = unsafe { *core::ptr::addr_of!(AT) };
    if at.is_null() {
        return disc(s, x + SIDE / 2, y + SIDE / 2, SIDE / 2, fallback, BLACK);
    }
    let px = unsafe { core::slice::from_raw_parts(at, 2 * N) };
    let at = |i: usize| u16::from_le_bytes([px[2 * i], px[2 * i + 1]]);
    for row in 0..SIDE as usize {
        // runs of one colour as one rect
        let line = row * SIDE as usize;
        let mut k = 0;
        while k < SIDE as usize {
            let mut e = k + 1;
            while e < SIDE as usize && at(line + e) == at(line + k) {
                e += 1;
            }
            s.rect(x + k as i32, y + row as i32, (e - k) as i32, 1, at(line + k));
            k = e;
        }
    }
}

/// The icon and `text` (white), centred on the panel, the icon's top at `y`.
pub fn title(s: &mut impl Surface, y: i32, text: &str, fallback: Rgb) {
    let (t, _) = font::clip(text, 240);
    let w = SIDE + 10 + font::width(t);
    let x = C - w / 2;
    draw(s, x, y, fallback);
    font::draw(s, x + SIDE + 10, y + (SIDE - font::HEIGHT) / 2, t, 0xFFFF);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinks_with_alpha_on_black() {
        // white, opaque on the left half, see-through on the right
        let mut icon = std::vec![0u8; 16 + 96 * 96 * 3];
        for y in 0..96 {
            for x in 0..96 {
                let i = 16 + (y * 96 + x) * 3;
                icon[i] = 0xFF;
                icon[i + 1] = 0xFF;
                icon[i + 2] = if x < 48 { 255 } else { 0 };
            }
        }
        let mut out = [0u16; N];
        assert!(shrink(&icon, &mut out));
        assert_eq!(out[0], 0xFFFF);
        assert_eq!(out[SIDE as usize - 1], 0);
        assert!(!shrink(&icon[..100], &mut out));
        // in place: the same pixels
        let ram: &'static mut [u8] = std::boxed::Box::leak(icon.into_boxed_slice());
        load_in_place(ram);
        let at = unsafe { *core::ptr::addr_of!(AT) };
        let got = unsafe { core::slice::from_raw_parts(at, 2 * N) };
        for (k, &c) in out.iter().enumerate() {
            assert_eq!(u16::from_le_bytes([got[2 * k], got[2 * k + 1]]), c, "pixel {k}");
        }
    }
}
