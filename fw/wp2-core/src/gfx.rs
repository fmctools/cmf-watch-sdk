use crate::font8x8::{CYRILLIC, GLYPHS};

pub const W: usize = 466;
pub const H: usize = 466;

fn glyph(ch: char) -> &'static [u8; 8] {
    match ch {
        ' '..='~' => &GLYPHS[ch as usize - 0x20],
        '\u{410}'..='\u{44F}' => &CYRILLIC[ch as usize - 0x410],
        '\u{401}' => &CYRILLIC[64],
        '\u{451}' => &CYRILLIC[65],
        _ => &GLYPHS[usize::from(b'?' - 0x20)],
    }
}

pub struct Canvas<'a> {
    pub px: &'a mut [u16],
}

impl Canvas<'_> {
    pub fn fill_rect(&mut self, x: i16, y: i16, w: u16, h: u16, c: u16) {
        let x0 = i32::from(x).max(0);
        let y0 = i32::from(y).max(0);
        let x1 = (i32::from(x) + i32::from(w)).min(W as i32);
        let y1 = (i32::from(y) + i32::from(h)).min(H as i32);
        if x0 >= x1 {
            return;
        }
        for row in y0..y1 {
            let base = row as usize * W;
            self.px[base + x0 as usize..base + x1 as usize].fill(c);
        }
    }

    /// Copies `w * h` row-major RGB565 pixels from `src` to (x, y), clipped to the screen like `fill_rect`.
    /// `src` must hold at least `w * h` pixels (the loader checks that against the app block first); returns
    /// whether any pixel landed on screen.
    pub fn blit(&mut self, x: i16, y: i16, w: u16, h: u16, src: &[u16]) -> bool {
        let sw = usize::from(w);
        if sw == 0 || src.len() < sw * usize::from(h) {
            return false;
        }
        let (x0, y0) = (i32::from(x).max(0), i32::from(y).max(0));
        let (x1, y1) = ((i32::from(x) + i32::from(w)).min(W as i32), (i32::from(y) + i32::from(h)).min(H as i32));
        if x0 >= x1 || y0 >= y1 {
            return false;
        }
        let (dx, dy) = (i32::from(x), i32::from(y));
        for row in y0..y1 {
            let sy = (row - dy) as usize;
            let sx = (x0 - dx) as usize;
            let at = row as usize * W + x0 as usize;
            let n = (x1 - x0) as usize;
            self.px[at..at + n].copy_from_slice(&src[sy * sw + sx..sy * sw + sx + n]);
        }
        true
    }

    // UTF-8: ASCII and Cyrillic have glyphs, everything else and every invalid byte is one `?` cell.
    pub fn text(&mut self, x: i16, y: i16, s: &[u8], c: u16) -> i16 {
        let mut cx = i32::from(x);
        for chunk in s.utf8_chunks() {
            for ch in chunk.valid().chars() {
                self.glyph(cx, i32::from(y), glyph(ch), c);
                cx += 16;
            }
            for _ in chunk.invalid() {
                self.glyph(cx, i32::from(y), glyph('?'), c);
                cx += 16;
            }
        }
        cx.clamp(i16::MIN.into(), i16::MAX.into()) as i16
    }

    fn glyph(&mut self, cx: i32, y: i32, g: &[u8; 8], c: u16) {
        for (row, bits) in g.iter().enumerate() {
            for col in 0..8 {
                if bits >> col & 1 != 0 {
                    let px = i16::try_from(cx + 2 * col);
                    let py = i16::try_from(y + 2 * row as i32);
                    if let (Ok(px), Ok(py)) = (px, py) {
                        self.fill_rect(px, py, 2, 2, c);
                    }
                }
            }
        }
    }}

// Dirty box of draw calls, packed into a u32 so it fits one atomic: bytes x0, y0, x1, y1 (inclusive)
// in 2 px units. ponytail: the 2 px grid flushes at most 1 extra px per side; exact needs 36 bits.
pub const EMPTY: u32 = 0x0000_FFFF;
pub const FULL: u32 = ((W as u32 - 1) / 2) << 16 | ((H as u32 - 1) / 2) << 24;

// the part of a fill_rect that lands on screen
pub fn rect_box(x: i16, y: i16, w: u16, h: u16) -> u32 {
    let x0 = i32::from(x).max(0);
    let y0 = i32::from(y).max(0);
    let x1 = (i32::from(x) + i32::from(w)).min(W as i32);
    let y1 = (i32::from(y) + i32::from(h)).min(H as i32);
    if x0 >= x1 || y0 >= y1 {
        return EMPTY;
    }
    (x0 / 2 | (y0 / 2) << 8 | ((x1 - 1) / 2) << 16 | ((y1 - 1) / 2) << 24) as u32
}

pub fn merge(a: u32, b: u32) -> u32 {
    let [a0, a1, a2, a3] = a.to_le_bytes();
    let [b0, b1, b2, b3] = b.to_le_bytes();
    u32::from_le_bytes([a0.min(b0), a1.min(b1), a2.max(b2), a3.max(b3)])
}

