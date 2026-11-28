//! Text in [`CHAR`]-px cells: measure, centre, fit with an ellipsis, wrap, numbers and big (scaled) text.

use crate::{CHAR, Surface};
use wp2_core::font8x8::{CYRILLIC, GLYPHS};

/// Longest prefix of `s` with at most `max` bytes that ends on a char boundary (invalid UTF-8 is cut off).
pub fn fit(s: &[u8], max: usize) -> &str {
    let s = match core::str::from_utf8(s) {
        Ok(v) => v,
        Err(e) => unsafe { core::str::from_utf8_unchecked(&s[..e.valid_up_to()]) },
    };
    let mut n = s.len().min(max);
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    &s[..n]
}

/// At most `chars` chars of `s`; `true` if it was cut.
pub fn clip(s: &str, chars: usize) -> (&str, bool) {
    match s.char_indices().nth(chars) {
        Some((i, _)) => (&s[..i], true),
        None => (s, false),
    }
}

/// Cells `s` takes (one per char).
pub fn chars(s: &str) -> usize {
    s.chars().count()
}

pub fn width(s: &str) -> i32 {
    chars(s) as i32 * CHAR
}

/// Three dots in the cell at `x`.
pub fn ellipsis(s: &mut impl Surface, x: i32, y: i32, c: u16) {
    for i in 0..3 {
        s.rect(x + 1 + i * 5, y + 12, 2, 2, c);
    }
}

/// `text` in at most `cells` cells; a cut text ends with an ellipsis cell. Returns whether it was cut.
pub fn fitted(s: &mut impl Surface, x: i32, y: i32, text: &str, cells: usize, c: u16) -> bool {
    let (shown, cut) = clip(text, cells);
    if cut {
        let (shown, _) = clip(shown, cells.saturating_sub(1));
        s.text(x, y, shown, c);
        if cells > 0 {
            ellipsis(s, x + (cells as i32 - 1) * CHAR, y, c);
        }
    } else {
        s.text(x, y, shown, c);
    }
    cut
}

/// `text` centred on `cx`.
pub fn centre(s: &mut impl Surface, cx: i32, y: i32, text: &str, c: u16) {
    s.text(cx - width(text) / 2, y, text, c);
}

/// Greedy word wrap into `out` (lines of at most `cells` chars; `\n` breaks; a longer word is cut hard). The last
/// slot gets all the rest, so draw it with [`fitted`] to end in an ellipsis. Returns the number of lines.
pub fn wrap<'a>(text: &'a str, cells: usize, out: &mut [&'a str]) -> usize {
    let mut n = 0;
    let mut rest = text.trim_start_matches(' ');
    while !rest.is_empty() && n < out.len() {
        if n + 1 == out.len() {
            out[n] = rest;
            return n + 1;
        }
        let (line, next) = split(rest, cells);
        out[n] = line.trim_end_matches(' ');
        n += 1;
        rest = next.trim_start_matches(' ');
    }
    n
}

fn split(s: &str, cells: usize) -> (&str, &str) {
    let (mut space, mut n) = (None, 0);
    for (i, ch) in s.char_indices() {
        if ch == '\n' {
            return (&s[..i], &s[i + 1..]);
        }
        if n == cells {
            if ch == ' ' {
                return (&s[..i], &s[i..]);
            }
            return match space {
                Some(j) if j > 0 => (&s[..j], &s[j..]),
                _ => (&s[..i.max(ch.len_utf8())], &s[i.max(ch.len_utf8())..]),
            };
        }
        if ch == ' ' {
            space = Some(i);
        }
        n += 1;
    }
    (s, "")
}

/// Decimal `n` into `buf`.
pub fn int(n: i32, buf: &mut [u8; 11]) -> &str {
    let mut i = buf.len();
    let mut v = n.unsigned_abs();
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    if n < 0 {
        i -= 1;
        buf[i] = b'-';
    }
    core::str::from_utf8(&buf[i..]).unwrap_or("?")
}

fn glyph(ch: char) -> &'static [u8; 8] {
    match ch {
        ' '..='~' => &GLYPHS[ch as usize - 0x20],
        '\u{410}'..='\u{44F}' => &CYRILLIC[ch as usize - 0x410],
        '\u{401}' => &CYRILLIC[64],
        '\u{451}' => &CYRILLIC[65],
        _ => &GLYPHS[usize::from(b'?' - 0x20)],
    }
}

/// The loader's font at `scale` px per font pixel (2 = normal text): `8 · scale` px per char.
pub fn big(s: &mut impl Surface, x: i32, y: i32, text: &str, scale: i32, c: u16) {
    for (i, ch) in text.chars().enumerate() {
        let g = glyph(ch);
        let gx = x + i as i32 * 8 * scale;
        let mut row = 0;
        while row < 8 {
            let bits = g[row];
            let mut h = 1;
            while row + h < 8 && g[row + h] == bits {
                h += 1;
            }
            let mut col = 0;
            while col < 8 {
                if bits >> col & 1 == 0 {
                    col += 1;
                    continue;
                }
                let from = col;
                while col < 8 && bits >> col & 1 != 0 {
                    col += 1;
                }
                s.rect(gx + from * scale, y + row as i32 * scale, (col - from) * scale, h as i32 * scale, c);
            }
            row += h;
        }
    }
}

