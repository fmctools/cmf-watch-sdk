//! Voice typing for any app with a keyboard: the watch's mic ([`cmf_wp2_app::mic::Voice`], 8 kHz with its gain) as
//! IMA ADPCM to the phone, which writes it down (its speech recognition, docs/keyboards.md) and sends the words back. One screen,
//! drawn over the app's: what is heard so far, the level, ✕ cancel and ✓ done.
//!
//! Messages (`~` in every app, no app uses it otherwise):
//!   watch → phone: `~1` start dictating, `~3` start a recording (saved on the phone), `~a seq:u16 block` one block
//!   of 240 samples (3 + 120 bytes), `~0` the end, `~2` cancel.
//!   phone → watch: `~ kind text`: kind 0 words so far, 1 the final words, 2 a problem (said on the screen).
//!
//! An app: a mic key calls [`Dictate::start`]; while [`Dictate::active`] its loop waits at most [`Dictate::wait_ms`]
//! and calls [`Dictate::tick`], gives every message to [`Dictate::message`] first, touches to [`Dictate::tap`],
//! the button to [`Dictate::cancel`], and draws [`Dictate::draw`] when told to. [`Out::Done`]: the words are in
//! [`Dictate::text`].

use crate::color::{Rgb, BLACK, WHITE};
use crate::draw::{disc, ring};
use crate::font;
use crate::Surface;
use cmf_wp2_app::adpcm;
use cmf_wp2_app::mic::Voice;
use cmf_wp2_app::Api;

const BLOCK: usize = 240;
const MSG: usize = 4 + adpcm::BLOCK_HEADER + BLOCK / 2;
/// blocks waiting for the outbox: 0.36 s (the phone collects them about every 40 ms)
const QUEUE: usize = 12;
const LISTEN_MAX_MS: u32 = 30_000;
/// A recording: 30 minutes at most.
const RECORD_MAX_MS: u32 = 30 * 60_000;
const ANSWER_MAX_MS: u32 = 10_000;
const TEXT_MAX: usize = 124;

