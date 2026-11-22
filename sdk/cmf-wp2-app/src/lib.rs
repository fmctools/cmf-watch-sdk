#![no_std]

use core::sync::atomic::{AtomicPtr, Ordering};

pub mod lv;
pub mod stock;
pub mod storage;

pub const ABI: u16 = 1;
pub const ABI_MINOR: u16 = 3;
/// The package header's `abi_minor`: `wp2apps pack` reads this symbol from the ELF.
#[unsafe(no_mangle)]
pub static WP2_ABI_MINOR: u16 = ABI_MINOR;

pub const TOUCH_DOWN: u8 = 1;
pub const TOUCH_UP: u8 = 2;
pub const CROWN: u8 = 3;
pub const BUTTON: u8 = 4;
pub const PAUSE: u8 = 5;
pub const RESUME: u8 = 6;
/// ABI 1.1: a host message is in the inbox, `x` = its length; read it with [`Api::recv`].
pub const MESSAGE: u8 = 7;
/// ABI 1.2: an LVGL object event. `x` = the object's handle, `y` = the LVGL event code, `delta` = the value
/// (slider, arc, bar, roller); see [`lv`].
pub const LV: u8 = 8;
/// Heart rate, about once a second after [`Api::heart_rate`]`(true)`: `delta` = beats per minute, 0 = no reading
/// yet or off the wrist; `x` = 1 on the wrist, 0 off. Personal data: do not log it or send it unasked.
pub const HEART: u8 = 9;
/// Phone music state after [`Api::media_bind`], once at the bind and on every change: `x` = 1 playing, `y` = 1 the
/// phone is linked and has media to control (loader of 2026-11-16).
pub const MEDIA: u8 = 10;
/// GPS, about once a second after [`Api::gps`]`(true)`: `x` = fix quality (0 = no fix yet, 1 = GPS fix, 2 =
/// differential), `y` = satellites in use. Read the position with [`Api::gps_fix`] (loader of 2026-11-19).
pub const GPS: u8 = 11;

/// [`Api::media_key`] commands, as the stock music controls send them.
pub const MEDIA_PLAY: u8 = 1;
pub const MEDIA_PAUSE: u8 = 2;
pub const MEDIA_PREV: u8 = 3;
pub const MEDIA_NEXT: u8 = 4;
/// One step of the phone's media volume, as the stock volume popup sends it (its crown sends one per six detents).
/// The loaders of 2026-11-16 to 2026-11-19 answer `-22`: check the result and fall back.
pub const MEDIA_VOLUME_UP: u8 = 5;
pub const MEDIA_VOLUME_DOWN: u8 = 6;

/// SENSOR records for `lv_submit` (docs/abi-1.4.md, "Heart rate"); statics, so they lie in the app block.
#[doc(hidden)]
pub static HEART_ON: &[u8] = &[0x13, 2, 0, 0, 1, 1];
#[doc(hidden)]
pub static HEART_OFF: &[u8] = &[0x13, 2, 0, 0, 1, 0];
#[doc(hidden)]
pub static GPS_ON: &[u8] = &[0x13, 2, 0, 0, 2, 1];
#[doc(hidden)]
pub static GPS_OFF: &[u8] = &[0x13, 2, 0, 0, 2, 0];
#[doc(hidden)]
pub const GPS_FIX_LEN: usize = 20;

/// The latest GPS fix as the watch parsed it (docs/abi-1.4.md, "GPS"). Personal data: show it, record it on the
/// watch if the user asked for that, but never log it or send it unasked.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpsFix {
    /// Latitude and longitude in 1e-7 degrees (north and east positive); 0 without a fix.
    pub lat_e7: i32,
    pub lon_e7: i32,
    /// Altitude above mean sea level in decimetres, as the receiver's GGA gives it.
    pub alt_dm: i32,
    /// `Api.millis` clock when the loader read it.
    pub ms: u32,
    /// GGA fix quality: 0 no fix, 1 GPS, 2 differential.
    pub fix: u8,
    /// Satellites in use.
    pub sats: u8,
    /// Seconds from the GPS start to its first fix, 0 until then.
    pub ttff_s: u16,
}

