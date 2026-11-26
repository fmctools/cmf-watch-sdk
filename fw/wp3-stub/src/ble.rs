// The install channel on the Watch 3 Pro (c1 = 0xFFFF, c2 = model::WP3.stub_opcodes): the WP2 stub's ble.rs
// protocol (wp2_core::proto) with the Watch 3 Pro differences:
// - DATA is `data_opcode` (0x90A9): stock 1.0.1.77 handles 0x90A2 itself (Do Not Disturb sync, `dnd_handler`) and
//   never sends it down the default branch this hook sits in;
// - the StubVersion reply carries a model byte after the WP2 fields (`MODEL_ID`), which a host reads to pick DATA;
// - a loader package must carry this model in header byte 0x38 (wp2-core port.rs; END answers Invalid otherwise);
// - subset: INFO, StubVersion, BEGIN, DATA, END (full package check), DELETE and PING. MSG and PUSH answer NoApp,
//   nothing is queued for the host (no EVENT drain, no kick) and there are no widget cards to forget: milestone M3.
// Kept apart from the WP2 stub so that one stays byte-identical; the shared parts are wp2-core's proto, pkg, aes and
// widget code. Every stock address comes from model::WP3.
use crate::fs::{self, File};
use core::mem::transmute;
use core::sync::atomic::Ordering;
use wp2_core::aes;
use wp2_core::layout::{
    blob_path, data_path, slot_path, widget_path, StubRam, APP_SIZE, DIR, LOADER_ABI_MINOR, LOADER_SIZE, LOADER_SLOT,
    STUB_VERSION,
};
use wp2_core::model::WP3 as M;
use wp2_core::pkg::{check_slot, crc32, file_size, parse_header, HEADER_SIZE, KIND_APP};
use wp2_core::port::loader_model_ok;
use wp2_core::proto::{parse, Request, Status, DATA, MAX_DATA};
use wp2_core::widget::{trailer_head, trailer_len, TRAILER_HEAD};

/// the model byte of the StubVersion reply: 1 = Watch Pro 2 (whose stub sends none), 2 = Watch 3 Pro
pub const MODEL_ID: u8 = M.model_id as u8;
const EEXIST: i32 = -17;
const HEADER_LEN: usize = 11;
// longest reply: status + 64-byte header
const MAX_REPLY: usize = 1 + HEADER_SIZE;
const APP_SECRET: &[u8] = b"APP_SECRET\0";

pub fn ram() -> &'static mut StubRam {
    unsafe { &mut *(M.stub_ram as *mut StubRam) }
}

// Replaces `bl read_payload` in the stock default branch (`ble_call`); the frame pointer is in r4 there (r5 on WP2).
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wp3_ble_default() {
    core::arch::naked_asm!("mov r2, r4", "b.w {c}", c = sym wp3_ble_c)
}

extern "C" fn wp3_ble_c(buf: *mut u8, len: u16, frame: *const u8) -> i32 {
    let sleep: extern "C" fn(u32) = unsafe { transmute(M.os_sleep as usize) };
    while ram().busy.swap(1, Ordering::Acquire) != 0 {
        sleep(1);
    }
    let rc = frame_c(buf, len, frame);
    ram().busy.store(0, Ordering::Release);
    rc
}

fn ours(c2: u16) -> bool {
    M.stub_opcodes.iter().any(|&op| op == u32::from(c2))
}

