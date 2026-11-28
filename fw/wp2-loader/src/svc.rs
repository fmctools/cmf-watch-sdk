// ABI 1.1 services (docs/abi-1.1.md): host channel, app data, time, battery, screen wake lock.
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use wp2_core::chan::{listening, parse_store, store_header, Ring, MSG_MAX, STORE_HEADER, STORE_MAX};
use wp2_core::events::{Event, Time, MESSAGE};
use wp2_core::fs;
use wp2_core::layout::{data_path, stub_ram, APP_PAUSED, APP_V11, E_IO, KICK_MAGIC};
use wp2_core::pkg::crc32;
use wp2_core::stock::*;

use crate::app::NONE;

const E_RANGE: i32 = -1;
const E_AGAIN: i32 = -11;
const E_NOTCONN: i32 = -107;
// rtc_app: signed zone hours/minutes at +0/+1, the RTC device at +4 (0x100C14B4, 0x100C1458)
const RTC_APP: u32 = 0x2FFB_449C;
const WAKE_ID: u32 = 0xF0;
const ZONE_MAX: i32 = 14 * 3600;

// inbox position up to which MESSAGE events went out (app thread)
static ANNOUNCED: AtomicU8 = AtomicU8::new(0);
static KEEP_ON: AtomicBool = AtomicBool::new(false);
static WAKE_HELD: AtomicBool = AtomicBool::new(false);

fn channel() -> bool {
    stub_ram().api == 1
}

fn inbox() -> &'static Ring {
    &stub_ram().inbox
}

// UI thread, app thread not running yet. The blob the phone pushed (stub 8, docs/widget-push.md) is the app's first
// host message: running_slot is still NONE, so the stub pushes nothing into the inbox meanwhile.
pub fn start(slot: u8, abi_minor: u8) {
    let ram = stub_ram();
    KEEP_ON.store(false, Ordering::Relaxed);
    if channel() {
        ram.inbox.clear();
        ANNOUNCED.store(ram.inbox.head(), Ordering::Relaxed);
        ram.app_flags = if abi_minor >= 1 { APP_V11 } else { 0 };
        let mut file: fs::File = [0; 0x2C];
        if abi_minor >= 1 && fs::open(&mut file, &wp2_core::layout::blob_path(slot), fs::READ) >= 0 {
            let mut blob = [0u8; wp2_core::chan::MSG_MAX];
            let n = fs::read(&mut file, &mut blob);
            fs::close(&mut file);
            if n > 0 {
                ram.inbox.push(slot, &blob[..n as usize]);
            }
        }
    }
}

// UI thread, app thread aborted
pub fn stop() {
    file_close();
    KEEP_ON.store(false, Ordering::Relaxed);
    wake(false);
    if channel() {
        let ram = stub_ram();
        ram.app_flags = 0;
        ram.inbox.clear();
    }
}

pub fn set_paused(paused: bool) {
    if channel() {
        let ram = stub_ram();
        ram.app_flags = if paused { ram.app_flags | APP_PAUSED } else { ram.app_flags & !APP_PAUSED };
    }
}

// UI tick: the wake lock follows the app's wish while it runs in the foreground
pub fn tick(running: bool, paused: bool) {
    wake(KEEP_ON.load(Ordering::Relaxed) && running && !paused);
}

fn wake(on: bool) {
    if WAKE_HELD.swap(on, Ordering::AcqRel) == on {
        return;
    }
    if on {
        stock!(WAKE_LOCK, fn(u32))(WAKE_ID);
    } else {
        stock!(WAKE_UNLOCK, fn(u32))(WAKE_ID);
    }
}

// app thread (next_event): a MESSAGE event for the oldest inbox message not announced yet
pub fn announce() -> Option<Event> {
    if !channel() {
        return None;
    }
    let r = inbox();
    let (head, tail) = (r.head(), r.tail());
    let mut a = ANNOUNCED.load(Ordering::Relaxed);
    if tail.wrapping_sub(a) > tail.wrapping_sub(head) {
        a = head; // recv() went past the announced ones
    }
    let m = r.peek(a.wrapping_sub(head))?;
    ANNOUNCED.store(a.wrapping_add(1), Ordering::Relaxed);
    Some(Event { kind: MESSAGE, x: i16::from(m.len), ..Event::default() })
}

pub extern "C" fn send(data: *const u8, len: usize) -> i32 {
    if data.is_null() || !(1..=MSG_MAX).contains(&len) {
        return E_RANGE;
    }
    let ram = stub_ram();
    if !channel() || !listening(ram.host_key) {
        return E_NOTCONN;
    }
    let bytes = unsafe { core::slice::from_raw_parts(data, len) };
    if !ram.outbox.push(ram.running_slot, bytes) {
        return E_AGAIN;
    }
    // a stub with kick sends it to a K host now instead of on the host's next frame (docs/abi-1.4.md)
    if ram.kick_magic == KICK_MAGIC && ram.kick != 0 {
        let kick: extern "C" fn() = unsafe { core::mem::transmute(ram.kick as usize) };
        kick();
    }
    0
}