/// Dot-matrix text (the Nothing look): each font pixel is a `dot`-px square on a `pitch`-px grid, so `8 · pitch`
/// px per char with gaps between the dots.
pub fn dots(s: &mut impl Surface, x: i32, y: i32, text: &str, pitch: i32, dot: i32, c: u16) {
    for (i, ch) in text.chars().enumerate() {
        let g = glyph(ch);
        let gx = x + i as i32 * 8 * pitch;
        for (row, bits) in g.iter().enumerate() {
            for col in 0..8 {
                if bits >> col & 1 != 0 {
                    s.rect(gx + col * pitch, y + row as i32 * pitch, dot, dot, c);
                }
            }
        }
    }
}

/// [`dots`] with the off dots painted `bg`: a new text drawn over an old one at the same pitch replaces it without
/// clearing first (a clear shows on the panel as a flicker).
pub fn dots_opaque(s: &mut impl Surface, x: i32, y: i32, text: &str, pitch: i32, dot: i32, c: u16, bg: u16) {
    for (i, ch) in text.chars().enumerate() {
        let g = glyph(ch);
        let gx = x + i as i32 * 8 * pitch;
        for (row, bits) in g.iter().enumerate() {
            for col in 0..8 {
                let on = bits >> col & 1 != 0;
                s.rect(gx + col * pitch, y + row as i32 * pitch, dot, dot, if on { c } else { bg });
            }
        }
    }
}

/// [`dots`] centred on `cx`.
pub fn dots_centre(s: &mut impl Surface, cx: i32, y: i32, text: &str, pitch: i32, dot: i32, c: u16) {
    dots(s, cx - chars(text) as i32 * 4 * pitch, y, text, pitch, dot, c);
}

pub fn big_width(text: &str, scale: i32) -> i32 {
    chars(text) as i32 * 8 * scale
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Count, Fb, SIZE};
    use std::vec;

    #[test]
    fn utf8_fit_and_clip() {
        assert_eq!(fit("привет".as_bytes(), 5), "пр");
        assert_eq!(fit(b"ab\xD0", 10), "ab");
        assert_eq!(clip("привет мир", 6), ("привет", true));
        assert_eq!(clip("ok", 6), ("ok", false));
        assert_eq!((chars("Ёжик"), width("Ёжик")), (4, 64));
    }

    #[test]
    fn wrap_words_cyrillic_and_rest() {
        let mut out = [""; 4];
        let n = wrap("Съешь же ещё этих мягких французских булок", 12, &mut out);
        assert_eq!(&out[..n], ["Съешь же ещё", "этих мягких", "французских", "булок"]);
        let n = wrap("  длинноесловобезпробелов и ещё", 8, &mut out);
        assert_eq!(&out[..n], ["длинноес", "ловобезп", "робелов", "и ещё"]);
        let mut two = [""; 2];
        assert_eq!(wrap("one two three four", 8, &mut two), 2);
        assert_eq!(two, ["one two", "three four"]);
        assert_eq!(wrap("a\nb c", 10, &mut out), 2);
        assert_eq!(&out[..2], ["a", "b c"]);
        assert_eq!(wrap("   ", 10, &mut out), 0);
        assert_eq!(wrap("abc", 0, &mut two), 2);
    }

    #[test]
    fn fitted_cuts_with_ellipsis() {
        let mut px = vec![0u16; (SIZE * SIZE) as usize];
        let mut fb = Fb::new(&mut px);
        assert!(fitted(&mut fb, 10, 10, "Рефакторинг парсера", 6, 0xFFFF));
        // 5 chars, then the dots in the 6th cell, nothing after
        assert_eq!(fb.at(10 + 5 * CHAR + 1, 22), 0xFFFF);
        assert!((10 + 6 * CHAR..SIZE).all(|x| (10..26).all(|y| fb.at(x, y) == 0)));
        assert!(!fitted(&mut fb, 10, 40, "ok", 6, 0xFFFF));
    }

    #[test]
    fn numbers() {
        let mut b = [0u8; 11];
        assert_eq!(int(0, &mut b), "0");
        assert_eq!(int(-42, &mut b), "-42");
        assert_eq!(int(i32::MIN, &mut b), "-2147483648");
    }

    #[test]
    fn big_text_scales_the_font() {
        let mut px = vec![0u16; (SIZE * SIZE) as usize];
        let mut fb = Fb::new(&mut px);
        big(&mut fb, 0, 0, "1Ж", 2, 0xFFFF);
        let mut ref_px = vec![0u16; (SIZE * SIZE) as usize];
        let mut r = Fb::new(&mut ref_px);
        crate::Surface::text(&mut r, 0, 0, "1Ж", 0xFFFF);
        assert!(px == ref_px, "scale 2 is the loader's text");
        let mut n = Count::default();
        big(&mut n, 0, 0, "0123456789", 8, 0xFFFF);
        assert!(n.rects < 200, "{}", n.rects);
        assert_eq!(big_width("12:30", 6), 240);
    }
}
