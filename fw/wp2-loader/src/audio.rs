// The speaker for apps (docs/abi-1.4.md, "Audio"): 16-bit mono PCM through the stock audio_track, as the factory
// speaker test plays it (wp2_core::stock, AUDIO_TRACK_*).
//
// Safety model. Every stock audio call runs on the UI thread (the UI tick): the app thread only copies samples into
// RING, so a force stop or crash, which aborts the app thread wherever it is, can never leave a stock audio lock
// held. The track exists only while an app wants it, runs in the foreground and is not paused: a call, an alarm or a
// notification pauses the app and the track is destroyed before the stock needs the speaker; exit, crash and force
// stop destroy it (`stop`). While the track lives the loader holds the wake lock: the UI tick, which feeds the
// track, does not run while the panel is dark.
//
// RING comes from the RES heap at the app's first OPEN and goes back in `stop` (the app ended: its thread is gone and
// the UI thread, the only reader, is the caller), so the loader's 64 KiB block doesn't carry 6 KiB for apps that
// never play.
use core::ptr::{addr_of_mut, null_mut};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU8, Ordering};
use wp2_core::layout::stub_ram;
use wp2_core::lvcmd::{audio_level, AudioCmd};
use wp2_core::stock::*;

const E_ARG: i32 = -22;
const E_NOMEM: i32 = -12;
const E_NOTCONN: i32 = -107;
const WAKE_ID: u32 = 0xF1;
const VOLUME_UNSET: u8 = 0xFF;
const MAX_LEVEL_DEFAULT: u8 = 16;

/// 6 KiB: 190 ms at 16 kHz on top of the track's own 2 KiB; the UI tick runs every 10 ms.
const RING_SIZE: usize = 6 * 1024;
static RING: AtomicPtr<[u8; RING_SIZE]> = AtomicPtr::new(null_mut());
// free-running byte counts: HEAD written by the app thread, TAIL by the UI thread
static HEAD: AtomicU32 = AtomicU32::new(0);
static TAIL: AtomicU32 = AtomicU32::new(0);

// the app's wish: rate in kHz (0 = closed) and a generation bumped on every OPEN, so a failed create is tried
// again only when the app asks again
static WANT_KHZ: AtomicU8 = AtomicU8::new(0);
static WANT_GEN: AtomicU32 = AtomicU32::new(0);
static VOLUME: AtomicU8 = AtomicU8::new(VOLUME_UNSET);
static VOLUME_DIRTY: AtomicBool = AtomicBool::new(false);
// 0, or the error of the last create (STATE answers it)
static ERROR: AtomicI32 = AtomicI32::new(0);

// UI thread only
struct Track {
    handle: u32,
    stream: u32,
    khz: u8,
    want: u32,
    tried: u32,
}
static mut TRACK: Track = Track { handle: 0, stream: 0, khz: 0, want: 0, tried: 0 };

fn track() -> &'static mut Track {
    unsafe { &mut *addr_of_mut!(TRACK) }
}

/// The ring, once an OPEN got it from the heap.
fn ring() -> Option<&'static mut [u8; RING_SIZE]> {
    unsafe { RING.load(Ordering::Acquire).as_mut() }
}

// app thread (OPEN): the ring from the heap, unless the app has it already
fn take_ring() -> bool {
    if ring().is_some() {
        return true;
    }
    let p = wp2_core::heap::alloc(wp2_core::heap::RES, RING_SIZE as u32, 4);
    RING.store(p.cast(), Ordering::Release);
    !p.is_null()
}

// app thread (lv_submit). `p`..`p + len` checked against the app block here.
pub fn request(cmd: AudioCmd) -> i32 {
    match cmd {
        AudioCmd::Open { rate_khz } => {
            // no room for the ring: as a track that could not be made (STATE and WRITE answer -12)
            ERROR.store(if take_ring() { 0 } else { E_NOMEM }, Ordering::Relaxed);
            WANT_KHZ.store(rate_khz, Ordering::Release);
            WANT_GEN.fetch_add(1, Ordering::AcqRel);
            0
        }
        AudioCmd::Close => {
            WANT_KHZ.store(0, Ordering::Release);
            0
        }
        AudioCmd::Volume { percent } => {
            VOLUME.store(percent, Ordering::Relaxed);
            VOLUME_DIRTY.store(true, Ordering::Release);
            0
        }
        AudioCmd::State => {
            let e = ERROR.load(Ordering::Relaxed);
            if e != 0 { e } else { HEAD.load(Ordering::Acquire).wrapping_sub(TAIL.load(Ordering::Acquire)) as i32 }
        }
        AudioCmd::Write { ptr, len } => write(ptr, len),
        AudioCmd::MicOpen | AudioCmd::MicRead { .. } | AudioCmd::MicClose => crate::mic::request(cmd),
    }
}

fn write(ptr: u32, len: u32) -> i32 {
    if WANT_KHZ.load(Ordering::Acquire) == 0 {
        return E_NOTCONN;
    }
    let e = ERROR.load(Ordering::Relaxed);
    if e != 0 {
        return e;
    }
    let Some(r) = ring() else { return E_NOMEM };
    let ram = stub_ram();
    let (block, size) = (ram.app_block, ram.app_size);
    // no ptr + len: it could wrap
    if ptr < block || ptr - block > size || len > size - (ptr - block) {
        return E_ARG;
    }
    let head = HEAD.load(Ordering::Relaxed);
    let free = RING_SIZE as u32 - head.wrapping_sub(TAIL.load(Ordering::Acquire));
    let n = len.min(free) & !1;
    let src = unsafe { core::slice::from_raw_parts(ptr as *const u8, n as usize) };
    let at = head as usize % RING_SIZE;
    let first = (n as usize).min(RING_SIZE - at);
    r[at..at + first].copy_from_slice(&src[..first]);
    r[..n as usize - first].copy_from_slice(&src[first..]);
    HEAD.store(head.wrapping_add(n), Ordering::Release);
    n as i32
}