pub extern "C" fn recv(buf: *mut u8, cap: usize) -> i32 {
    let Some(m) = (if channel() { inbox().peek(0) } else { None }) else { return 0 };
    let n = usize::from(m.len);
    if buf.is_null() || cap < n {
        return E_RANGE;
    }
    unsafe { core::ptr::copy_nonoverlapping(m.data.as_ptr(), buf, n) };
    inbox().pop();
    n as i32
}

fn slot() -> Option<u8> {
    Some(stub_ram().running_slot).filter(|&s| s != NONE)
}

pub extern "C" fn save(data: *const u8, len: usize) -> i32 {
    if len > STORE_MAX || (data.is_null() && len > 0) {
        return E_RANGE;
    }
    let Some(slot) = slot() else { return E_IO };
    let (dat, new) = (data_path(slot, false), data_path(slot, true));
    fs::unlink(&new);
    if len == 0 {
        fs::unlink(&dat);
        return 0;
    }
    let bytes = unsafe { core::slice::from_raw_parts(data, len) };
    let mut file: fs::File = [0; 0x2C];
    if fs::open(&mut file, &new, fs::WRITE | fs::CREATE) < 0 {
        return E_IO;
    }
    let header = store_header(bytes);
    let ok = fs::write(&mut file, &header) == STORE_HEADER as i32 && fs::write(&mut file, bytes) == len as i32;
    if fs::close(&mut file) < 0 || !ok {
        fs::unlink(&new);
        return E_IO;
    }
    fs::unlink(&dat);
    if fs::rename(&new, &dat) < 0 { E_IO } else { 0 }
}

// the app's widget record (lv_submit WIDGET): replaced (records differ in length, an open does not truncate), the
// stub reads it when a card is laid out.
// ponytail: no tmp + rename as in save(); a cut write shows the package name until the next one (the stub checks)
pub fn save_widget(rec: &[u8]) -> i32 {
    let Some(slot) = slot() else { return E_IO };
    let mut file: fs::File = [0; 0x2C];
    let path = wp2_core::layout::widget_path(slot);
    fs::unlink(&path);
    if fs::open(&mut file, &path, fs::WRITE | fs::CREATE) < 0 {
        return E_IO;
    }
    let ok = fs::write(&mut file, rec) == rec.len() as i32;
    if fs::close(&mut file) < 0 || !ok { E_IO } else { 0 }
}

enum Blob {
    Missing,
    Bad,
    Len(i32),
}

fn read_blob(path: &[u8], buf: &mut [u8]) -> Blob {
    let mut file: fs::File = [0; 0x2C];
    if fs::open(&mut file, path, fs::READ) < 0 {
        return Blob::Missing;
    }
    let mut h = [0u8; STORE_HEADER];
    let r = match fs::read_exact(&mut file, &mut h).then(|| parse_store(&h)).flatten() {
        None => Blob::Bad,
        Some((len, _)) if len > buf.len() => Blob::Len(E_RANGE),
        Some((len, crc)) if len == 0 || (fs::read_exact(&mut file, &mut buf[..len]) && crc32(0, &buf[..len]) == crc) => {
            Blob::Len(len as i32)
        }
        Some(_) => Blob::Bad,
    };
    fs::close(&mut file);
    r
}

pub extern "C" fn load(buf: *mut u8, cap: usize) -> i32 {
    let Some(slot) = slot() else { return E_IO };
    let buf = if buf.is_null() { &mut [][..] } else { unsafe { core::slice::from_raw_parts_mut(buf, cap) } };
    let mut bad = false;
    // N.new only survives a save cut between unlink and rename; a half-written one fails the CRC
    for path in [data_path(slot, false), data_path(slot, true)] {
        match read_blob(&path, buf) {
            Blob::Len(n) => return n,
            Blob::Bad => bad = true,
            Blob::Missing => {}
        }
    }
    if bad { E_IO } else { 0 }
}

/// The wall clock's hour and minute, None till the phone set it (the status bar).
pub fn clock() -> Option<(u8, u8)> {
    if unsafe { ((RTC_APP + 4) as *const u32).read_volatile() } == 0 {
        return None;
    }
    let mut raw = [0u8; 16];
    stock!(RTC_GET_TIME, fn(*mut u8))(raw.as_mut_ptr());
    Some((raw[2], raw[1]))
}

pub extern "C" fn time(out: *mut Time) -> i32 {
    if out.is_null() || unsafe { ((RTC_APP + 4) as *const u32).read_volatile() } == 0 {
        return E_IO;
    }
    let mut raw = [0u8; 16];
    stock!(RTC_GET_TIME, fn(*mut u8))(raw.as_mut_ptr());
    let zone = stock!(TIME_ZONE, fn() -> i32)();
    let t = Time {
        second: raw[0],
        minute: raw[1],
        hour: raw[2],
        day: raw[3],
        month: raw[4],
        weekday: raw[5],
        year: u16::from_le_bytes([raw[6], raw[7]]),
        millis: u16::from_le_bytes([raw[8], raw[9]]),
        // never synced: the stock zone is not a real offset
        utc_offset_min: if zone.abs() <= ZONE_MAX { (zone / 60) as i16 } else { i16::MIN },
    };
    unsafe { out.write(t) };
    0
}

