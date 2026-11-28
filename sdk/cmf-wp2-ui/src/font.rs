//! Proportional text in Inter Medium 20 px (inter.rs, baked by hosts/fontgen.py): sharp on the ~350 ppi panel,
//! about as many rects per char as the 8×8 font. Same calls as [`crate::text`], measured in pixels instead of cells.
//! `y` is where the old 16-px cell started: caps sit at the same height, descenders reach `y + HEIGHT`.

use crate::Surface;
use crate::inter::{ADV, CODES, OFF, RECTS};
use core::sync::atomic::{AtomicUsize, Ordering};
pub use crate::inter::HEIGHT;

/// Emoji take this much room in a line; an app draws them (`set_emoji`, e.g. the watch's own emoji), else a box.
pub const EMOJI_W: i32 = 24;
/// Draws emoji `cp` with its top-left at (x, y) through `rect`; false: not one it has (a box is drawn).
pub type EmojiFn = fn(cp: u32, x: i32, y: i32, rect: &mut dyn FnMut(i32, i32, i32, i32, u16)) -> bool;
static EMOJI: AtomicUsize = AtomicUsize::new(0);

/// Who draws emoji (once, at the start).
pub fn set_emoji(f: EmojiFn) {
    EMOJI.store(f as usize, Ordering::Relaxed);
}

/// Pictographs and symbols drawn as emoji.
pub fn is_emoji(c: u32) -> bool {
    (0x1F000..0x1FB00).contains(&c) || (0x2600..0x27C0).contains(&c) || matches!(c, 0x2B50 | 0x2B55 | 0x2B1B | 0x2B1C | 0x203C | 0x2049)
}

/// Parts of an emoji that take no room: variation selectors, joiners, skin tones, tags.
fn invisible(c: u32) -> bool {
    matches!(c, 0xFE0E | 0xFE0F | 0x200D | 0x20E3) || (0x1F3FB..=0x1F3FF).contains(&c) || (0xE0020..=0xE007F).contains(&c)
}

fn draw_emoji(sf: &mut impl Surface, cp: u32, x: i32, y: i32, c: u16) {
    let f = EMOJI.load(Ordering::Relaxed);
    if f != 0 {
        // a fn pointer stored by set_emoji
        let f: EmojiFn = unsafe { core::mem::transmute::<usize, EmojiFn>(f) };
        let mut rect = |x: i32, y: i32, w: i32, h: i32, c: u16| sf.rect(x, y, w, h, c);
        if f(cp, x + 1, y - 1, &mut rect) {
            return;
        }
    }
    // a box, as a missing emoji shows
    for (bx, by, bw, bh) in [(3, 0, 18, 2), (3, 18, 18, 2), (3, 0, 2, 20), (19, 0, 2, 20)] {
        sf.rect(x + bx, y + by, bw, bh, c);
    }
}

fn glyph(ch: char) -> usize {
    let c = ch as u32;
    let find = |c: u32| CODES.binary_search(&(c.min(0xFFFF) as u16)).ok();
    find(c)
        .or_else(|| match ch {
            '\u{2018}' => find('\'' as u32),
            '\t' => find(' ' as u32),
            _ => None,
        })
        .unwrap_or_else(|| find('?' as u32).unwrap_or(0))
}

/// Advance of one char in px.
pub fn advance(ch: char) -> i32 {
    let c = ch as u32;
    if invisible(c) {
        0
    } else if is_emoji(c) {
        EMOJI_W
    } else {
        i32::from(ADV[glyph(ch)])
    }
}

pub fn width(s: &str) -> i32 {
    s.chars().map(advance).sum()
}

/// Draws `s` from `x`; returns the x after it.
pub fn draw(sf: &mut impl Surface, x: i32, y: i32, s: &str, c: u16) -> i32 {
    let mut x = x;
    for ch in s.chars() {
        let cp = ch as u32;
        if invisible(cp) {
            continue;
        }
        if is_emoji(cp) {
            draw_emoji(sf, cp, x, y, c);
            x += EMOJI_W;
            continue;
        }
        let g = glyph(ch);
        for r in RECTS[usize::from(OFF[g]) * 4..usize::from(OFF[g + 1]) * 4].chunks_exact(4) {
            sf.rect(x + i32::from(r[0]), y + i32::from(r[1] as i8), i32::from(r[2]), i32::from(r[3]), c);
        }
        x += i32::from(ADV[g]);
    }
    x
}

