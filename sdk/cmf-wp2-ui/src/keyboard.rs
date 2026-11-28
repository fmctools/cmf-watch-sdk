//! A QWERTY keyboard laid out for the round panel (Wear OS style), the watch's emoji as a second page, and the text
//! being typed. With [`crate::dial`] (the rotary keyboard) and [`crate::dictate`] (voice typing) it gives any app
//! text input; `sdk/examples/keyboards` shows the three together.
//!
//! The app owns the loop: a TOUCH_DOWN goes to [`Keyboard::tap`], a TOUCH_UP to [`Keyboard::release`], a crown
//! turn to [`Keyboard::crown`] (the emoji pages, else scrolling the text), the loop's timeout includes
//! [`Keyboard::hold_wait`] and each pass calls [`Keyboard::tick`] (delete held: it repeats). The [`Act`] says what to
//! draw. A pressed key shows white for [`FLASH_MS`]: draw it with [`Keyboard::draw_key`] `pressed`, then plain again
//! when the time is up. The text is at most [`TEXT_MAX`] bytes.
//!
//! The app sets what the field is before drawing: [`Keyboard::input`] (text, or a number pad for integers and
//! decimals), [`Keyboard::action`] (what the action key says: send, done, next) and [`Keyboard::multiline`] (the
//! action key types a new line and a submit button beside the text confirms).
//!
//! Emoji: call [`crate::emoji::init`] once; until then (or on a loader before 6) the keyboard has no emoji key.

use crate::color::{rgb565, Rgb, BLACK};
use crate::draw::{disc, ring, rrect};
use crate::font::{self, centre, clip};
use crate::geom::chord;
use crate::repeat::Repeat;
use crate::text::fit;
use crate::{dictate, emoji, Surface, C};

pub const TEXT_MAX: usize = 100;
/// A pressed key shows white this long.
pub const FLASH_MS: u32 = 110;
/// Key height, and the top of each of the four rows.
pub const KH: i32 = 58;
pub const ROW_Y: [i32; 4] = [184, 242, 300, 358];
/// The mic button beside the text: voice typing.
pub const MIC_X: i32 = 404;
pub const MIC_Y: i32 = 140;
/// The submit button on the other side of the text, in a multiline field.
pub const SUBMIT_X: i32 = 62;
/// The text box: its first line's y, lines, width.
const TEXT_Y: i32 = 76;
const TEXT_ROWS: usize = 3;
const TEXT_PX: i32 = 280;
/// The emoji page: COLS × ROWS cells of CELL px from TOP, the keys row below.
const EM_COLS: usize = 6;
const EM_ROWS: usize = 3;
const EM_PAGE: usize = EM_COLS * EM_ROWS;
const EM_CELL: i32 = 58;
const EM_TOP: i32 = 166;
const LINE: i32 = 25;

const CARD: Rgb = Rgb(0x17, 0x17, 0x17);
const KEYC: Rgb = Rgb(0x26, 0x26, 0x26);
const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);
const W: u16 = 0xFFFF;
const SOFT: u16 = rgb565(0xC8, 0xC8, 0xC8);
const GREY: u16 = rgb565(0x80, 0x80, 0x80);
const DIM: u16 = rgb565(0x3A, 0x3A, 0x3A);
/// The face on the emoji key.
const FACE: u32 = 0x1F603;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Ch(u8),
    Shift,
    Del,
    Sym,
    Space,
    /// the field's [`Action`]; in a multiline field a new line
    Action,
    /// the minus on a number pad (signed numbers)
    Minus,
    /// to the emoji page and back
    Emoji,
    /// to the rotary keyboard
    Dial,
    /// no key: room kept in a number pad's grid
    Gap,
}

/// What the field is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Input {
    /// letters, symbols, emoji, voice typing, the rotary keyboard
    Text,
    /// a number pad: digits, a leading minus if `signed`
    Integer { signed: bool },
    /// a number pad with one decimal point
    Decimal { signed: bool },
}

/// What the action key does (its label); the app hears [`Act::Submit`] whichever it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Send,
    Done,
    /// on to the app's next field
    Next,
}

impl Action {
    fn label(self) -> &'static str {
        match self {
            Action::Send => ">",
            Action::Done => "ok",
            Action::Next => "next",
        }
    }
}