fn max_level() -> u8 {
    let cfg = unsafe { (AUDIO_CFG as *const u32).read_volatile() };
    if cfg == 0 { MAX_LEVEL_DEFAULT } else { unsafe { ((cfg + 2) as *const u8).read_volatile() } }
}

fn open(t: &mut Track, khz: u8, want: u32) {
    t.tried = want;
    // (stream_type, rate_khz, format 16-bit, mode mono, no outer stream, no event callback, no user data)
    let h = stock!(AUDIO_TRACK_CREATE, fn(u32, u32, u32, u32, u32, u32, u32) -> u32)(AUDIO_STREAM_TYPE, khz.into(), 1, 1, 0, 0, 0);
    if h == 0 {
        ERROR.store(E_NOMEM, Ordering::Relaxed);
        return;
    }
    let stream = stock!(AUDIO_TRACK_GET_STREAM, fn(u32) -> u32)(h);
    if stream == 0 {
        stock!(AUDIO_TRACK_DESTROY, fn(u32) -> i32)(h);
        ERROR.store(E_NOMEM, Ordering::Relaxed);
        return;
    }
    stock!(AUDIO_TRACK_START, fn(u32) -> i32)(h);
    stock!(WAKE_LOCK, fn(u32))(WAKE_ID);
    (t.handle, t.stream, t.khz, t.want) = (h, stream, khz, want);
    VOLUME_DIRTY.store(VOLUME.load(Ordering::Relaxed) != VOLUME_UNSET, Ordering::Release);
}

fn close(t: &mut Track) {
    if t.handle != 0 {
        stock!(AUDIO_TRACK_STOP, fn(u32) -> i32)(t.handle);
        stock!(AUDIO_TRACK_DESTROY, fn(u32) -> i32)(t.handle);
        stock!(WAKE_UNLOCK, fn(u32))(WAKE_ID);
        (t.handle, t.stream, t.khz) = (0, 0, 0);
    }
}

// what is queued is dropped: after a pause or a close it would play late
fn drop_queued() {
    TAIL.store(HEAD.load(Ordering::Acquire), Ordering::Release);
}

// UI tick
pub fn tick(running: bool, paused: bool) {
    let t = track();
    let khz = WANT_KHZ.load(Ordering::Acquire);
    let want = WANT_GEN.load(Ordering::Acquire);
    if !running || paused || khz == 0 {
        close(t);
        drop_queued();
        return;
    }
    if t.handle != 0 && t.khz != khz {
        close(t); // a new rate: a new track
    }
    if t.handle == 0 {
        if ERROR.load(Ordering::Relaxed) == E_NOMEM && ring().is_none() {
            return; // no ring: no track either
        }
        if t.tried == want && ERROR.load(Ordering::Relaxed) != 0 {
            return; // failed for this OPEN: not again every tick
        }
        open(t, khz, want);
        if t.handle == 0 {
            return;
        }
    }
    if VOLUME_DIRTY.swap(false, Ordering::AcqRel) {
        let level = audio_level(VOLUME.load(Ordering::Relaxed), max_level());
        stock!(AUDIO_TRACK_SET_VOLUME, fn(u32, u32) -> i32)(t.handle, level.into());
    }
    feed(t);
}

fn feed(t: &Track) {
    let space = stock!(STREAM_GET_SPACE, fn(u32) -> i32)(t.stream);
    if space <= 0 {
        return;
    }
    let tail = TAIL.load(Ordering::Relaxed);
    let queued = HEAD.load(Ordering::Acquire).wrapping_sub(tail);
    let mut n = (queued.min(space as u32) & !1) as usize;
    let mut at = tail as usize % RING_SIZE;
    let Some(r) = ring() else { return };
    while n > 0 {
        let chunk = n.min(RING_SIZE - at);
        let wrote = stock!(AUDIO_TRACK_WRITE, fn(u32, *const u8, u32) -> i32)(t.handle, r[at..].as_ptr(), chunk as u32);
        if wrote <= 0 {
            break;
        }
        TAIL.fetch_add(wrote as u32, Ordering::AcqRel);
        if wrote as usize != chunk {
            break;
        }
        n -= chunk;
        at = (at + chunk) % RING_SIZE;
    }
}

// UI thread, app thread aborted
pub fn stop() {
    WANT_KHZ.store(0, Ordering::Release);
    VOLUME.store(VOLUME_UNSET, Ordering::Relaxed);
    VOLUME_DIRTY.store(false, Ordering::Relaxed);
    ERROR.store(0, Ordering::Relaxed);
    close(track());
    track().tried = 0;
    HEAD.store(0, Ordering::Release);
    TAIL.store(0, Ordering::Release);
    // the app is gone: its ring back to the heap
    let p = RING.swap(null_mut(), Ordering::AcqRel);
    unsafe { wp2_core::heap::free(wp2_core::heap::RES, p.cast()) };
}