impl GpsFix {
    #[doc(hidden)]
    pub fn from_bytes(b: &[u8; GPS_FIX_LEN]) -> GpsFix {
        let i = |o: usize| i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        GpsFix {
            lat_e7: i(0),
            lon_e7: i(4),
            alt_dm: i(8),
            ms: i(12) as u32,
            fix: b[16],
            sats: b[17],
            ttff_s: u16::from_le_bytes([b[18], b[19]]),
        }
    }
}

/// Longest message in either direction.
pub const MSG_MAX: usize = 128;
/// Largest blob [`Api::save`] keeps.
pub const STORE_MAX: usize = 4096;

/// `x`, `y`: touch point for `TOUCH_DOWN`/`TOUCH_UP`. `delta`: for `CROWN`, the direction of one detent,
/// `+1` or `-1` per event, never a magnitude (a fast turn is several events); 0 otherwise.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Event {
    pub kind: u8,
    pub _pad: u8,
    pub x: i16,
    pub y: i16,
    pub delta: i16,
}

#[repr(C)]
pub struct Api {
    pub abi: u16,
    pub abi_minor: u16,
    pub width: u16,
    pub height: u16,
    pub fill_rect: extern "C" fn(x: i16, y: i16, w: u16, h: u16, rgb565: u16),
    pub draw_text: extern "C" fn(x: i16, y: i16, text: *const u8, len: usize, rgb565: u16),
    pub present: extern "C" fn(),
    pub next_event: extern "C" fn(event: *mut Event, timeout_ms: u32) -> bool,
    pub motor: extern "C" fn(duty_permille: u16, freq_hz: u16) -> i32,
    pub millis: extern "C" fn() -> u32,
    pub exit: extern "C" fn(code: i32) -> !,
    // ABI 1.1 (docs/abi-1.1.md)
    pub send_msg: extern "C" fn(data: *const u8, len: usize) -> i32,
    pub recv_msg: extern "C" fn(buf: *mut u8, cap: usize) -> i32,
    pub load_data: extern "C" fn(buf: *mut u8, cap: usize) -> i32,
    pub save_data: extern "C" fn(data: *const u8, len: usize) -> i32,
    pub get_time: extern "C" fn(out: *mut Time) -> i32,
    pub get_battery: extern "C" fn() -> i32,
    pub screen_on: extern "C" fn(on: bool) -> i32,
    // ABI 1.2 (docs/specs/2026-11-11-lvgl-apps-design.md): never read when `abi_minor < 2`
    pub lv_submit: extern "C" fn(buf: *const u8, len: usize) -> i32,
    // ABI 1.3 (docs/abi-1.3.md): never read when `abi_minor < 3`
    pub own_back: extern "C" fn(on: bool),
}

/// Local wall-clock time as the watch shows it. `weekday` 0 = Sunday; `utc_offset_min` as set by the phone app.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Time {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub weekday: u8,
    pub millis: u16,
    pub utc_offset_min: i16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
    pub full: bool,
    pub charger: bool,
}

impl Api {
    pub fn rect(&self, x: i16, y: i16, w: u16, h: u16, rgb565: u16) {
        (self.fill_rect)(x, y, w, h, rgb565)
    }

    /// 8x8 font at 2x, 16 px per char. ASCII and Cyrillic (А-я, Ё, ё) have glyphs, any other char draws as `?`.
    pub fn text(&self, x: i16, y: i16, text: &str, rgb565: u16) {
        (self.draw_text)(x, y, text.as_ptr(), text.len(), rgb565)
    }

    pub fn show(&self) {
        (self.present)()
    }

    pub fn event(&self, timeout_ms: u32) -> Option<Event> {
        let mut event = Event::default();
        (self.next_event)(&mut event, timeout_ms).then_some(event)
    }

