// Output: the panel mask, PNG frames, the JSON lines log, local time.
use cmf_wp2_app::Time;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;
use wp2_core::gfx::{H, W};

pub fn rgb(c: u16) -> [u8; 3] {
    let (r, g, b) = (c >> 11, (c >> 5) & 63, c & 31);
    [(r << 3 | r >> 2) as u8, (g << 2 | g >> 4) as u8, (b << 3 | b >> 2) as u8]
}

/// The round panel: pixel centres inside the 466 px circle.
pub fn on_panel(x: usize, y: usize) -> bool {
    let (dx, dy) = (2 * x as i64 + 1 - W as i64, 2 * y as i64 + 1 - H as i64);
    dx * dx + dy * dy <= (W * W) as i64
}

pub fn png(path: &Path, px: &[u16]) -> std::io::Result<()> {
    let mut data = Vec::with_capacity(W * H * 3);
    for (i, &c) in px.iter().enumerate() {
        data.extend(if on_panel(i % W, i / W) { rgb(c) } else { [0; 3] });
    }
    let mut enc = png::Encoder::new(BufWriter::new(fs::File::create(path)?), W as u32, H as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    // unfiltered rows: tests read the pixels with zlib alone
    enc.set_filter(png::Filter::NoFilter);
    enc.write_header()?.write_image_data(&data)?;
    Ok(())
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    (s.len() % 2 == 0 && s.is_ascii()).then_some(())?;
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

/// UTF-8 as is, anything else as `hex:…`
pub fn shown(b: &[u8]) -> String {
    std::str::from_utf8(b).map_or_else(|_| format!("hex:{}", hex(b)), |s| s.escape_debug().to_string())
}

pub fn data(b: &[u8]) -> (&'static str, V<'static>) {
    match std::str::from_utf8(b) {
        Ok(s) => ("text", V::O(s.to_owned())),
        Err(_) => ("hex", V::O(hex(b))),
    }
}

pub enum V<'a> {
    I(i64),
    F(f64),
    B(bool),
    S(&'a str),
    O(String),
}

fn esc(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn json(t: u32, ev: &str, fields: &[(&str, V)]) -> String {
    let mut s = format!("{{\"t\":{t},\"ev\":");
    esc(ev, &mut s);
    for (k, v) in fields {
        s.push(',');
        esc(k, &mut s);
        s.push(':');
        match v {
            V::I(i) => s += &i.to_string(),
            V::F(f) => s += &format!("{f:.1}"),
            V::B(b) => s += if *b { "true" } else { "false" },
            V::S(x) => esc(x, &mut s),
            V::O(x) => esc(x, &mut s),
        }
    }
    s.push('}');
    s
}

/// JSON lines, flushed per record so an abort keeps them.
pub struct Log(Option<Box<dyn Write + Send>>);

impl Log {
    pub fn open(path: Option<&Path>) -> Log {
        Log(path.and_then(|p| -> Option<Box<dyn Write + Send>> {
            if p == Path::new("-") {
                return Some(Box::new(std::io::stdout()));
            }
            if let Some(d) = p.parent() {
                let _ = fs::create_dir_all(d);
            }
            match fs::File::create(p) {
                Ok(f) => Some(Box::new(f)),
                Err(e) => {
                    eprintln!("wp2-sim: log {}: {e}", p.display());
                    None
                }
            }
        }))
    }

    pub fn rec(&mut self, t: u32, ev: &str, fields: &[(&str, V)]) {
        if let Some(w) = self.0.as_mut() {
            let _ = writeln!(w, "{}", json(t, ev, fields));
            let _ = w.flush();
        }
    }
}

#[cfg(not(windows))]
#[repr(C)]
struct Tm {
    sec: i32,
    min: i32,
    hour: i32,
    mday: i32,
    mon: i32,
    year: i32,
    wday: i32,
    yday: i32,
    isdst: i32,
    gmtoff: std::ffi::c_long,
    zone: *const std::ffi::c_char,
}

#[cfg(not(windows))]
unsafe extern "C" {
    fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
}

/// Local time (zone from TZ) of `ms` since the epoch.
#[cfg(not(windows))]
pub fn local(ms: i64) -> Option<Time> {
    let secs = ms.div_euclid(1000);
    let mut tm =
        Tm { sec: 0, min: 0, hour: 0, mday: 0, mon: 0, year: 0, wday: 0, yday: 0, isdst: 0, gmtoff: 0, zone: std::ptr::null() };
    if unsafe { localtime_r(&secs, &mut tm) }.is_null() {
        return None;
    }
    Some(Time {
        year: (tm.year + 1900) as u16,
        month: (tm.mon + 1) as u8,
        day: tm.mday as u8,
        hour: tm.hour as u8,
        minute: tm.min as u8,
        second: tm.sec as u8,
        weekday: tm.wday as u8,
        millis: ms.rem_euclid(1000) as u16,
        utc_offset_min: (tm.gmtoff / 60) as i16,
    })
}

// The UCRT's struct tm is nine ints (no tm_gmtoff/tm_zone), localtime_s takes the pointers the other way
// round, and the zone offset comes from _get_timezone: the Mac path above does not link on Windows.
#[cfg(windows)]
#[repr(C)]
struct Tm {
    sec: i32,
    min: i32,
    hour: i32,
    mday: i32,
    mon: i32,
    year: i32,
    wday: i32,
    yday: i32,
    isdst: i32,
}

#[cfg(windows)]
unsafe extern "C" {
    // UCRT: localtime_s is a header macro over this, and _get_dstflag does not exist
    fn _localtime64_s(out: *mut Tm, t: *const i64) -> i32;
    fn GetTimeZoneInformation(zone: *mut TimeZoneInfo) -> u32;
}

#[cfg(windows)]
#[repr(C)]
struct TimeZoneInfo {
    bias: i32,
    standard_name: [u16; 32],
    standard_date: [u16; 8],
    standard_bias: i32,
    daylight_name: [u16; 32],
    daylight_date: [u16; 8],
    daylight_bias: i32,
}

/// Local time (zone from TZ) of `ms` since the epoch, Windows edition.
#[cfg(windows)]
pub fn local(ms: i64) -> Option<Time> {
    let secs = ms.div_euclid(1000);
    let mut tm = Tm {
        sec: 0, min: 0, hour: 0, mday: 0, mon: 0, year: 0, wday: 0, yday: 0, isdst: 0,
    };
    if unsafe { _localtime64_s(&mut tm, &secs) } != 0 {
        return None;
    }
    let mut zone = TimeZoneInfo {
        bias: 0, standard_name: [0; 32], standard_date: [0; 8], standard_bias: 0,
        daylight_name: [0; 32], daylight_date: [0; 8], daylight_bias: 0,
    };
    // 1 = standard time in effect, 2 = daylight time, 0 = failure (offset then stays 0)
    let state = unsafe { GetTimeZoneInformation(&mut zone) };
    let bias = zone.bias + if state == 2 { zone.daylight_bias } else { 0 };
    Some(Time {
        year: (tm.year + 1900) as u16,
        month: (tm.mon + 1) as u8,
        day: tm.mday as u8,
        hour: tm.hour as u8,
        minute: tm.min as u8,
        second: tm.sec as u8,
        weekday: tm.wday as u8,
        millis: ms.rem_euclid(1000) as u16,
        utc_offset_min: (-bias / 60) as i16,
    })
}