/// What a tap asks the app to do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    None,
    /// draw the whole keyboard ([`Keyboard::draw`])
    Redraw,
    /// the text changed: [`Keyboard::draw_text`], then `key` pressed for [`FLASH_MS`]
    Typed(Key),
    /// an emoji added from the page, or the text scrolled: [`Keyboard::draw_text`]
    Text,
    /// the action key (or the submit button): [`Keyboard::text`] is the result, [`Keyboard::action`] says what next
    Submit,
    /// the rotary keyboard ([`crate::dial`]) instead
    Dial,
    /// voice typing ([`crate::dictate`]); its words go in with [`Keyboard::add_words`]
    Voice,
}

/// One keyboard row: up to 10 keys `(key, width)`, centred.
struct Row {
    keys: [(Key, i32); 10],
    n: usize,
}

fn chars_row(s: &[u8], w: i32) -> Row {
    let mut keys = [(Key::Space, 0); 10];
    for (i, &c) in s.iter().enumerate() {
        keys[i] = (Key::Ch(c), w);
    }
    Row { keys, n: s.len() }
}

fn row(r: usize, sym: bool, emoji: bool, dial: bool) -> Row {
    match (r, sym) {
        (0, false) => chars_row(b"qwertyuiop", 44),
        (1, false) => chars_row(b"asdfghjkl", 44),
        (0, true) => chars_row(b"1234567890", 44),
        (1, true) => chars_row(b"@#&-+()/'", 44),
        (2, _) => {
            let mut row = chars_row(if sym { b"\"*:;!?," } else { b"zxcvbnm" }, 42);
            row.keys.copy_within(0..7, 1);
            row.keys[0] = (if sym { Key::Ch(b'.') } else { Key::Shift }, 46);
            row.keys[8] = (Key::Del, 46);
            row.n = 9;
            row
        }
        _ => {
            // the bottom row: the switches the app has; space takes what is left of 286 px (the chord at the row's foot is ~288)
            let mut row = chars_row(b"", 0);
            let mut keys = [(Key::Sym, 54), (Key::Emoji, 46), (Key::Dial, 46), (Key::Space, 0), (Key::Action, 58)];
            let shown = |k: Key| match k {
                Key::Emoji => emoji,
                Key::Dial => dial,
                _ => true,
            };
            let used: i32 = keys.iter().filter(|k| shown(k.0)).map(|k| k.1).sum();
            keys[3].1 = 286 - used;
            for k in keys.into_iter().filter(|k| shown(k.0)) {
                row.keys[row.n] = k;
                row.n += 1;
            }
            row
        }
    }
}

/// A number pad's row (Gboard-like): three digits, then delete, minus, point, the action down the side.
fn num_row(r: usize, signed: bool, decimal: bool) -> Row {
    const PAD: i32 = 66;
    let mut row = chars_row(&b"123456789"[(r * 3).min(6)..(r * 3).min(6) + 3], PAD);
    let side = match r {
        0 => Key::Del,
        1 if signed => Key::Minus,
        2 if decimal => Key::Ch(b'.'),
        3 => Key::Action,
        _ => Key::Gap,
    };
    if r == 3 {
        row.keys[..3].copy_from_slice(&[(Key::Gap, PAD), (Key::Ch(b'0'), PAD), (Key::Gap, PAD)]);
    }
    row.keys[3] = (side, PAD);
    row.n = 4;
    row
}

/// The emoji page's bottom row.
fn each_emoji_key(mut f: impl FnMut(Key, i32, i32, i32)) {
    let keys = [(Key::Emoji, 62), (Key::Del, 52), (Key::Space, 90), (Key::Action, 72)];
    let mut x = C - keys.iter().map(|k| k.1).sum::<i32>() / 2;
    for (k, w) in keys {
        f(k, x, ROW_Y[3], w);
        x += w;
    }
}

fn rule(s: &mut impl Surface, y: i32) {
    let (x0, x1) = chord(y, y + 2, 40);
    let mut x = x0;
    while x < x1 {
        s.rect(x, y, 2, 2, DIM);
        x += 8;
    }
}

/// Draws through a surface with body text in Inter ([`crate::font`]).
struct Inter<'a, S: Surface>(&'a mut S);

impl<S: Surface> Surface for Inter<'_, S> {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        self.0.rect(x, y, w, h, c);
    }

    fn text(&mut self, x: i32, y: i32, s: &str, c: u16) {
        font::draw(self.0, x, y, s, c);
    }
}

