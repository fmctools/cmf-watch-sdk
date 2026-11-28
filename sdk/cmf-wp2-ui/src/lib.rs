#![cfg_attr(not(test), no_std)]
//! UI kit for cmf-watch-sdk apps. Everything draws through [`Surface`] (`rect` + `text`), so the same code runs on the
//! watch ([`cmf_wp2_app::Api`]), in host tests ([`Fb`]) and in the simulator. Fixed point only: the target has no FPU.

pub mod ask;
pub mod color;
pub mod draw;
pub mod font;
mod inter;
pub mod geom;
pub mod haptic;
pub mod logo;
pub mod motion;
pub mod text;
pub mod widget;

pub use color::Rgb;

pub const SIZE: i32 = 466;
/// Screen centre (x and y) and the radius of the round panel.
pub const C: i32 = SIZE / 2;
/// Text cell: the loader's 8×8 font at 2×.
pub const CHAR: i32 = 16;
/// Fixed-point one for ratios, coverage and easing.
pub const ONE: i32 = 1024;

pub trait Surface {
    /// Opaque fill; `w`/`h` <= 0 draws nothing.
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16);
    /// One [`CHAR`] cell per char, transparent background.
    fn text(&mut self, x: i32, y: i32, s: &str, c: u16);
}

impl Surface for &cmf_wp2_app::Api {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        if w > 0 && h > 0 {
            cmf_wp2_app::Api::rect(self, x as i16, y as i16, w.min(u16::MAX as i32) as u16, h.min(u16::MAX as i32) as u16, c);
        }
    }

    fn text(&mut self, x: i32, y: i32, s: &str, c: u16) {
        cmf_wp2_app::Api::text(self, x as i16, y as i16, s, c);
    }
}

/// A 466×466 RGB565 buffer that draws like the loader (same clipping, same font). `rects` counts rect calls.
pub struct Fb<'a> {
    pub px: &'a mut [u16],
    pub rects: u32,
}

impl<'a> Fb<'a> {
    pub fn new(px: &'a mut [u16]) -> Self {
        Fb { px, rects: 0 }
    }

    pub fn at(&self, x: i32, y: i32) -> u16 {
        self.px[(y * SIZE + x) as usize]
    }
}

impl Surface for Fb<'_> {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        if w > 0 && h > 0 {
            self.rects += 1;
            let (x0, y0) = (x.max(0), y.max(0));
            let (x1, y1) = ((x + w).min(SIZE), (y + h).min(SIZE));
            for row in y0..y1 {
                if x0 < x1 {
                    self.px[(row * SIZE + x0) as usize..(row * SIZE + x1) as usize].fill(c);
                }
            }
        }
    }

    fn text(&mut self, x: i32, y: i32, s: &str, c: u16) {
        wp2_core::gfx::Canvas { px: self.px }.text(x as i16, y as i16, s.as_bytes(), c);
    }
}

/// Counts what a draw would cost, draws nothing.
#[derive(Default)]
pub struct Count {
    pub rects: u32,
    pub px: u64,
}

impl Surface for Count {
    fn rect(&mut self, _: i32, _: i32, w: i32, h: i32, _: u16) {
        if w > 0 && h > 0 {
            self.rects += 1;
            self.px += (w * h) as u64;
        }
    }

    fn text(&mut self, _: i32, _: i32, _: &str, _: u16) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{BLACK, Rgb, WHITE};
    use crate::draw::*;

    // `cargo test -p cmf-wp2-ui rect_counts -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn rect_counts() {
        const GREY: Rgb = Rgb(40, 40, 50);
        let cases: [(&str, fn(&mut Count)); 9] = [
            ("disc r 20", |s| disc(s, C, C, 20, WHITE, BLACK)),
            ("disc r 8", |s| disc(s, C, C, 8, WHITE, BLACK)),
            ("ring r 24..36", |s| ring(s, C, C, 24, 36, WHITE, BLACK)),
            ("arc r 130..150, full with track", |s| arc(s, C, C, 130, 150, 49152, 65536, WHITE, Some(GREY), BLACK)),
            ("arc r 130..150, 1/3 with track", |s| arc(s, C, C, 130, 150, 49152, 21845, WHITE, Some(GREY), BLACK)),
            ("line 200 px diagonal w 5", |s| line(s, 100, 100, 241, 241, 5, WHITE, BLACK)),
            ("pill 120x40", |s| rrect(s, 100, 100, 120, 40, 20, WHITE, BLACK)),
            ("gradient 246x36", |s| gradient(s, 100, 100, 246, 36, WHITE, BLACK, false)),
            ("big \"12:30\" x6", |s| text::big(s, 100, 100, "12:30", 6, 0xFFFF)),
        ];
        for (name, f) in cases {
            let mut c = Count::default();
            f(&mut c);
            std::println!("{name}: {} rects, {} px", c.rects, c.px);
        }
    }
}