fn frame_c(buf: *mut u8, len: u16, frame: *const u8) -> i32 {
    let read: extern "C" fn(*mut u8, u32) -> i32 = unsafe { transmute(M.read_payload as usize) };
    let rc = read(buf, len as u32);
    let f = unsafe { core::slice::from_raw_parts(frame, HEADER_LEN) };
    let c1 = u16::from_le_bytes([f[3], f[4]]);
    let c2 = u16::from_le_bytes([f[9], f[10]]);
    if rc != 0 || c1 != 0xFFFF || !ours(c2) {
        return rc;
    }
    let ram = ram();
    let frame = unsafe { core::slice::from_raw_parts(buf, len as usize) };
    let mut k_plain = [0u8; 160];
    // the stock session first, else the own channel key K (as on WP2, docs/abi-1.4.md)
    let (payload, k): (&[u8], bool) = 'found: {
        if keyed(M.authkey) && keyed(M.session_key) {
            let decrypt: extern "C" fn(*mut u16, *const u8, u32, u32) -> *const u8 =
                unsafe { transmute(M.decrypt as usize) };
            let mut n = 0u16;
            let plain = decrypt(&mut n, buf, len as u32, c2 as u32);
            if !plain.is_null() {
                break 'found (unsafe { core::slice::from_raw_parts(plain, n as usize) }, false);
            }
        }
        if let Some(key) = chan_key(ram) {
            if let Some(n) = aes::open(&key, frame, &mut k_plain) {
                break 'found (&k_plain[..n], true);
            }
        }
        return rc;
    };
    let key = session_word();
    if ram.host_key != key || ram.chan_mode != k as u8 {
        ram.host_key = key;
        ram.chan_mode = k as u8;
    }
    // the shared parser knows DATA by its WP2 opcode
    let request = if u32::from(c2) == M.data_opcode { DATA } else { c2 };
    let mut out = [0u8; MAX_REPLY];
    let size = handle(ram, request, payload, &mut out);
    let _ = reply(ram, c2, &out[..size]);
    rc
}

fn session_word() -> u32 {
    unsafe { (M.session_key as *const u32).read_unaligned() }
}

// K, read once from NVRAM (the secret only changes with a factory reset, which also reboots)
fn chan_key(ram: &mut StubRam) -> Option<[u8; 16]> {
    if ram.chan_ready != 1 {
        let get: extern "C" fn(*const u8, *mut u8, u32) -> i32 = unsafe { transmute(M.kv_get as usize) };
        let mut hex = [0u8; 32];
        if get(APP_SECRET.as_ptr(), hex.as_mut_ptr(), 32) != 32 {
            return None;
        }
        ram.chan_key = aes::channel_key(&hex)?;
        ram.chan_ready = 1;
    }
    Some(ram.chan_key)
}

fn keyed(addr: u32) -> bool {
    unsafe { core::slice::from_raw_parts(addr as *const u8, 16) }.iter().any(|&b| b != 0)
}

// true once the frame is in the stream; the reply opcode is the request's | 0xA000 (DATA: 0xB0A9)
fn reply(ram: &mut StubRam, c2: u16, plain: &[u8]) -> bool {
    let encrypt: extern "C" fn(*mut u16, *const u8, u32, u32) -> *const u8 =
        unsafe { transmute(M.encrypt as usize) };
    let send: extern "C" fn(*const u8, u32) -> i32 = unsafe { transmute(M.send as usize) };
    let reply_c2 = c2 | 0xA000;
    let mut n = 0u16;
    let mut sealed = [0u8; 160];
    let cipher = if ram.chan_mode == 1 {
        n = aes::seal(&ram.chan_key, plain, &mut sealed) as u16;
        if n == 0 { core::ptr::null() } else { sealed.as_ptr() }
    } else {
        encrypt(&mut n, plain.as_ptr(), plain.len() as u32, reply_c2 as u32)
    };
    if cipher.is_null() || HEADER_LEN + n as usize > ram.reply.len() {
        return false;
    }
    let r = &mut ram.reply;
    r[0] = 0xF5;
    r[1..3].copy_from_slice(&n.to_be_bytes());
    r[3..5].copy_from_slice(&0xFFFFu16.to_be_bytes());
    r[5..9].fill(0);
    r[9..11].copy_from_slice(&reply_c2.to_be_bytes());
    unsafe { core::ptr::copy_nonoverlapping(cipher, r[HEADER_LEN..].as_mut_ptr(), n as usize) };
    send(r.as_ptr(), (HEADER_LEN + n as usize) as u32) == 0
}

fn status(out: &mut [u8], s: Status) -> usize {
    out[0] = s as u8;
    1
}

// the package of the loader or of the running app is in use (DELETE refuses then; an install replaces the files, the
// running copies live in RES heap blocks)
fn busy(ram: &StubRam, slot: u8) -> bool {
    if slot == LOADER_SLOT { ram.loader_active != 0 } else { slot == ram.running_slot }
}