pub struct Keyboard {
    /// numbers and symbols instead of letters
    pub sym: bool,
    /// the next letter a capital (at the start, and after the end of a sentence)
    pub shift: bool,
    /// the emoji page shown, and which of its pages
    pub emoji_page: Option<usize>,
    /// a key for the rotary keyboard ([`Act::Dial`])
    pub dial: bool,
    /// the action key's colour and the caret's
    pub accent: Rgb,
    /// shown while nothing is typed
    pub hint: &'static str,
    pub input: Input,
    pub action: Action,
    /// the action key types a new line; a submit button beside the text confirms (text input only)
    pub multiline: bool,
    text: [u8; TEXT_MAX],
    n: usize,
    /// lines scrolled back from the end (the caret's)
    scroll: usize,
    /// delete held
    del: Repeat,
}

impl Keyboard {
    pub const fn new(accent: Rgb) -> Self {
        Keyboard {
            sym: false,
            shift: true,
            emoji_page: None,
            dial: true,
            accent,
            hint: "Message",
            input: Input::Text,
            action: Action::Send,
            multiline: false,
            text: [0; TEXT_MAX],
            n: 0,
            scroll: 0,
            del: Repeat::new(),
        }
    }

    fn number(&self) -> bool {
        self.input != Input::Text
    }

    fn lines_mode(&self) -> bool {
        self.multiline && !self.number()
    }

    /// Whether `c` may go next: a number field takes digits, a leading minus (signed) and one point (decimal).
    fn allowed(&self, c: u8) -> bool {
        let typed = &self.text[..self.n];
        match self.input {
            Input::Text => true,
            Input::Integer { signed } | Input::Decimal { signed } => match c {
                b'0'..=b'9' => true,
                b'-' => signed && typed.is_empty(),
                b'.' => matches!(self.input, Input::Decimal { .. }) && !typed.contains(&b'.'),
                _ => false,
            },
        }
    }

    fn push(&mut self, c: u8) -> bool {
        if self.n >= TEXT_MAX || !self.allowed(c) {
            return false;
        }
        self.text[self.n] = c;
        self.n += 1;
        self.scroll = 0;
        true
    }

    pub fn text(&self) -> &str {
        fit(&self.text[..self.n], TEXT_MAX)
    }

    pub fn clear(&mut self) {
        self.n = 0;
        self.shift = true;
        self.scroll = 0;
        self.del.stop();
    }

    /// Words from voice typing (or anywhere) after what is typed, a space between; cut at a char to fit.
    pub fn add_words(&mut self, words: &str) {
        let words = words.trim();
        if words.is_empty() {
            return;
        }
        self.scroll = 0;
        if self.number() {
            // a number field keeps what fits it ("minus 4.5" heard as "-4.5")
            for c in words.bytes() {
                self.push(c);
            }
            return;
        }
        if self.n > 0 && self.text[self.n - 1] != b' ' && self.n < TEXT_MAX {
            self.text[self.n] = b' ';
            self.n += 1;
        }
        let fits = fit(words.as_bytes(), TEXT_MAX - self.n);
        self.text[self.n..self.n + fits.len()].copy_from_slice(fits.as_bytes());
        self.n += fits.len();
        self.shift = false;
    }

    /// A key that types (the rotary keyboard's too): a char, space, minus, a new line (the action key in a
    /// multiline field) or delete. Whether the text changed: a number field ignores what doesn't fit it.
    pub fn type_key(&mut self, k: Key) -> bool {
        match k {
            Key::Del => {
                if self.n == 0 {
                    return false;
                }
                // a whole char, emoji included
                self.n = self.text().char_indices().last().map_or(0, |(i, _)| i);
                self.shift = self.n == 0 || self.text[self.n - 1] == b'\n';
                self.scroll = 0;
                true
            }
            Key::Space | Key::Ch(_) | Key::Minus | Key::Action => {
                let c = match k {
                    Key::Ch(c) if self.shift && !self.number() => c.to_ascii_uppercase(),
                    Key::Ch(c) => c,
                    Key::Minus => b'-',
                    Key::Action => b'\n',
                    _ => b' ',
                };
                if !self.push(c) {
                    return false;
                }
                // capital after the end of a sentence, and on a new line
                self.shift = c == b'\n' || (c == b' ' && self.n >= 2 && matches!(self.text[self.n - 2], b'.' | b'?' | b'!'));
                true
            }
            _ => false,
        }
    }