const RED: Rgb = Rgb(0xE5, 0x1A, 0x22);
const GREY: Rgb = Rgb(0x8A, 0x8A, 0x8A);
const CARD: Rgb = Rgb(0x26, 0x26, 0x26);
const C: i32 = 233;
const MIC_Y: i32 = 176;
const BTN_Y: i32 = 392;
const NO_X: i32 = 163;
const OK_X: i32 = 303;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Phase {
    Off,
    Listening,
    /// the end sent: the phone's final words to come
    Waiting,
    /// a problem said on the screen; any tap goes back
    Failed,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Out {
    None,
    /// draw the screen again ([`Dictate::draw`])
    Redraw,
    /// the words are in [`Dictate::text`]; the app's own screen again
    Done,
    /// nothing typed; the app's own screen again
    Cancel,
}

pub struct Dictate {
    pub phase: Phase,
    voice: Option<Voice>,
    enc: Enc,
    text: [u8; TEXT_MAX],
    text_n: usize,
    since: u32,
    level: i32,
    drawn_level: i32,
    shown_at: u32,
    /// a recording (the phone keeps the sound): the screen says so
    record: bool,
    /// a recording's seconds as shown
    secs: u32,
}

impl Default for Dictate {
    fn default() -> Self {
        Self::new()
    }
}

impl Dictate {
    pub const fn new() -> Self {
        Dictate {
            phase: Phase::Off,
            voice: None,
            enc: Enc::new(),
            text: [0; TEXT_MAX],
            text_n: 0,
            since: 0,
            level: 0,
            drawn_level: -1,
            shown_at: 0,
            record: false,
            secs: 0,
        }
    }

    pub fn active(&self) -> bool {
        self.phase != Phase::Off
    }

    /// How long the app's loop may wait for an event: the mic is read every 20 ms.
    pub fn wait_ms(&self, other: u32) -> u32 {
        match self.phase {
            Phase::Listening | Phase::Waiting => other.min(20),
            _ => other,
        }
    }

    /// The words: the final ones after [`Out::Done`], else what is heard so far.
    pub fn text(&self) -> &str {
        core::str::from_utf8(&self.text[..self.text_n]).unwrap_or("")
    }

    /// Starts listening (`record`: a recording the phone keeps, else dictation). Draw the screen after it.
    pub fn start(&mut self, api: &Api, record: bool) {
        *self = Self::new();
        self.record = record;
        self.since = api.now();
        if api.send(if record { b"~3" } else { b"~1" }) == -107 {
            return self.fail("No phone connected");
        }
        let (v, rc) = Voice::start(api);
        if rc < 0 {
            api.send(b"~2");
            return self.fail("The mic needs App Loader 6");
        }
        self.voice = Some(v);
        self.phase = Phase::Listening;
    }

    fn fail(&mut self, why: &str) {
        self.set_text(why.as_bytes());
        self.phase = Phase::Failed;
    }

    fn set_text(&mut self, t: &[u8]) {
        // the newest words: the end of a long text
        let mut from = t.len().saturating_sub(TEXT_MAX);
        while from < t.len() && (t[from] & 0xC0) == 0x80 {
            from += 1;
        }
        self.text_n = t.len() - from;
        self.text[..self.text_n].copy_from_slice(&t[from..]);
    }

    fn end(&mut self, api: &Api) {
        if let Some(mut v) = self.voice.take() {
            let enc = &mut self.enc;
            v.pump(api, |x| enc.sample(x));
            Voice::stop(api);
        }
        self.enc.seal();
        self.enc.ending = true;
        self.enc.flush(api);
        self.phase = Phase::Waiting;
        self.since = api.now();
    }

    /// The mic read, blocks sent, time limits; every 20 ms while active.
    pub fn tick(&mut self, api: &Api) -> Out {
        match self.phase {
            Phase::Listening => {
                if let Some(mut v) = self.voice.take() {
                    let enc = &mut self.enc;
                    let rc = v.pump(api, |x| enc.sample(x));
                    self.level = self.level.max(v.take_level());
                    self.voice = Some(v);
                    if rc < 0 {
                        Voice::stop(api);
                        self.voice = None;
                        api.send(b"~2");
                        self.fail("The mic did not start");
                        return Out::Redraw;
                    }
                }
                self.enc.flush(api);
                let el = api.now().wrapping_sub(self.since);
                if el > if self.record { RECORD_MAX_MS } else { LISTEN_MAX_MS } {
                    self.end(api);
                    return Out::Redraw;
                }
                if self.record && el / 1000 != self.secs {
                    // the time recorded, each second
                    self.secs = el / 1000;
                    return Out::Redraw;
                }
                // the level ring, 8 times a second
                if api.now().wrapping_sub(self.shown_at) > 120 {
                    self.shown_at = api.now();
                    let l = core::mem::take(&mut self.level);
                    if (l - self.drawn_level).abs() > 300 {
                        self.drawn_level = l;
                        return Out::Redraw;
                    }
                }
                Out::None
            }
            Phase::Waiting => {
                self.enc.flush(api);
                if api.now().wrapping_sub(self.since) > ANSWER_MAX_MS {
                    self.phase = Phase::Off;
                    // what was heard, if anything
                    return if self.text_n > 0 { Out::Done } else { Out::Cancel };
                }
                Out::None
            }
            _ => Out::None,
        }
    }

    /// A message from the phone: `Some` if it was one of ours (`~`), with what the app should do.
    pub fn message(&mut self, m: &[u8]) -> Option<Out> {
        let [b'~', kind, ref text @ ..] = *m else { return None };
        if self.phase == Phase::Off {
            return Some(Out::None);
        }
        match kind {
            0 => {
                if self.phase == Phase::Failed {
                    return Some(Out::None);
                }
                self.set_text(text);
                Some(Out::Redraw)
            }
            1 => {
                self.set_text(text);
                if self.phase == Phase::Waiting {
                    self.phase = Phase::Off;
                    Some(Out::Done)
                } else {
                    // a final before the end (the phone gave up early): shown, kept for the end
                    Some(Out::Redraw)
                }
            }
            _ => {
                self.set_text(text);
                self.phase = Phase::Failed;
                Some(Out::Redraw)
            }
        }
    }

    /// The side button: cancel.
    pub fn cancel(&mut self, api: &Api) -> Out {
        if self.voice.take().is_some() {
            Voice::stop(api);
        }
        if matches!(self.phase, Phase::Listening | Phase::Waiting) {
            api.send(b"~2");
        }
        self.phase = Phase::Off;
        Out::Cancel
    }

    pub fn tap(&mut self, api: &Api, x: i32, y: i32) -> Out {
        match self.phase {
            Phase::Failed => {
                self.phase = Phase::Off;
                Out::Cancel
            }
            Phase::Listening => {
                if (y - BTN_Y).abs() < 40 && (x - NO_X).abs() < 40 {
                    return self.cancel(api);
                }
                // ✓ or the mic itself: done talking
                if ((y - BTN_Y).abs() < 40 && (x - OK_X).abs() < 40) || ((x - C).abs() < 70 && (y - MIC_Y).abs() < 70) {
                    self.end(api);
                    return Out::Redraw;
                }
                Out::None
            }
            Phase::Waiting if (y - BTN_Y).abs() < 40 && (x - NO_X).abs() < 40 => self.cancel(api),
            _ => Out::None,
        }
    }

    /// The whole screen.
    pub fn draw(&self, s: &mut impl Surface) {
        s.rect(0, 0, 466, 466, 0);
        let title = match self.phase {
            Phase::Listening if self.record => "Recording",
            Phase::Listening => "Listening",
            Phase::Waiting => "Writing it down",
            _ => "Voice typing",
        };
        if self.record && self.phase == Phase::Listening {
            // Recording 1:07, the time in red
            let mut b = [0u8; 8];
            let (m, sec) = (self.secs / 60, self.secs % 60);
            let mut k = 0;
            if m >= 10 {
                b[k] = b'0' + (m / 10 % 10) as u8;
                k += 1;
            }
            b[k] = b'0' + (m % 10) as u8;
            b[k + 1] = b':';
            b[k + 2] = b'0' + (sec / 10) as u8;
            b[k + 3] = b'0' + (sec % 10) as u8;
            let t = core::str::from_utf8(&b[..k + 4]).unwrap_or("");
            let w = font::width(title) + 10 + font::width(t);
            let x = font::draw(s, C - w / 2, 52, title, GREY.rgb565());
            font::draw(s, x + 10, 52, t, RED.rgb565());
        } else {
            font::centre(s, C, 52, title, GREY.rgb565());
        }
        // the mic: a ring that grows with the voice
        let lv = if self.phase == Phase::Listening { (self.drawn_level.max(0) * 40 / 32768).min(40) } else { 0 };
        if lv > 2 {
            ring(s, C, MIC_Y, 52, 52 + lv, RED.scale(96), BLACK);
        }
        disc(s, C, MIC_Y, 50, if self.phase == Phase::Failed { CARD } else { RED }, BLACK);
        mic_icon(s, C, MIC_Y, WHITE.rgb565(), 2);
        // the words
        let t = self.text();
        let ink = if self.phase == Phase::Failed { RED.rgb565() } else { WHITE.rgb565() };
        if t.is_empty() && self.phase == Phase::Listening {
            font::centre(s, C, 262, "Speak now", GREY.rgb565());
        } else {
            let mut lines = [""; 3];
            let n = font::wrap(t, 340, &mut lines);
            for (i, l) in lines[..n].iter().enumerate() {
                let (l, _) = font::clip(l, 340);
                font::centre(s, C, 250 + i as i32 * 28, l, ink);
            }
        }
        match self.phase {
            Phase::Listening => {
                button(s, NO_X, false);
                button(s, OK_X, true);
            }
            Phase::Waiting => button(s, NO_X, false),
            _ => font::centre(s, C, BTN_Y - 10, "Tap to go back", GREY.rgb565()),
        }
    }
}

/// The sound on its way: 240-sample ADPCM blocks queued for the outbox.
struct Enc {
    st: adpcm::State,
    block: [i16; BLOCK],
    n: usize,
    seq: u16,
    queue: [[u8; MSG]; QUEUE],
    q_len: [u8; QUEUE],
    q_head: usize,
    q_n: usize,
    /// `~0` still to send, after every block
    ending: bool,
}

impl Enc {
    const fn new() -> Enc {
        Enc {
            st: adpcm::State { predictor: 0, index: 0 },
            block: [0; BLOCK],
            n: 0,
            seq: 0,
            queue: [[0; MSG]; QUEUE],
            q_len: [0; QUEUE],
            q_head: 0,
            q_n: 0,
            ending: false,
        }
    }

    fn sample(&mut self, x: i16) {
        self.block[self.n] = x;
        self.n += 1;
        if self.n == BLOCK {
            self.seal();
        }
    }

    fn push(&mut self, m: &[u8]) -> bool {
        if self.q_n == QUEUE {
            return false;
        }
        let i = (self.q_head + self.q_n) % QUEUE;
        self.queue[i][..m.len()].copy_from_slice(m);
        self.q_len[i] = m.len() as u8;
        self.q_n += 1;
        true
    }

    fn flush(&mut self, api: &Api) {
        while self.q_n > 0 {
            let i = self.q_head;
            if api.send(&self.queue[i][..usize::from(self.q_len[i])]) == -11 {
                return; // the outbox is full: the phone takes it soon
            }
            self.q_head = (i + 1) % QUEUE;
            self.q_n -= 1;
        }
        if self.ending && api.send(b"~0") != -11 {
            self.ending = false;
        }
    }

    fn seal(&mut self) {
        if self.n < 2 {
            return;
        }
        let k = self.n & !1;
        let mut m = [0u8; MSG];
        m[0] = b'~';
        m[1] = b'a';
        m[2..4].copy_from_slice(&self.seq.to_le_bytes());
        let len = 4 + adpcm::encode_block(&mut self.st, &self.block[..k], &mut m[4..]);
        // a full queue drops the block: the phone hears a gap, the watch never stalls
        self.push(&m[..len]);
        self.seq = self.seq.wrapping_add(1);
        self.n = 0;
    }
}

fn button(s: &mut impl Surface, x: i32, ok: bool) {
    disc(s, x, BTN_Y, 30, if ok { WHITE } else { CARD }, BLACK);
    if ok {
        // ✓
        crate::draw::line(s, x - 12, BTN_Y + 1, x - 4, BTN_Y + 9, 4, BLACK, WHITE);
        crate::draw::line(s, x - 4, BTN_Y + 9, x + 12, BTN_Y - 9, 4, BLACK, WHITE);
    } else {
        // ✕
        crate::draw::line(s, x - 10, BTN_Y - 10, x + 10, BTN_Y + 10, 4, WHITE, CARD);
        crate::draw::line(s, x - 10, BTN_Y + 10, x + 10, BTN_Y - 10, 4, WHITE, CARD);
    }
}

/// The round mic button an app puts beside its typed text (radius 22).
pub fn mic_button(s: &mut impl Surface, cx: i32, cy: i32) {
    disc(s, cx, cy, 22, CARD, BLACK);
    mic_icon(s, cx, cy + 1, WHITE.rgb565(), 1);
}

/// A tap on that button (a little bigger than drawn).
pub fn mic_hit(cx: i32, cy: i32, x: i32, y: i32) -> bool {
    (x - cx) * (x - cx) + (y - cy) * (y - cy) <= 32 * 32
}

/// A microphone glyph centred on (cx, cy), `k` times its 22 px key size: for an app's mic key.
pub fn mic_icon(s: &mut impl Surface, cx: i32, cy: i32, c: u16, k: i32) {
    // the capsule
    let (w, h) = (8 * k, 13 * k);
    let top = cy - 11 * k;
    s.rect(cx - w / 2 + k, top, w - 2 * k, h, c);
    s.rect(cx - w / 2, top + k, w, h - 2 * k, c);
    // the cradle: a U under it
    let (u, t) = (6 * k, (2 * k).max(2));
    s.rect(cx - u - t, cy - 3 * k, t, 5 * k, c);
    s.rect(cx + u, cy - 3 * k, t, 5 * k, c);
    s.rect(cx - u - t + k, cy + 2 * k, 2 * u + 2 * t - 2 * k, t, c);
    // the stand
    s.rect(cx - t / 2, cy + 2 * k + t, t, 4 * k, c);
    s.rect(cx - 4 * k, cy + 6 * k + t, 8 * k, t, c);
}