    /// `duty_permille` 0..=1000 (0 = off), `freq_hz` 1..=20000. Returns 0, or:
    /// `-1` out of range; `-3` paused under a stock popup (between `PAUSE` and `RESUME`);
    /// `-4` locked: the loader cut a run longer than 2000 ms, and the motor stays off until `vibrate(0, _)`;
    /// another negative value is a PWM driver error.
    pub fn vibrate(&self, duty_permille: u16, freq_hz: u16) -> i32 {
        (self.motor)(duty_permille, freq_hz)
    }

    pub fn now(&self) -> u32 {
        (self.millis)()
    }

    /// Queues a message (1..=[`MSG_MAX`] bytes) for the host. `0`, or `-1` bad length, `-11` outbox full
    /// (4 queued), `-107` no wp2apps host connected.
    pub fn send(&self, data: &[u8]) -> i32 {
        (self.send_msg)(data.as_ptr(), data.len())
    }

    /// Takes the oldest host message into `buf`. `None` if the inbox is empty or `buf` is shorter than the message
    /// (then it stays queued; [`MSG_MAX`] always fits).
    pub fn recv<'a>(&self, buf: &'a mut [u8]) -> Option<&'a [u8]> {
        let n = (self.recv_msg)(buf.as_mut_ptr(), buf.len());
        (n > 0).then(|| &buf[..n as usize])
    }

    /// The app's stored blob: its length (`0` = nothing stored), `-1` if `buf` is too short, `-5` I/O error.
    pub fn load(&self, buf: &mut [u8]) -> i32 {
        (self.load_data)(buf.as_mut_ptr(), buf.len())
    }

    /// Replaces the stored blob atomically (at most [`STORE_MAX`] bytes; empty deletes it). Writes NAND: call on
    /// change, not per frame. `0`, `-1` too long, `-5` I/O error.
    pub fn save(&self, data: &[u8]) -> i32 {
        (self.save_data)(data.as_ptr(), data.len())
    }

    pub fn time(&self) -> Option<Time> {
        let mut t = Time::default();
        ((self.get_time)(&mut t) == 0).then_some(t)
    }

    pub fn battery(&self) -> Option<Battery> {
        let b = (self.get_battery)();
        (b >= 0).then(|| Battery {
            percent: b as u8,
            charging: b & 0x100 != 0,
            full: b & 0x200 != 0,
            charger: b & 0x400 != 0,
        })
    }

    /// The LVGL command channel, `None` on a loader older than 1.2 (the table has no `lv_submit` then).
    pub fn lv(&self) -> Option<lv::Lv> {
        (self.abi_minor >= 2).then(|| lv::Lv(self.lv_submit))
    }

    /// Keeps the display on while this app runs in the foreground; the loader drops it on pause and exit.
    pub fn keep_screen_on(&self, on: bool) {
        (self.screen_on)(on);
    }

    /// Starts (`true`) or stops the watch's continuous heart-rate measurement for this app; while it runs in the
    /// foreground a [`HEART`] event comes about once a second. The loader stops the measurement on exit, crash,
    /// force stop and `PAUSE`, and starts it again on `RESUME`; a new launch starts with it off. Costs battery:
    /// ask only while the value is on screen or recorded. `0`, or `-22` on a loader older than 2026-11-14 (no
    /// heart rate there; ABI 1.3, no `abi_minor` change).
    pub fn heart_rate(&self, on: bool) -> i32 {
        if self.abi_minor < 2 {
            return -22;
        }
        let r = if on { HEART_ON } else { HEART_OFF };
        (self.lv_submit)(r.as_ptr(), r.len())
    }

    /// Turns the watch's GPS on (`true`) or off for this app; while it runs in the foreground a [`GPS`] event comes
    /// about once a second, and [`Api::gps_fix`] reads the position. The loader turns the GPS off on exit, crash,
    /// force stop and `PAUSE`, and on again on `RESUME`; a new launch starts with it off. A GPS a stock workout
    /// already runs is only read, never turned off. Costs battery (the receiver is on): ask only while the position
    /// is shown or recorded. The stock also stores a point of its own every few seconds while the GPS has a fix
    /// (docs/abi-1.4.md, "GPS"). `0`, or `-22` on a loader older than 2026-11-19 (ABI 1.3, no `abi_minor` change).
    pub fn gps(&self, on: bool) -> i32 {
        if self.abi_minor < 2 {
            return -22;
        }
        let r = if on { GPS_ON } else { GPS_OFF };
        (self.lv_submit)(r.as_ptr(), r.len())
    }

    /// The latest fix after [`Api::gps`]`(true)`; `fix` 0 until the receiver has one. `None` while the app has not
    /// asked for the GPS, and on a loader older than 2026-11-19.
    pub fn gps_fix(&self) -> Option<GpsFix> {
        if self.abi_minor < 2 {
            return None;
        }
        // lv_submit takes only buffers inside the app block: a static, not the stack; the loader fills the payload
        static mut BUF: [u8; 4 + GPS_FIX_LEN] = [0; 4 + GPS_FIX_LEN];
        let buf = unsafe { &mut *core::ptr::addr_of_mut!(BUF) };
        buf[..4].copy_from_slice(&[0x18, GPS_FIX_LEN as u8, 0, 0]);
        if (self.lv_submit)(buf.as_ptr(), buf.len()) != 0 {
            return None;
        }
        let b = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(BUF)) };
        Some(GpsFix::from_bytes(b[4..].try_into().unwrap()))
    }

    /// Sends a music command to the phone ([`MEDIA_PLAY`], [`MEDIA_PAUSE`], [`MEDIA_PREV`], [`MEDIA_NEXT`],
    /// [`MEDIA_VOLUME_UP`], [`MEDIA_VOLUME_DOWN`]), as the stock music controls do. `0`; `-19` when the phone is not
    /// linked or has no media; `-22` for another value, on a loader older than 2026-11-16, and for the volume
    /// commands on the loaders of 2026-11-16 to 2026-11-19. Waits for the loader's UI tick: with the panel dark it returns once it
    /// is lit again.
    pub fn media_key(&self, cmd: u8) -> i32 {
        if self.abi_minor < 2 {
            return -22;
        }
        static mut BUF: [u8; 5] = [0x16, 1, 0, 0, 0];
        let buf = unsafe { &mut *core::ptr::addr_of_mut!(BUF) };
        buf[4] = cmd;
        (self.lv_submit)(buf.as_ptr(), buf.len())
    }

    /// Has the loader show the phone's track in two labels (`title`, `artist`; `None` = not shown): it writes them
    /// once a second while the app runs in the foreground, and sends [`MEDIA`] at once and on every change of
    /// playing/available. Call again to change the labels; [`Api::media_unbind`] stops it (the app's exit does
    /// too). `0`; `-22` for a handle that is not a live label, before the first LVGL batch, and on a loader older
    /// than 2026-11-16.
    pub fn media_bind(&self, title: Option<lv::Obj>, artist: Option<lv::Obj>) -> i32 {
        self.media_record(true, title.map_or(0, |o| o.0), artist.map_or(0, |o| o.0))
    }

    /// Stops [`Api::media_bind`]: the labels keep their last text, no more [`MEDIA`] events.
    pub fn media_unbind(&self) -> i32 {
        self.media_record(false, 0, 0)
    }

    fn media_record(&self, on: bool, title: u16, artist: u16) -> i32 {
        if self.abi_minor < 2 {
            return -22;
        }
        static mut BUF: [u8; 9] = [0x17, 5, 0, 0, 0, 0, 0, 0, 0];
        let buf = unsafe { &mut *core::ptr::addr_of_mut!(BUF) };
        buf[4] = on.into();
        buf[5..7].copy_from_slice(&title.to_le_bytes());
        buf[7..9].copy_from_slice(&artist.to_le_bytes());
        (self.lv_submit)(buf.as_ptr(), buf.len())
    }

    /// The app's widget on the cards left of the watch face (`wp2apps cards set widgets:0x40/<slot + 1>`): a
    /// `title` (one line, up to 23 bytes) and a `value` (up to 39 bytes, `\n` starts a second line), cut at a char
    /// boundary. Kept on NAND until the next call or the app's removal; the card shows it from its next layout
    /// (the face shown again). Each call writes NAND: call it when the value changes, not every frame. `0`, `-5` on
    /// a write error, `-22` on a loader without widgets.
    pub fn widget(&self, title: &str, value: &str) -> i32 {
        if self.abi_minor < 2 {
            return -22;
        }
        // lv_submit takes only buffers inside the app block: a static, not the stack
        static mut BUF: [u8; WIDGET_BATCH] = [0; WIDGET_BATCH];
        let buf = unsafe { &mut *core::ptr::addr_of_mut!(BUF) };
        *buf = widget_record(title, value);
        (self.lv_submit)(buf.as_ptr(), buf.len())
    }

    /// A `columns` card (stub 9, docs/app-guide.md "Widget"): one to four columns, each a caption, one of the app's
    /// widget icons (`widget-icons/` next to `Cargo.toml`, by index in file name order) and a value, laid out as the
    /// stock weather-by-hour widget. Text over 9 bytes is cut at a char boundary. `0`; `-22` when the watch's stub
    /// cannot draw it (before stub 9, or a loader up to `loader-2026-11-22-884e9a6`), for 0 or more than 4 columns or
    /// an icon index over 15: fall back to [`Api::widget`] (with `º`, its font has no `°`). An icon the package lacks
    /// is left out, the card stays. Writes NAND like `widget`.
    pub fn widget_columns(&self, columns: &[Column]) -> i32 {
        match columns_record(columns) {
            Some((rec, n)) => self.widget_v2(&rec[..n]),
            None => -22,
        }
    }

    /// A v2 widget record as is (`FF ‖ template ‖ fields`, up to 128 bytes, docs/widget-push.md). `-22` for a record
    /// the loader refuses or the running stub does not draw.
    pub fn widget_v2(&self, rec: &[u8]) -> i32 {
        if self.abi_minor < 2 || rec.is_empty() || rec.len() > WIDGET_V2_MAX {
            return -22;
        }
        // lv_submit takes only buffers inside the app block: a static, not the stack
        static mut BUF: [u8; 4 + WIDGET_V2_MAX] = [0; 4 + WIDGET_V2_MAX];
        let buf = unsafe { &mut *core::ptr::addr_of_mut!(BUF) };
        buf[..4].copy_from_slice(&[0x14, rec.len() as u8, 0, 0]);
        buf[4..4 + rec.len()].copy_from_slice(rec);
        (self.lv_submit)(buf.as_ptr(), 4 + rec.len())
    }

    /// `true` when the app runs as its page next to the watch face (`app_features!(PAGE)`, docs/app-guide.md "Page"),
    /// `false` in the Apps menu and on a loader without pages. On a page the crown and the button stay with the
    /// watch (no BUTTON), and swiping away gives PAUSE, back RESUME.
    pub fn on_page(&self) -> bool {
        if self.abi_minor < 2 {
            return false;
        }
        static mut BUF: [u8; 4] = [0x15, 0, 0, 0];
        (self.lv_submit)(unsafe { core::ptr::addr_of!(BUF) } as *const u8, 4) == 1
    }

    /// ABI 1.3: `true` = this app handles BUTTON ("back") itself and exits by returning; the loader then stops it
    /// only if it leaves a BUTTON unread for 1 s (hung). A no-op on an older loader.
    pub fn own_back(&self, on: bool) {
        if self.abi_minor >= 3 {
            (self.own_back)(on);
        }
    }
}