    fn has_emoji(&self) -> bool {
        !emoji::all().is_empty() && !self.number()
    }

    fn em_pages(&self) -> usize {
        emoji::all().len().div_ceil(EM_PAGE).max(1)
    }

    /// Every key with its rect `(key, x, y, w)`, for drawing and hit tests.
    pub fn each_key(&self, mut f: impl FnMut(Key, i32, i32, i32)) {
        if self.emoji_page.is_some() {
            return each_emoji_key(f);
        }
        for (r, &y) in ROW_Y.iter().enumerate() {
            let row = match self.input {
                Input::Text => row(r, self.sym, self.has_emoji(), self.dial),
                Input::Integer { signed } => num_row(r, signed, false),
                Input::Decimal { signed } => num_row(r, signed, true),
            };
            let total: i32 = row.keys[..row.n].iter().map(|k| k.1).sum();
            let mut x = C - total / 2;
            for &(k, w) in &row.keys[..row.n] {
                if k != Key::Gap {
                    f(k, x, y, w);
                }
                x += w;
            }
        }
    }

    /// The whole screen: `title` on top (empty: none), the text, the keys or the emoji page, the mic (text input)
    /// and the submit button (multiline).
    pub fn draw(&self, s: &mut impl Surface, title: &str) {
        let mut s = Inter(s);
        s.rect(0, 0, crate::SIZE, crate::SIZE, 0);
        if !title.is_empty() {
            let (t, _) = clip(title, 240);
            centre(&mut s, C, 32, t, SOFT);
        }
        match self.emoji_page {
            None => {
                self.draw_text(s.0);
                rule(&mut s, 172);
            }
            Some(page) => {
                self.draw_text(s.0);
                let all = emoji::all();
                let from = page * EM_PAGE;
                let x0 = C - EM_CELL * EM_COLS as i32 / 2;
                let big = emoji::SIZE * 2;
                for (i, &cp) in all.iter().enumerate().skip(from).take(EM_PAGE) {
                    let (col, row) = ((i - from) % EM_COLS, (i - from) / EM_COLS);
                    let x = x0 + col as i32 * EM_CELL + (EM_CELL - big) / 2;
                    let y = EM_TOP + row as i32 * EM_CELL + (EM_CELL - big) / 2;
                    emoji::draw_scaled(cp, x, y, 2, &mut |x, y, w, h, c| s.rect(x, y, w, h, c));
                }
                // which page: dots, the crown turns them
                let n = self.em_pages() as i32;
                for p in 0..n {
                    let on = p as usize == page;
                    s.rect(C - n * 8 + p * 16 + 4, EM_TOP - 12, 6, 6, if on { W } else { GREY });
                }
            }
        }
        self.each_key(|k, _, _, _| self.draw_key(s.0, k, false));
        if !self.number() {
            dictate::mic_button(&mut s, MIC_X, MIC_Y);
        }
        if self.lines_mode() {
            disc(&mut s, SUBMIT_X, MIC_Y, 22, self.accent, BLACK);
            centre(&mut s, SUBMIT_X, MIC_Y - 8, self.action.label(), 0);
        }
    }

    fn text_rows(&self) -> usize {
        if self.emoji_page.is_some() { TEXT_ROWS - 1 } else { TEXT_ROWS }
    }

    /// Lines the text takes at `px` (a new line at the end starts one more, where the caret is).
    fn line_count(&self, px: i32) -> usize {
        let typed = self.text();
        font::lines(typed, px).count() + typed.ends_with('\n') as usize
    }

    /// Only the text (after a key typed): its box cleared and drawn again.
    pub fn draw_text(&self, s: &mut impl Surface) {
        self.draw_text_in(s, TEXT_Y, self.text_rows(), TEXT_PX);
    }