pub extern "C" fn battery() -> i32 {
    let capacity = stock!(POWER_CAPACITY, fn() -> i32)();
    if capacity < 0 {
        return E_IO;
    }
    let status = stock!(POWER_STATUS, fn() -> i32)();
    let dc5v = stock!(POWER_DC5V, fn() -> i32)();
    capacity.clamp(0, 100) | i32::from(status == 3) << 8 | i32::from(status == 4) << 9 | i32::from(dc5v > 0) << 10
}

pub extern "C" fn keep_screen_on(on: bool) -> i32 {
    KEEP_ON.store(on, Ordering::Relaxed);
    0
}

struct Reader {
    f: fs::File,
    path: [u8; 48],
    pos: usize,
    open: bool,
}

static mut READER: Reader = Reader { f: [0; 0x2C], path: [0; 48], pos: 0, open: false };

/// Closes the file a FILE read left open: before a write, and when the app ends.
pub fn file_close() {
    let rd = unsafe { &mut *core::ptr::addr_of_mut!(READER) };
    if rd.open {
        fs::close(&mut rd.f);
        rd.open = false;
    }
}

/// App thread (lv_submit FILE, wp2-core lvcmd::file): `/NAND:/wp2apps/<slot>_<name>`. A read lands in the batch
/// buffer (`batch`). NAND is written synchronously, like `save`.
pub fn file(cmd: wp2_core::lvcmd::FileCmd, batch: (*mut u8, usize)) -> i32 {
    use wp2_core::lvcmd::FileCmd;
    let Some(slot) = slot() else { return E_RANGE };
    let name = match cmd {
        FileCmd::Read { name, .. } | FileCmd::Write { name, .. } | FileCmd::Delete { name } => name,
        FileCmd::Emoji { .. } => b"emoji",
    };
    // "/NAND:/wp2apps/" + slot hex + "_" + name + NUL
    let mut path = [0u8; 48];
    let dir = &wp2_core::layout::DIR[..wp2_core::layout::DIR.len() - 1];
    path[..dir.len()].copy_from_slice(dir);
    let mut n = dir.len();
    path[n] = b'/';
    path[n + 1] = b"0123456789ABCDEF"[usize::from(slot & 15)];
    path[n + 2] = b'_';
    n += 3;
    path[n..n + name.len()].copy_from_slice(name);
    if matches!(cmd, FileCmd::Emoji { .. }) {
        // the stock's own emoji, read in place: nothing is copied or written
        const EMOJI: &[u8] = b"/NAND:C/emoji28.font";
        path = [0; 48];
        path[..EMOJI.len()].copy_from_slice(EMOJI);
    }
    let mut f: fs::File = [0; 0x2C];
    if !matches!(cmd, FileCmd::Read { .. } | FileCmd::Emoji { .. }) {
        file_close();
    }
    match cmd {
        FileCmd::Read { skip, .. } | FileCmd::Emoji { skip } => {
            // the batch buffer itself takes the bytes (lv::submit checked it lies in the app block)
            let out = unsafe { core::slice::from_raw_parts_mut(batch.0, batch.1) };
            let rd = unsafe { &mut *core::ptr::addr_of_mut!(READER) };
            // the stock fs table has no seek: the last file read stays open, so chunk after chunk is linear
            if !(rd.open && rd.path == path && rd.pos == skip as usize) {
                if rd.open {
                    fs::close(&mut rd.f);
                }
                rd.open = false;
                let rc = fs::open(&mut rd.f, &path, fs::READ);
                if rc < 0 {
                    return rc;
                }
                rd.open = true;
                rd.path = path;
                rd.pos = 0;
                let mut left = skip as usize;
                while left > 0 {
                    let k = left.min(out.len());
                    let r = fs::read(&mut rd.f, &mut out[..k]);
                    if r <= 0 {
                        return 0;
                    }
                    left -= r as usize;
                    rd.pos += r as usize;
                }
            }
            let mut got = 0;
            while got < out.len() {
                let r = fs::read(&mut rd.f, &mut out[got..]);
                if r <= 0 {
                    break;
                }
                got += r as usize;
            }
            rd.pos += got;
            got as i32
        }
        FileCmd::Write { data, append, .. } => {
            if !append {
                fs::unlink(&path);
            }
            let rc = fs::open(&mut f, &path, fs::WRITE | fs::CREATE | if append { fs::APPEND } else { 0 });
            if rc < 0 {
                return rc;
            }
            let w = fs::write(&mut f, data);
            fs::close(&mut f);
            w
        }
        FileCmd::Delete { .. } => fs::unlink(&path),
    }
}