fn handle(ram: &mut StubRam, c2: u16, payload: &[u8], out: &mut [u8; MAX_REPLY]) -> usize {
    use wp2_core::proto::{DELETE, INFO};
    if c2 == INFO || c2 == DELETE {
        abort(ram); // a host only sends these between transfers, so an open one was abandoned
    }
    match parse(c2, payload) {
        Err(s) => status(out, s),
        Ok(Request::Info(slot)) => info(slot, out),
        // status, the stub's release (u32 LE), the highest package abi_minor it accepts, then the model byte
        Ok(Request::StubVersion) => {
            out[0] = Status::Ok as u8;
            out[1..5].copy_from_slice(&STUB_VERSION.to_le_bytes());
            out[5] = LOADER_ABI_MINOR as u8;
            out[6] = MODEL_ID;
            7
        }
        Ok(Request::Begin { slot, size, crc }) => status(out, begin(ram, slot, size, crc)),
        Ok(Request::Data { offset, bytes }) => {
            let s = data(ram, offset, bytes);
            out[1..5].copy_from_slice(&ram.xfer_written.to_le_bytes());
            out[0] = s as u8;
            5
        }
        Ok(Request::End) => status(out, end(ram)),
        Ok(Request::Delete(slot)) => {
            let s = if busy(ram, slot) {
                Status::Busy
            } else if fs::unlink(&slot_path(slot, false)) < 0 {
                Status::Io
            } else {
                drop_data(slot);
                Status::Ok
            };
            status(out, s)
        }
        // no loader on this model yet: nothing runs, nothing takes a message or a widget record
        Ok(Request::Ping) => {
            out[..3].copy_from_slice(&[Status::Ok as u8, ram.running_slot, ram.app_flags]);
            3
        }
        Ok(Request::Msg { .. }) | Ok(Request::Push { .. }) => status(out, Status::NoApp),
    }
}

fn drop_data(slot: u8) {
    fs::unlink(&data_path(slot, false));
    fs::unlink(&data_path(slot, true));
    fs::unlink(&widget_path(slot));
    fs::unlink(&blob_path(slot));
}

fn info(slot: u8, out: &mut [u8; MAX_REPLY]) -> usize {
    let mut file: File = [0; M.fs_file as usize];
    out[0] = Status::Ok as u8;
    if fs::open(&mut file, &slot_path(slot, false), fs::READ) < 0 {
        return 1;
    }
    let ok = fs::read_exact(&mut file, &mut out[1..]);
    fs::close(&mut file);
    if ok { MAX_REPLY } else { 1 }
}

fn abort(ram: &mut StubRam) {
    if ram.xfer_state != 0 {
        fs::close(&mut ram.file);
        fs::unlink(&slot_path(ram.xfer_slot, true));
        ram.xfer_state = 0;
    }
}

fn begin(ram: &mut StubRam, slot: u8, size: u32, crc: u32) -> Status {
    // the loader while its view is open, or the running app's package. The WP2 stub takes such an install only for a
    // page behind the face (its `behind_face`); this model has no pages, so the loaded loader is always in the
    // foreground or under a view on top of it, and a reload at its next FOCUS would end a running app
    if busy(ram, slot) {
        return Status::Busy;
    }
    let limit = if slot == LOADER_SLOT { LOADER_SIZE } else { APP_SIZE };
    if (size as usize) < HEADER_SIZE || size > HEADER_SIZE as u32 + 2 * limit {
        return Status::BadRequest;
    }
    abort(ram);
    let rc = fs::mkdir(DIR);
    if rc < 0 && rc != EEXIST {
        return Status::Io;
    }
    let tmp = slot_path(slot, true);
    fs::unlink(&tmp);
    if fs::open(&mut ram.file, &tmp, fs::WRITE | fs::CREATE) < 0 {
        return Status::Io;
    }
    ram.xfer_state = 1;
    ram.xfer_slot = slot;
    ram.xfer_size = size;
    ram.xfer_crc = crc;
    ram.xfer_written = 0;
    Status::Ok
}

fn data(ram: &mut StubRam, offset: u32, bytes: &[u8]) -> Status {
    if ram.xfer_state != 1 || offset != ram.xfer_written {
        return Status::Sequence;
    }
    if bytes.len() > MAX_DATA || ram.xfer_written as usize + bytes.len() > ram.xfer_size as usize {
        return Status::BadRequest;
    }
    if fs::write(&mut ram.file, bytes) != bytes.len() as i32 {
        abort(ram);
        return Status::Io;
    }
    ram.xfer_written += bytes.len() as u32;
    Status::Ok
}