const WIDGET_BATCH: usize = 4 + 64;
const WIDGET_V2_MAX: usize = 128;

/// One column of [`Api::widget_columns`]: `icon` is an index into the app's widget icons, `None` for none.
#[derive(Clone, Copy, Debug)]
pub struct Column<'a> {
    pub caption: &'a str,
    pub icon: Option<u8>,
    pub value: &'a str,
}

/// The v2 `columns` record (`FF 01 n`, then per column caption[10] ‖ icon ‖ value[10]) and its length; None for 0
/// or more than 4 columns or an icon over 15. The same bytes as `wp2apps.install.columns_record`.
#[doc(hidden)]
pub fn columns_record(columns: &[Column]) -> Option<([u8; WIDGET_V2_MAX], usize)> {
    if columns.is_empty() || columns.len() > 4 || columns.iter().any(|c| c.icon.is_some_and(|i| i > 15)) {
        return None;
    }
    let mut out = [0u8; WIDGET_V2_MAX];
    out[..3].copy_from_slice(&[0xFF, 1, columns.len() as u8]);
    for (i, c) in columns.iter().enumerate() {
        let at = 3 + 21 * i;
        put(&mut out[at..at + 10], c.caption);
        out[at + 10] = c.icon.unwrap_or(0xFF);
        put(&mut out[at + 11..at + 21], c.value);
    }
    Some((out, 3 + 21 * columns.len()))
}