    /// The text in a box of `rows` lines `px` wide, centred, its first line at `y0`: that box only is cleared (the
    /// rotary keyboard's letters round it stay; [`crate::dial::TEXT_Y`] and the rest are its box). The lines with
    /// the caret are shown, or as far back as the crown scrolled.
    pub fn draw_text_in(&self, s: &mut impl Surface, y0: i32, rows: usize, px: i32) {
        s.rect(C - px / 2 - 6, y0 - 6, px + 12, rows as i32 * LINE + 8, 0);
        let mut s = Inter(s);
        let typed = self.text();
        let caret = self.accent.rgb565();
        if typed.is_empty() {
            centre(&mut s, C, y0, self.hint, DIM);
            s.rect(C - font::width(self.hint) / 2 - 8, y0 - 2, 3, 20, caret);
            return;
        }
        let n = self.line_count(px);
        let first = n.saturating_sub(rows + self.scroll);
        let (mut last_w, mut last_y) = (0, y0);
        for (i, l) in font::lines(typed, px).chain(core::iter::once("")).skip(first).take(rows.min(n - first)).enumerate() {
            let (l, _) = clip(l, px);
            let w = font::width(l);
            let y = y0 + i as i32 * LINE;
            s.text(C - w / 2, y, l, W);
            (last_w, last_y) = (w, y);
        }
        if first + rows >= n {
            // the caret, after a trailing space
            let trailing = typed.ends_with(' ') as i32 * font::advance(' ');
            let x = (C + last_w / 2 + trailing + 2).min(C + px / 2 + 3);
            s.rect(x, last_y - 2, 3, 20, caret);
        }
    }

    /// One key, plain or `pressed` (white).
    pub fn draw_key(&self, s: &mut impl Surface, key: Key, pressed: bool) {
        let mut s = Inter(s);
        let (sym, shift) = (self.sym, self.shift);
        self.each_key(|k, x, y, w| {
            if k != key {
                return;
            }
            let fill = match (pressed, k) {
                (true, _) => WHITE,
                (_, Key::Action) if !self.lines_mode() => self.accent,
                (_, Key::Shift) if shift => WHITE,
                (_, Key::Ch(_)) => KEYC,
                _ => CARD,
            };
            s.rect(x, y, w, KH, 0);
            rrect(&mut s, x + 3, y + 3, w - 6, KH - 6, 14, fill, BLACK);
            let ink = if pressed || (k == Key::Shift && shift) { 0 } else { W };
            let (cx, cy) = (x + w / 2, y + KH / 2);
            let mut b = [0u8; 4];
            let label: &str = match k {
                Key::Ch(c) => {
                    let c = if shift && !sym { c.to_ascii_uppercase() } else { c };
                    (c as char).encode_utf8(&mut b)
                }
                Key::Shift => "^",
                Key::Del => "<",
                Key::Sym if sym => "abc",
                Key::Sym => "?12",
                Key::Space => "space",
                Key::Minus => "-",
                Key::Gap => "",
                Key::Action if self.lines_mode() => {
                    // a new line: the return arrow
                    s.rect(cx + 7, cy - 9, 3, 13, ink);
                    s.rect(cx - 6, cy + 1, 16, 3, ink);
                    for i in 0..6 {
                        s.rect(cx - 11 + i, cy + 2 - i, 1, 2 * i + 1, ink);
                    }
                    return;
                }
                Key::Action => self.action.label(),
                Key::Emoji if self.emoji_page.is_some() => "abc",
                Key::Dial => {
                    // the rotary keyboard: a ring with its marker
                    ring(&mut s, cx, cy, 9, 11, if pressed { BLACK } else { WHITE }, fill);
                    disc(&mut s, cx, cy - 14, 3, self.accent, fill);
                    return;
                }
                Key::Emoji => {
                    // a smiling face, from the watch's own emoji
                    let e = emoji::SIZE / 2;
                    if !emoji::draw_scaled(FACE, cx - e, cy - e, 1, &mut |x, y, w, h, c| s.rect(x, y, w, h, c)) {
                        centre(&mut s, cx, cy - 8, ":)", ink);
                    }
                    return;
                }
            };
            let ink = match k {
                Key::Space if !pressed => GREY,
                Key::Action if !pressed => 0,
                _ => ink,
            };
            centre(&mut s, cx, cy - 8, label, ink);
        });
    }