fn end(ram: &mut StubRam) -> Status {
    if ram.xfer_state != 1 || ram.xfer_written != ram.xfer_size {
        return Status::Sequence;
    }
    fs::close(&mut ram.file);
    let slot = ram.xfer_slot;
    let tmp = slot_path(slot, true);
    let s = match verify(ram, &tmp) {
        Status::Ok => {
            if name(&slot_path(slot, false)) != name(&tmp) {
                drop_data(slot);
            }
            fs::unlink(&slot_path(slot, false));
            if fs::rename(&tmp, &slot_path(slot, false)) < 0 { Status::Io } else { Status::Ok }
        }
        s => s,
    };
    if s != Status::Ok {
        fs::unlink(&tmp);
    } else if busy(ram, slot) {
        ram.reload = 1;
    }
    ram.xfer_state = 0;
    s
}

// the package name field, zeros if the file is missing
fn name(path: &[u8]) -> [u8; 24] {
    let mut file: File = [0; M.fs_file as usize];
    let mut raw = [0u8; 0x20];
    if fs::open(&mut file, path, fs::READ) >= 0 {
        if !fs::read_exact(&mut file, &mut raw) {
            raw = [0; 0x20];
        }
        fs::close(&mut file);
    }
    raw[8..].try_into().unwrap()
}

// Re-reads the closed file (as the WP2 stub): CRC of all of it, header checks, size, CRC of image and relocations;
// a bad widget-icon trailer is neutralised and the app still installs.
fn verify(ram: &mut StubRam, tmp: &[u8]) -> Status {
    if fs::open(&mut ram.file, tmp, fs::READ) < 0 {
        return Status::Io;
    }
    let (s, bad_trailer) = scan(ram);
    fs::close(&mut ram.file);
    if s == Status::Ok && bad_trailer != 0 {
        if fs::open(&mut ram.file, tmp, fs::READ | fs::WRITE) < 0 {
            return Status::Io;
        }
        let ok = fs::seek(&mut ram.file, bad_trailer) == 0 && fs::write(&mut ram.file, &[0; 4]) == 4;
        if fs::close(&mut ram.file) < 0 || !ok {
            return Status::Io;
        }
    }
    s
}

// (status, offset of a bad trailer's magic or 0)
fn scan(ram: &mut StubRam) -> (Status, u32) {
    let mut raw = [0u8; HEADER_SIZE];
    if !fs::read_exact(&mut ram.file, &mut raw) {
        return (Status::Io, 0);
    }
    // a loader must be this model's (byte 0x38), an app carries none
    let h = loader_model_ok(&M, &raw)
        .and_then(|clean| parse_header(&clean).ok())
        .filter(|h| check_slot(h, ram.xfer_slot).is_ok());
    let body_len = h.as_ref().map_or(0, |h| file_size(h) as usize - HEADER_SIZE);
    let (mut all, mut body, mut tcrc) = (crc32(0, &raw), 0u32, 0u32);
    let mut head = [0u8; TRAILER_HEAD];
    let mut off = 0;
    let size = ram.xfer_size as usize - HEADER_SIZE;
    while off < size {
        let chunk = &mut ram.scratch[..(size - off).min(0x200)];
        if !fs::read_exact(&mut ram.file, chunk) {
            return (Status::Io, 0);
        }
        all = crc32(all, chunk);
        let b = body_len.saturating_sub(off).min(chunk.len());
        body = crc32(body, &chunk[..b]);
        let t = (off + b).saturating_sub(body_len);
        let hn = TRAILER_HEAD.saturating_sub(t).min(chunk.len() - b);
        head[t.min(TRAILER_HEAD)..t.min(TRAILER_HEAD) + hn].copy_from_slice(&chunk[b..b + hn]);
        tcrc = crc32(tcrc, &chunk[b + hn..]);
        off += chunk.len();
    }
    if all != ram.xfer_crc {
        return (Status::Crc, 0);
    }
    let Some(h) = h else { return (Status::Invalid, 0) };
    if body_len > size || body != h.body_crc {
        return (Status::Invalid, 0);
    }
    let rest = size - body_len;
    let good = rest == 0 || trailer_head(&head).is_some_and(|(n, crc)| rest == trailer_len(n) && crc == tcrc);
    let bad = if good || rest < 4 || h.kind != KIND_APP { 0 } else { file_size(&h) };
    if !good && h.kind != KIND_APP {
        return (Status::Invalid, 0);
    }
    (Status::Ok, bad)
}