/// Longest prefix of `s` at most `px` wide; `true` if it was cut.
pub fn clip(s: &str, px: i32) -> (&str, bool) {
    let mut w = 0;
    for (i, ch) in s.char_indices() {
        w += advance(ch);
        if w > px {
            return (&s[..i], true);
        }
    }
    (s, false)
}

/// `text` in at most `px`; a cut text ends in "…". Returns whether it was cut.
pub fn fitted(sf: &mut impl Surface, x: i32, y: i32, text: &str, px: i32, c: u16) -> bool {
    let (shown, cut) = clip(text, px);
    if cut {
        let (shown, _) = clip(shown, px - advance('\u{2026}'));
        let end = draw(sf, x, y, shown.trim_end_matches(' '), c);
        draw(sf, end, y, "\u{2026}", c);
    } else {
        draw(sf, x, y, shown, c);
    }
    cut
}

/// `text` centred on `cx`.
pub fn centre(sf: &mut impl Surface, cx: i32, y: i32, text: &str, c: u16) {
    draw(sf, cx - width(text) / 2, y, text, c);
}

/// Greedy word wrap into `out`, lines at most `px` wide (`\n` breaks, a longer word is cut hard). The last slot
/// gets all the rest: draw it with [`fitted`]. Returns the number of lines.
pub fn wrap<'a>(text: &'a str, px: i32, out: &mut [&'a str]) -> usize {
    let mut n = 0;
    let mut rest = text.trim_start_matches(' ');
    while !rest.is_empty() && n < out.len() {
        if n + 1 == out.len() {
            out[n] = rest;
            return n + 1;
        }
        let (line, next) = split(rest, px);
        out[n] = line.trim_end_matches(' ');
        n += 1;
        rest = next.trim_start_matches(' ');
    }
    n
}

/// The lines [`wrap`] makes, all of them, one at a time (no array to size): for a text that scrolls.
pub fn lines(text: &str, px: i32) -> impl Iterator<Item = &str> {
    let mut rest = text.trim_start_matches(' ');
    core::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let (line, next) = split(rest, px);
        rest = next.trim_start_matches(' ');
        Some(line.trim_end_matches(' '))
    })
}

fn split(s: &str, px: i32) -> (&str, &str) {
    let (mut space, mut w) = (None, 0);
    for (i, ch) in s.char_indices() {
        if ch == '\n' {
            return (&s[..i], &s[i + 1..]);
        }
        w += advance(ch);
        if w > px {
            if ch == ' ' {
                return (&s[..i], &s[i..]);
            }
            return match space {
                Some(j) if j > 0 => (&s[..j], &s[j..]),
                _ => {
                    let k = if i == 0 { ch.len_utf8() } else { i };
                    (&s[..k], &s[k..])
                }
            };
        }
        if ch == ' ' {
            space = Some(i);
        }
    }
    (s, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Count(u32, i32, i32);
    impl Surface for Count {
        fn rect(&mut self, _: i32, y: i32, _: i32, h: i32, _: u16) {
            self.0 += 1;
            self.1 = self.1.min(y);
            self.2 = self.2.max(y + h);
        }
        fn text(&mut self, _: i32, _: i32, _: &str, _: u16) {}
    }

    #[test]
    fn proportional() {
        assert!(width("i") < width("m"));
        assert_eq!(width("ab"), advance('a') + advance('b'));
        // unknown chars draw as '?'; emoji take an emoji's room (drawn by the app, else a box), their joiners none
        assert_eq!(advance('\u{0D85}'), advance('?'));
        assert_eq!(advance('\u{1F600}'), EMOJI_W);
        assert_eq!(width("\u{1F44D}\u{1F3FD}\u{FE0F}"), EMOJI_W);
    }

    #[test]
    fn draws_in_the_line_box() {
        let mut c = Count(0, 99, 0);
        draw(&mut c, 0, 100, "Hgjy|", 0xFFFF);
        assert!(c.1 >= 100 - 4 && c.2 <= 100 + HEIGHT + 1, "{} {}", c.1, c.2);
        assert!(c.0 < 80);
    }

    #[test]
    fn wraps_by_width() {
        let mut out = [""; 4];
        let n = wrap("hey are we still on for tonight?", 150, &mut out);
        assert!(n >= 2);
        for l in &out[..n - 1] {
            assert!(width(l) <= 150, "{l}");
        }
        assert_eq!(wrap("a\nb", 150, &mut out), 2);
        let (s, cut) = clip("hello world", width("hello"));
        assert_eq!((s, cut), ("hello", true));
    }
}