    /// A TOUCH_DOWN at (x, y) at `now` (delete held from then repeats).
    pub fn tap(&mut self, x: i32, y: i32, now: u32) -> Act {
        self.del.stop();
        if !self.number() && dictate::mic_hit(MIC_X, MIC_Y, x, y) {
            return Act::Voice;
        }
        if self.lines_mode() && dictate::mic_hit(SUBMIT_X, MIC_Y, x, y) {
            return Act::Submit;
        }
        if let Some(page) = self.emoji_page.filter(|_| y < ROW_Y[3]) {
            return self.emoji_tap(page, x, y);
        }
        let mut hit = None;
        self.each_key(|k, kx, ky, w| {
            if (kx..kx + w).contains(&x) && (ky..ky + KH).contains(&y) {
                hit = Some(k);
            }
        });
        let Some(k) = hit else { return Act::None };
        match k {
            Key::Action if !self.lines_mode() => Act::Submit,
            Key::Dial => Act::Dial,
            Key::Emoji => {
                self.emoji_page = if self.emoji_page.is_some() { None } else { Some(0) };
                Act::Redraw
            }
            Key::Sym => {
                self.sym = !self.sym;
                Act::Redraw
            }
            Key::Shift => {
                self.shift = !self.shift;
                Act::Redraw
            }
            _ => {
                let was = self.shift;
                if k == Key::Del {
                    self.del.start(now);
                }
                if !self.type_key(k) {
                    return Act::None;
                }
                // the letters change case: all of them drawn again
                if was != self.shift && self.emoji_page.is_none() && !self.number() { Act::Redraw } else { Act::Typed(k) }
            }
        }
    }

    /// TOUCH_UP: a held delete stops.
    pub fn release(&mut self) {
        self.del.stop();
    }

    /// ms until a held delete repeats, for the event timeout.
    pub fn hold_wait(&self, now: u32) -> Option<u32> {
        self.del.wait(now)
    }

    /// On each pass of the loop: delete held long enough deletes again (`Typed(Del)`, or `Redraw` when the letters
    /// change case); with nothing left it stops.
    pub fn tick(&mut self, now: u32) -> Act {
        if !self.del.due(now) {
            return Act::None;
        }
        let was = self.shift;
        if !self.type_key(Key::Del) {
            self.del.stop();
            return Act::None;
        }
        if was != self.shift && self.emoji_page.is_none() && !self.number() { Act::Redraw } else { Act::Typed(Key::Del) }
    }

    fn emoji_tap(&mut self, page: usize, x: i32, y: i32) -> Act {
        let x0 = C - EM_CELL * EM_COLS as i32 / 2;
        let (col, row) = ((x - x0).div_euclid(EM_CELL), (y - EM_TOP).div_euclid(EM_CELL));
        if !(0..EM_COLS as i32).contains(&col) || !(0..EM_ROWS as i32).contains(&row) {
            return Act::None;
        }
        let i = page * EM_PAGE + row as usize * EM_COLS + col as usize;
        let Some(ch) = emoji::all().get(i).and_then(|&cp| char::from_u32(cp)) else { return Act::None };
        let mut b = [0u8; 4];
        let b = ch.encode_utf8(&mut b).as_bytes();
        if self.n + b.len() > TEXT_MAX {
            return Act::None;
        }
        self.text[self.n..self.n + b.len()].copy_from_slice(b);
        self.n += b.len();
        self.shift = false;
        self.scroll = 0;
        Act::Text
    }