/// `s` into `out`, cut at a char boundary so a NUL stays
fn put(out: &mut [u8], s: &str) {
    let mut n = s.len().min(out.len() - 1);
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    out[..n].copy_from_slice(&s.as_bytes()[..n]);
}

/// WIDGET record for `lv_submit` (op 0x14, handle 0): title NUL-padded to 24 bytes, then the value NUL-padded to 40.
#[doc(hidden)]
pub fn widget_record(title: &str, value: &str) -> [u8; WIDGET_BATCH] {
    let mut out = [0u8; WIDGET_BATCH];
    out[..4].copy_from_slice(&[0x14, 64, 0, 0]);
    put(&mut out[4..28], title);
    put(&mut out[28..], value);
    out
}

static API: AtomicPtr<Api> = AtomicPtr::new(core::ptr::null_mut());

pub fn api() -> Option<&'static Api> {
    unsafe { API.load(Ordering::Relaxed).as_ref() }
}

#[doc(hidden)]
pub fn __start(api: &'static Api, main: fn(&'static Api) -> i32) -> i32 {
    // the volatile read keeps WP2_ABI_MINOR in the linked image for the packager
    if api.abi != ABI || api.abi_minor < unsafe { core::ptr::read_volatile(&WP2_ABI_MINOR) } {
        return -2;
    }
    API.store(api as *const Api as *mut Api, Ordering::Relaxed);
    main(api)
}

