// The mic for apps (docs/abi-1.4.md, "Mic"): 16 kHz 16-bit PCM from the stock audio_record, as the factory mic
// test records (wp2_core::stock, REC_*). Passed on as the recorder makes it: the emulator gives 32000 B/s (mono);
// the factory test splits it as two channels, so an app tells them apart by the rate.
//
// Same safety model as audio.rs: every stock call runs on the UI tick; the app thread only copies out of PCM. The
// recorder lives while the app wants it, runs in the foreground and is not paused, and holds the wake lock. One
// buffer from the media heap holds the stock's ring stream and PCM; it is freed only once the app thread is gone
// (`stop`), so a read can never meet a freed buffer.
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use wp2_core::layout::stub_ram;
use wp2_core::lvcmd::AudioCmd;
use wp2_core::stock::*;

const E_ARG: i32 = -22;
const E_NOMEM: i32 = -12;
const WAKE_ID: u32 = 0xF2;
/// The stock's ring and ours (256 ms of mono, 128 ms of stereo).
const STREAM_SIZE: u32 = 4096;
const PCM_SIZE: u32 = 8192;

static WANT: AtomicBool = AtomicBool::new(false);
static ERROR: AtomicI32 = AtomicI32::new(0);
/// the buffer (0 none): [stream ring | PCM]
static MEM: AtomicU32 = AtomicU32::new(0);
// free-running byte counts of PCM: HEAD written by the UI thread, TAIL by the app thread
static HEAD: AtomicU32 = AtomicU32::new(0);
static TAIL: AtomicU32 = AtomicU32::new(0);
// UI thread only: the recorder and its stream
static REC: AtomicU32 = AtomicU32::new(0);
static STREAM: AtomicU32 = AtomicU32::new(0);

// app thread
pub fn request(cmd: AudioCmd) -> i32 {
    match cmd {
        AudioCmd::MicOpen => {
            ERROR.store(0, Ordering::Relaxed);
            WANT.store(true, Ordering::Release);
            0
        }
        AudioCmd::MicClose => {
            WANT.store(false, Ordering::Release);
            0
        }
        AudioCmd::MicRead { ptr, len } => read(ptr, len),
        _ => E_ARG,
    }
}

/// Bytes copied to `ptr` (whole samples), or the error of the last open.
fn read(ptr: u32, len: u32) -> i32 {
    let e = ERROR.load(Ordering::Relaxed);
    let mem = MEM.load(Ordering::Acquire);
    if e != 0 || mem == 0 {
        return e;
    }
    let ram = stub_ram();
    let (block, size) = (ram.app_block, ram.app_size);
    if ptr < block || ptr - block > size || len > size - (ptr - block) {
        return E_ARG;
    }
    let tail = TAIL.load(Ordering::Relaxed);
    let n = HEAD.load(Ordering::Acquire).wrapping_sub(tail).min(len) & !1;
    let pcm = (mem + STREAM_SIZE) as *const u8;
    for i in 0..n {
        unsafe { *((ptr + i) as *mut u8) = *pcm.add(((tail + i) % PCM_SIZE) as usize) };
    }
    TAIL.store(tail.wrapping_add(n), Ordering::Release);
    n as i32
}

fn close() {
    let h = REC.swap(0, Ordering::Relaxed);
    if h != 0 {
        stock!(REC_STOP, fn(u32) -> i32)(h);
        stock!(REC_DESTROY, fn(u32) -> i32)(h);
        stock!(STREAM_FREE, fn(u32))(STREAM.swap(0, Ordering::Relaxed));
        stock!(WAKE_UNLOCK, fn(u32))(WAKE_ID);
    }
}

fn open() {
    let mut mem = MEM.load(Ordering::Relaxed);
    if mem == 0 {
        mem = stock!(PS_ALLOC, fn(u32) -> u32)(STREAM_SIZE + PCM_SIZE);
        if mem == 0 {
            ERROR.store(E_NOMEM, Ordering::Relaxed);
            return;
        }
        MEM.store(mem, Ordering::Release);
    }
    let stream = stock!(RING_STREAM, fn(u32, u32) -> u32)(mem, STREAM_SIZE);
    let h = if stream == 0 { 0 } else { stock!(REC_CREATE, fn(u32, u32, u32, u32, u32, u32) -> u32)(5, 16, 16, 1, 1, stream) };
    if h == 0 {
        if stream != 0 {
            stock!(STREAM_FREE, fn(u32))(stream);
        }
        ERROR.store(E_NOMEM, Ordering::Relaxed);
        return;
    }
    STREAM.store(stream, Ordering::Relaxed);
    REC.store(h, Ordering::Relaxed);
    // a new recording: nothing old
    TAIL.store(HEAD.load(Ordering::Relaxed), Ordering::Release);
    stock!(REC_START, fn(u32) -> i32)(h);
    stock!(WAKE_LOCK, fn(u32))(WAKE_ID);
}

// UI tick
pub fn tick(running: bool, paused: bool) {
    let want = WANT.load(Ordering::Acquire);
    if !running || paused || !want {
        close();
        return;
    }
    let h = REC.load(Ordering::Relaxed);
    if h == 0 {
        if ERROR.load(Ordering::Relaxed) == 0 {
            open();
        }
        return;
    }
    // what the recorder made, as it is (16-bit samples; the app finds mono or stereo by the rate); when the app
    // has not taken PCM, the newest is dropped
    let mem = MEM.load(Ordering::Relaxed);
    let pcm = (mem + STREAM_SIZE) as *mut u8;
    loop {
        let mut head = HEAD.load(Ordering::Relaxed);
        let room = PCM_SIZE - head.wrapping_sub(TAIL.load(Ordering::Acquire));
        let at = head % PCM_SIZE;
        let want = room.min(PCM_SIZE - at) & !3;
        let have = stock!(STREAM_LENGTH, fn(u32) -> i32)(stock!(REC_STREAM, fn(u32) -> u32)(h));
        if have < 4 || want == 0 {
            break;
        }
        let got = stock!(REC_READ, fn(u32, *mut u8, u32) -> i32)(h, unsafe { pcm.add(at as usize) }, (have as u32).min(want) & !3);
        if got <= 0 {
            break;
        }
        head = head.wrapping_add(got as u32);
        HEAD.store(head, Ordering::Release);
    }
}

// UI thread, app thread aborted
pub fn stop() {
    WANT.store(false, Ordering::Release);
    close();
    ERROR.store(0, Ordering::Relaxed);
    let mem = MEM.swap(0, Ordering::AcqRel);
    if mem != 0 {
        stock!(PS_FREE, fn(u32))(mem);
    }
    HEAD.store(0, Ordering::Release);
    TAIL.store(0, Ordering::Release);
}
