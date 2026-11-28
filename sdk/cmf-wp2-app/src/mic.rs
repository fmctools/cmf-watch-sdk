//! Speech from the mic ([`Api::mic_open`]) as 8 kHz mono: what voice typing and voice notes send to the phone (IMA
//! ADPCM, 4 KB/s). The loader passes on what the stock recorder makes, 16 kHz mono (as the emulator gives it) or
//! two channels (as the stock's factory test splits it): the first 400 ms tell them apart by the byte rate, then
//! channel 0 is taken and each two samples averaged. An automatic gain brings speech to a steady level: the real
//! watch's mic is quiet (speech peaks near 3500 of 32767, measured 2026-11-26).

use crate::Api;

/// The measuring time, whose samples are dropped.
const MEASURE_MS: u32 = 400;
/// 16-bit 16 kHz: 32000 B/s a channel
const STEREO_RATE: u32 = 48000;

pub struct Voice {
    start: u32,
    bytes: u32,
    /// 0 still measuring, else 1 or 2
    pub channels: u8,
    /// samples seen, for the channel of each
    seen: u32,
    pend: Option<i32>,
    /// the loudest sample since `take_level`
    level: i32,
    /// the automatic gain: the speech's envelope (peak, slowly falling) and the gain from it, Q8
    env: i32,
    gain: i32,
}

/// The level the gain brings speech peaks to, and its most (the real watch's mic peaks near 3500 for speech).
const TARGET: i32 = 22000;
const GAIN_MAX: i32 = 12 * 256;

/// One read's worth (4 ms of stereo); `pump` reads until the loader has no more. A static, not the stack: the loader
/// copies only into the app's own block (`mic_read` refuses anything else, -22), and the app's stack is the loader's.
const READ: usize = 256;
static mut RAW: [u8; READ] = [0; READ];

impl Voice {
    pub fn start(api: &Api) -> (Voice, i32) {
        let rc = api.mic_open();
        (Voice { start: api.now(), bytes: 0, channels: 0, seen: 0, pend: None, level: 0, env: 0, gain: GAIN_MAX }, rc)
    }

    pub fn stop(api: &Api) {
        api.mic_close();
    }

    /// Takes what the mic has: each 8 kHz sample to `out`. A negative error of the mic, else the bytes read.
    pub fn pump(&mut self, api: &Api, mut out: impl FnMut(i16)) -> i32 {
        let mut total = 0;
        loop {
            let n = self.pump_once(api, &mut out);
            if n < 0 {
                return if total > 0 { total } else { n };
            }
            total += n;
            if n < READ as i32 {
                return total;
            }
        }
    }

    fn pump_once(&mut self, api: &Api, out: &mut impl FnMut(i16)) -> i32 {
        // one app thread
        let raw = unsafe { &mut *core::ptr::addr_of_mut!(RAW) };
        let n = api.mic_read(raw);
        if n <= 0 {
            return n;
        }
        self.bytes += n as u32;
        if self.channels == 0 {
            let ms = api.now().wrapping_sub(self.start);
            if ms < MEASURE_MS {
                self.seen += n as u32 / 2;
                return n;
            }
            self.channels = if self.bytes * 1000 / ms.max(1) > STEREO_RATE { 2 } else { 1 };
        }
        for s in raw[..n as usize].chunks_exact(2) {
            let ch = self.seen % u32::from(self.channels);
            self.seen = self.seen.wrapping_add(1);
            if ch != 0 {
                continue;
            }
            let v = i32::from(i16::from_le_bytes([s[0], s[1]]));
            self.level = self.level.max(v.abs());
            match self.pend.take() {
                None => self.pend = Some(v),
                Some(a) => out(self.agc((a + v) / 2)),
            }
        }
        n
    }

    /// The automatic gain: quick to come down on a loud sound, slow (about 2 s) to rise again, and it never clips.
    fn agc(&mut self, v: i32) -> i16 {
        let a = v.abs();
        if a > self.env {
            self.env = a;
        } else {
            self.env -= (self.env >> 11) + 1;
            self.env = self.env.max(0);
        }
        let want = if self.env < 64 { GAIN_MAX } else { (TARGET * 256 / self.env).clamp(256, GAIN_MAX) };
        // down at once, up slowly
        self.gain = if want < self.gain { want } else { self.gain + ((want - self.gain) >> 9).max(1).min(want - self.gain) };
        ((v * self.gain) >> 8).clamp(-32768, 32767) as i16
    }

    /// The loudest sample since the last call (before the gain), 0..=32767.
    pub fn take_level(&mut self) -> i32 {
        core::mem::take(&mut self.level)
    }

    /// Bytes a second the mic has given so far.
    pub fn rate(&self, api: &Api) -> u32 {
        self.bytes * 1000 / api.now().wrapping_sub(self.start).max(1)
    }
}