#[cfg(not(wp2_sim))]
#[macro_export]
macro_rules! entry {
    ($main:path) => {
        #[unsafe(no_mangle)]
        #[unsafe(link_section = ".entry")]
        pub extern "C" fn _start(api: &'static $crate::Api) -> i32 {
            $crate::__start(api, $main)
        }
    };
}

#[cfg(wp2_sim)]
#[macro_export]
macro_rules! entry {
    ($main:path) => {
        extern "C" fn _start(api: &'static $crate::Api) -> i32 {
            $crate::__start(api, $main)
        }
        #[unsafe(export_name = "main")]
        extern "C" fn __wp2_sim_main() -> i32 {
            $crate::__sim_run(_start)
        }
    };
}

// `scripts/sim` builds apps for the host with `--cfg wp2_sim` (docs/app-guide.md). Keep it below the code with
// panics: their line numbers are in the watch build.
#[cfg(wp2_sim)]
extern crate std;

/// Runs the app in the host simulator (`sim/`, linked by `scripts/sim`).
#[cfg(wp2_sim)]
#[doc(hidden)]
pub fn __sim_run(start: extern "C" fn(&'static Api) -> i32) -> i32 {
    #[link(name = "wp2_sim")]
    unsafe extern "C" {
        fn wp2_sim_run(start: extern "C" fn(&'static Api) -> i32) -> i32;
    }
    // like the panic handler below: motor off, exit(-1)
    std::panic::set_hook(std::boxed::Box::new(|info| {
        std::eprintln!("{info}");
        if let Some(api) = api() {
            api.vibrate(0, 0);
            (api.exit)(-1)
        }
    }));
    unsafe { wp2_sim_run(start) }
}

/// The app's menu icon, in front of the code (docs/app-icons.md). `scripts/build-app` renders the crate's
/// `icon.svg` and passes the descriptor in `WP2_ICON`.
#[cfg(not(wp2_sim))]
#[macro_export]
macro_rules! app_icon {
    () => {
        #[used]
        #[unsafe(link_section = ".wp2info")]
        static WP2_ICON: [u8; include_bytes!(env!("WP2_ICON")).len()] = *include_bytes!(env!("WP2_ICON"));
    };
}

#[cfg(wp2_sim)]
#[macro_export]
macro_rules! app_icon {
    () => {};
}

/// `app_features!` flag: the app has a widget for the cards left of the watch face (`Api::widget`); the Apps menu
/// offers "Add widget" on a long press.
pub const WIDGET: u8 = 1;
/// `app_features!` flag: the app can run as its own page next to the watch face (`Api::on_page`); the Apps menu
/// offers "Add page".
pub const PAGE: u8 = 2;

/// Declares what the app offers beyond its menu row: `app_features!(cmf_wp2_app::WIDGET | cmf_wp2_app::PAGE);`
/// (docs/app-guide.md, "Widget" and "Page"). Placed after the icon.
#[cfg(not(wp2_sim))]
#[macro_export]
macro_rules! app_features {
    ($flags:expr) => {
        #[used]
        #[unsafe(link_section = ".wp2info.features")]
        static WP2_FEATURES: [u8; 8] = [b'W', b'P', b'2', b'F', $flags, 0, 0, 0];
    };
}

#[cfg(wp2_sim)]
#[macro_export]
macro_rules! app_features {
    ($flags:expr) => {};
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    if let Some(api) = api() {
        api.vibrate(0, 0);
        (api.exit)(-1)
    }
    loop {}
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::boxed::Box;

    #[test]
    fn widget_record_truncates_at_char_boundary() {
        let r = widget_record("Пульс и ещё длинный заголовок", "72 bpm");
        assert_eq!(&r[..4], &[0x14, 64, 0, 0]);
        let title = &r[4..28];
        let end = title.iter().position(|&b| b == 0).unwrap();
        assert!(end <= 23 && core::str::from_utf8(&title[..end]).is_ok());
        assert_eq!(&r[28..35], b"72 bpm\0");
        assert_eq!(widget_record("", &"x".repeat(80))[28 + 39], 0, "value keeps its NUL");
    }

    #[test]
    fn columns_record_golden() {
        // tests/test_install.py COLUMNS: wp2apps.install.columns_record gives the same bytes
        let c = |caption, icon, value| Column { caption, icon, value };
        let (r, n) = columns_record(&[c("Now", Some(0), "-3°"), c("23:00", Some(1), "4°"), c("00:00", None, "--")]).unwrap();
        let hex: std::string::String = r[..n].iter().map(|b| std::format!("{b:02x}")).collect();
        assert_eq!(hex, "ff01034e6f7700000000000000002d33c2b000000000000032333a303000000000000134c2b00000000000000030303a30300000000000ff2d2d0000000000000000");
        assert!(columns_record(&[]).is_none());
        assert!(columns_record(&[c("a", None, "b"); 5]).is_none());
        assert!(columns_record(&[c("a", Some(16), "b")]).is_none());
        let (r, n) = columns_record(&[c("ééééé", Some(15), "x")]).unwrap();
        assert_eq!((n, &r[3..13]), (24, "éééé\0\0".as_bytes()));
    }

    extern "C" fn rect(_: i16, _: i16, _: u16, _: u16, _: u16) {}
    extern "C" fn text(_: i16, _: i16, _: *const u8, _: usize, _: u16) {}
    extern "C" fn present() {}
    extern "C" fn next(_: *mut Event, _: u32) -> bool {
        false
    }
    extern "C" fn motor(_: u16, _: u16) -> i32 {
        0
    }
    extern "C" fn millis() -> u32 {
        0
    }
    extern "C" fn exit(_: i32) -> ! {
        panic!("exit")
    }
    extern "C" fn bytes(_: *const u8, _: usize) -> i32 {
        0
    }
    extern "C" fn bytes_mut(_: *mut u8, _: usize) -> i32 {
        0
    }
    extern "C" fn time(_: *mut Time) -> i32 {
        0
    }
    extern "C" fn battery() -> i32 {
        0x1_40
    }
    extern "C" fn screen(_: bool) -> i32 {
        0
    }
    extern "C" fn flag(_: bool) {}

    pub(crate) fn table(abi: u16, abi_minor: u16) -> &'static mut Api {
        Box::leak(Box::new(Api {
            abi, abi_minor, width: 466, height: 466, fill_rect: rect, draw_text: text, present,
            next_event: next, motor, millis, exit, send_msg: bytes, recv_msg: bytes_mut, load_data: bytes_mut,
            save_data: bytes, get_time: time, get_battery: battery, screen_on: screen, lv_submit: bytes, own_back: flag,
        }))
    }

    fn app(_: &'static Api) -> i32 {
        7
    }

    #[test]
    fn runs_on_same_major_and_newer_minor() {
        assert_eq!(__start(table(ABI, ABI_MINOR + 1), app), 7);
    }

    #[test]
    fn rejects_other_major() {
        assert_eq!(__start(table(ABI + 1, ABI_MINOR), app), -2);
    }

    #[test]
    fn rejects_older_minor() {
        assert_eq!(__start(table(ABI, ABI_MINOR - 1), app), -2);
    }

    extern "C" fn sensor(buf: *const u8, len: usize) -> i32 {
        let r = unsafe { core::slice::from_raw_parts(buf, len) };
        r[5] as i32 + 10
    }

    #[test]
    fn heart_rate_records() {
        let api = table(ABI, ABI_MINOR);
        api.lv_submit = sensor;
        assert_eq!((api.heart_rate(true), api.heart_rate(false)), (11, 10));
        api.abi_minor = 1;
        assert_eq!(api.heart_rate(true), -22);
    }

    extern "C" fn echo(buf: *const u8, len: usize) -> i32 {
        let r = unsafe { core::slice::from_raw_parts(buf, len) };
        r.iter().enumerate().map(|(i, &b)| (i as i32 + 1) * b as i32).sum()
    }

    #[test]
    fn media_records() {
        let mut api = table(ABI, ABI_MINOR);
        api.lv_submit = echo;
        // [0x16, 1, 0, 0, 4]
        assert_eq!(api.media_key(MEDIA_NEXT), 0x16 + 2 + 5 * 4);
        assert_eq!(api.media_key(MEDIA_VOLUME_DOWN), 0x16 + 2 + 5 * 6);
        // [0x17, 5, 0, 0, 1, 3, 0, 0x34, 0x12]
        assert_eq!(api.media_bind(Some(lv::Obj(3)), Some(lv::Obj(0x1234))), 0x17 + 10 + 5 + 6 * 3 + 8 * 0x34 + 9 * 0x12);
        assert_eq!(api.media_unbind(), 0x17 + 10);
        api.abi_minor = 1;
        assert_eq!(api.media_key(MEDIA_PLAY), -22);
    }

    #[test]
    fn battery_bits() {
        let b = table(ABI, ABI_MINOR).battery().unwrap();
        assert_eq!((b.percent, b.charging, b.full, b.charger), (0x40, true, false, false));
    }
}