// pixels (x0, y0, x1, y1), inclusive
pub fn area(b: u32) -> Option<[usize; 4]> {
    let [x0, y0, x1, y1] = b.to_le_bytes().map(usize::from);
    (x0 <= x1 && y0 <= y1).then(|| [2 * x0, 2 * y0, (2 * x1 + 1).min(W - 1), (2 * y1 + 1).min(H - 1)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_rect_clips() {
        let mut px = vec![0u16; W * H];
        let mut c = Canvas { px: &mut px };
        c.fill_rect(-10, -10, 20, 20, 0xFFFF);
        c.fill_rect(460, 460, 50, 50, 0xFFFF);
        assert_eq!(px.iter().filter(|&&p| p == 0xFFFF).count(), 100 + 36);
    }

    #[test]
    fn blit_clips_and_keeps_order() {
        let src: Vec<u16> = (1..=100).collect();
        let mut px = vec![0u16; W * H];
        assert!(Canvas { px: &mut px }.blit(460, 460, 10, 10, &src), "6 x 6 land");
        assert_eq!(px[460 + 466 * 460], 1, "top-left of the source, clipped");
        assert_eq!(px[465 + 466 * 465], 56, "source row 6, col 6");
        assert_eq!(px.iter().filter(|&&p| p != 0).count(), 36);

        let mut px = vec![0u16; W * H];
        assert!(Canvas { px: &mut px }.blit(-3, -2, 5, 4, &src), "part on screen");
        assert_eq!(px[0], 14, "source (3, 2)");
        assert_eq!(px[1 + 1 * W], 20, "source (4, 3)");
        assert_eq!(px.iter().filter(|&&p| p != 0).count(), 4);

        let mut px = vec![0u16; W * H];
        assert!(!Canvas { px: &mut px }.blit(500, 0, 4, 4, &src), "off screen");
        assert!(!Canvas { px: &mut px }.blit(0, 0, 4, 4, &src[..3]), "short source");
    }

    #[test]
    fn text_stays_in_cells() {
        let mut px = vec![0u16; W * H];
        let mut c = Canvas { px: &mut px };
        assert_eq!(c.text(100, 100, b"A\xD0", 0xFFFF), 132);
        assert!(px.iter().any(|&p| p != 0));
        assert!(px.iter().enumerate().all(|(i, &p)| p == 0 || (100..132).contains(&(i % W)) && (100..116).contains(&(i / W))));
    }

    fn render(s: &[u8]) -> (i16, Vec<u16>) {
        let mut px = vec![0u16; W * H];
        let end = Canvas { px: &mut px }.text(10, 10, s, 1);
        (end, px)
    }

    fn expect(cells: &[char]) -> Vec<u16> {
        let mut px = vec![0u16; W * H];
        let mut c = Canvas { px: &mut px };
        for (i, &ch) in cells.iter().enumerate() {
            c.glyph(10 + 16 * i as i32, 10, glyph(ch), 1);
        }
        px
    }

    #[test]
    fn cyrillic_mixed_advance_and_pixels() {
        let (end, px) = render("aЖб Ёё?".as_bytes());
        assert_eq!(end, 10 + 7 * 16);
        assert_eq!(px, expect(&['a', 'Ж', 'б', ' ', 'Ё', 'ё', '?']));
        assert_ne!(glyph('Ж'), glyph('?'));
        assert_ne!(glyph('ё'), glyph('е'));
        assert!((0x410..=0x44F).chain([0x401, 0x451]).all(|u| glyph(char::from_u32(u).unwrap()) != glyph('?')));
    }

    #[test]
    fn other_scripts_are_one_question_mark() {
        let (end, px) = render("é€😀".as_bytes());
        assert_eq!(end, 10 + 3 * 16);
        assert_eq!(px, expect(&['?', '?', '?']));
    }

    #[test]
    fn invalid_utf8_is_one_question_mark_per_byte() {
        let (end, px) = render(b"a\xFF\xC3(\x80");
        assert_eq!(end, 10 + 5 * 16);
        assert_eq!(px, expect(&['a', '?', '?', '(', '?']));
        // truncated at the end: lead byte of a 3-byte sequence plus one continuation
        let (end, px) = render(b"\xD0\xBF\xE2\x82");
        assert_eq!(end, 10 + 3 * 16);
        assert_eq!(px, expect(&['п', '?', '?']));
        assert_eq!(render(b"").0, 10);
    }

    #[test]
    fn dirty_box_clips_and_merges() {
        assert_eq!(area(EMPTY), None);
        assert_eq!(area(FULL), Some([0, 0, W - 1, H - 1]));
        assert_eq!(area(rect_box(-10, -10, 20, 20)), Some([0, 0, 9, 9]));
        assert_eq!(area(rect_box(460, 461, 50, 50)), Some([460, 460, W - 1, H - 1]));
        assert_eq!(area(rect_box(101, 7, 1, 1)), Some([100, 6, 101, 7]));
        for (x, y, w, h) in [(0, 0, 0, 5), (0, 0, 5, 0), (466, 0, 5, 5), (0, -20, 5, 20), (i16::MAX, 0, 9, 9)] {
            assert_eq!(rect_box(x, y, w, h), EMPTY);
        }
        assert_eq!(merge(EMPTY, EMPTY), EMPTY);
        let a = rect_box(10, 300, 4, 4);
        assert_eq!(merge(EMPTY, a), a);
        assert_eq!(area(merge(a, rect_box(200, 20, 10, 2))), Some([10, 20, 209, 303]));
        assert_eq!(merge(a, FULL), FULL);
    }
}