    /// A crown turn: the emoji pages (`Redraw` when the page changed), else the text scrolls a line (`Text`; down
    /// is towards the caret, where typing goes back to).
    pub fn crown(&mut self, d: i32) -> Act {
        let Some(page) = self.emoji_page else {
            let most = self.line_count(TEXT_PX).saturating_sub(TEXT_ROWS);
            let to = (self.scroll as i32 - d.signum()).clamp(0, most as i32) as usize;
            if to == self.scroll {
                return Act::None;
            }
            self.scroll = to;
            return Act::Text;
        };
        let p = (page as i32 + d.signum()).clamp(0, self.em_pages() as i32 - 1) as usize;
        if p == page {
            return Act::None;
        }
        self.emoji_page = Some(p);
        Act::Redraw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_at(kb: &Keyboard, want: Key) -> (i32, i32) {
        let mut at = None;
        kb.each_key(|k, x, y, w| {
            if k == want {
                at = Some((x + w / 2, y + KH / 2));
            }
        });
        at.expect("key shown")
    }

    #[test]
    fn rows_fit_the_round_panel() {
        let kb = Keyboard::new(WHITE);
        kb.each_key(|_, x, y, w| {
            let (x0, x1) = chord(y, y + KH, 0);
            assert!(x >= x0 && x + w <= x1, "key at ({x}, {y}) w {w} outside the circle ({x0}..{x1})");
        });
    }

    #[test]
    fn typing_capitalises_the_first_letter_and_after_a_sentence() {
        let mut kb = Keyboard::new(WHITE);
        for k in [Key::Ch(b'h'), Key::Ch(b'i'), Key::Ch(b'.'), Key::Space, Key::Ch(b'o'), Key::Ch(b'k')] {
            kb.type_key(k);
        }
        assert_eq!(kb.text(), "Hi. Ok");
        kb.type_key(Key::Del);
        assert_eq!(kb.text(), "Hi. O");
    }

    #[test]
    fn taps_type_and_switch() {
        let mut kb = Keyboard::new(WHITE);
        let (x, y) = key_at(&kb, Key::Ch(b'q'));
        // the first letter changes the case of all of them
        assert_eq!(kb.tap(x, y, 0), Act::Redraw);
        let (x, y) = key_at(&kb, Key::Ch(b'w'));
        assert_eq!(kb.tap(x, y, 0), Act::Typed(Key::Ch(b'w')));
        assert_eq!(kb.text(), "Qw");
        let (x, y) = key_at(&kb, Key::Sym);
        assert_eq!(kb.tap(x, y, 0), Act::Redraw);
        assert!(kb.sym);
        let (x, y) = key_at(&kb, Key::Dial);
        assert_eq!(kb.tap(x, y, 0), Act::Dial);
        let (x, y) = key_at(&kb, Key::Action);
        assert_eq!(kb.tap(x, y, 0), Act::Submit);
        assert_eq!(kb.tap(MIC_X, MIC_Y, 0), Act::Voice);
    }

    #[test]
    fn delete_held_repeats_until_lifted() {
        let mut kb = Keyboard::new(WHITE);
        kb.add_words("abcdef");
        let (x, y) = key_at(&kb, Key::Del);
        assert_eq!(kb.tap(x, y, 1000), Act::Typed(Key::Del));
        assert_eq!(kb.text(), "abcde");
        assert_eq!(kb.tick(1100), Act::None, "a tap deletes one");
        assert_eq!(kb.hold_wait(1100), Some(crate::repeat::DELAY_MS - 100));
        assert_eq!(kb.tick(1000 + crate::repeat::DELAY_MS), Act::Typed(Key::Del));
        assert_eq!(kb.text(), "abcd");
        assert_eq!(kb.tick(1000 + crate::repeat::DELAY_MS + crate::repeat::EVERY_MS), Act::Typed(Key::Del));
        kb.release();
        assert_eq!(kb.tick(10_000), Act::None, "lifted: stops at once");
        assert_eq!(kb.text(), "abc");
        assert_eq!(kb.hold_wait(10_000), None);
    }

    #[test]
    fn delete_takes_a_whole_emoji() {
        let mut kb = Keyboard::new(WHITE);
        kb.add_words("ok \u{1F603}");
        assert!(kb.type_key(Key::Del));
        assert_eq!(kb.text(), "ok ");
    }

    #[test]
    fn number_fields_take_numbers_only() {
        let mut kb = Keyboard::new(WHITE);
        kb.input = Input::Decimal { signed: true };
        for k in [Key::Ch(b'.'), Key::Minus, Key::Ch(b'1'), Key::Ch(b'.'), Key::Ch(b'5'), Key::Minus, Key::Space] {
            kb.type_key(k);
        }
        assert_eq!(kb.text(), ".15", "minus only first, one point, no space");
        kb.clear();
        assert!(kb.type_key(Key::Minus));
        kb.add_words("4.5 kg");
        assert_eq!(kb.text(), "-4.5");
        kb.clear();
        kb.input = Input::Integer { signed: false };
        assert!(!kb.type_key(Key::Minus));
        assert!(!kb.type_key(Key::Ch(b'.')));
        // the pad: digits, delete, the action; no minus or point, no mic
        let mut keys = std::vec::Vec::new();
        kb.each_key(|k, _, _, _| keys.push(k));
        assert_eq!(keys.len(), 12);
        assert!(!keys.contains(&Key::Minus) && !keys.contains(&Key::Ch(b'.')));
        assert!(keys.contains(&Key::Del) && keys.contains(&Key::Action) && keys.contains(&Key::Ch(b'0')));
        assert_eq!(kb.tap(MIC_X, MIC_Y, 0), Act::None);
        let (x, y) = key_at(&kb, Key::Ch(b'7'));
        assert_eq!(kb.tap(x, y, 0), Act::Typed(Key::Ch(b'7')));
        let (x, y) = key_at(&kb, Key::Action);
        assert_eq!(kb.tap(x, y, 0), Act::Submit);
        kb.input = Input::Decimal { signed: true };
        kb.each_key(|k, x, y, w| {
            let (x0, x1) = chord(y, y + KH, 0);
            assert!(x >= x0 && x + w <= x1, "{k:?} outside the circle");
        });
    }

    #[test]
    fn multiline_types_new_lines_and_submits_beside_the_text() {
        let mut kb = Keyboard::new(WHITE);
        kb.multiline = true;
        kb.action = Action::Done;
        kb.type_key(Key::Ch(b'a'));
        let (x, y) = key_at(&kb, Key::Action);
        assert_eq!(kb.tap(x, y, 0), Act::Redraw, "a new line: a capital next");
        assert_eq!(kb.text(), "A\n");
        assert_eq!(kb.line_count(TEXT_PX), 2);
        assert_eq!(kb.tap(SUBMIT_X, MIC_Y, 0), Act::Submit);
    }

    #[test]
    fn crown_scrolls_the_text_and_typing_goes_back_to_the_caret() {
        let mut kb = Keyboard::new(WHITE);
        kb.multiline = true;
        for c in b"abcde" {
            kb.type_key(Key::Ch(*c));
            kb.type_key(Key::Action);
        }
        // 6 lines, 3 shown: 3 back at most
        assert_eq!(kb.crown(1), Act::None, "at the caret already");
        assert_eq!(kb.crown(-1), Act::Text);
        assert_eq!(kb.crown(-1), Act::Text);
        assert_eq!(kb.crown(-1), Act::Text);
        assert_eq!(kb.crown(-1), Act::None);
        assert_eq!(kb.scroll, 3);
        let mut px = vec![0u16; (crate::SIZE * crate::SIZE) as usize];
        let mut fb = crate::Fb::new(&mut px);
        kb.draw_text(&mut fb);
        kb.type_key(Key::Ch(b'f'));
        assert_eq!(kb.scroll, 0);
    }

    #[test]
    fn the_text_box_clears_only_itself() {
        let mut px = vec![0u16; (crate::SIZE * crate::SIZE) as usize];
        let mut fb = crate::Fb::new(&mut px);
        fb.rect(0, 0, crate::SIZE, crate::SIZE, 0x1234);
        let mut kb = Keyboard::new(WHITE);
        kb.add_words("a long enough text to take more than one line of the box");
        kb.draw_text_in(&mut fb, crate::dial::TEXT_Y, crate::dial::TEXT_ROWS, crate::dial::TEXT_PX);
        let (x0, x1) = (C - crate::dial::TEXT_PX / 2 - 6, C + crate::dial::TEXT_PX / 2 + 6);
        assert_eq!(fb.at(x0 - 1, crate::dial::TEXT_Y + 10), 0x1234);
        assert_eq!(fb.at(x1 + 1, crate::dial::TEXT_Y + 10), 0x1234);
        assert_eq!(fb.at(C, crate::dial::TEXT_Y - 7), 0x1234);
        assert_eq!(fb.at(x0, crate::dial::TEXT_Y - 6), 0);
    }

    #[test]
    fn no_emoji_key_without_the_watch_font() {
        let kb = Keyboard::new(WHITE);
        let mut keys = 0;
        kb.each_key(|k, _, _, _| keys += (k == Key::Emoji) as i32);
        assert_eq!(keys, 0);
    }

    #[test]
    fn words_go_after_the_text_with_a_space() {
        let mut kb = Keyboard::new(WHITE);
        kb.type_key(Key::Ch(b'a'));
        kb.add_words("  coffee at ten ");
        assert_eq!(kb.text(), "A coffee at ten");
        let long = "x".repeat(200);
        kb.add_words(&long);
        assert_eq!(kb.text().len(), TEXT_MAX);
    }

    #[test]
    fn draws_without_panicking() {
        let mut px = vec![0u16; (crate::SIZE * crate::SIZE) as usize];
        let mut fb = crate::Fb::new(&mut px);
        let mut kb = Keyboard::new(WHITE);
        kb.draw(&mut fb, "Keyboards");
        kb.add_words("hello there");
        kb.draw_text(&mut fb);
        let (x, y) = key_at(&kb, Key::Ch(b'q'));
        assert_ne!(fb.at(x - 12, y), 0, "the keys are drawn");
        kb.draw_key(&mut fb, Key::Ch(b'q'), true);
        assert_eq!(fb.at(x - 12, y), 0xFFFF, "a pressed key is white");
    }
}
