// Own install channel on c1 = 0xFFFF, c2 = 0x90A0..=0x90A4 (docs/specs; Status and requests in wp2_core::proto).
use core::mem::transmute;
use core::sync::atomic::Ordering;
use wp2_core::fs::{self, File};
use wp2_core::chan::{listening, session_word, MSG_MAX};
use wp2_core::layout::{
    blob_path, data_path, slot_path, widget_path, stub_ram, StubRam, APP_PAUSED, APP_SIZE, APP_V11, DIR,
    LOADER_ABI_MINOR, LOADER_SIZE,
    LOADER_SLOT, STUB_VERSION,
};
use wp2_core::pkg::{check_slot, crc32, file_size, parse_header, HEADER_SIZE, KIND_APP};
use wp2_core::widget::{trailer_head, trailer_len, TRAILER_HEAD};
use wp2_core::proto::{parse, push_wait, Request, Status, ANY_SLOT, DELETE, EVENT, INFO, MAX_DATA, PUSH, PUSH_WIDGET};
use wp2_core::aes;
use wp2_core::cards::{page_slot, APP_WIDGET, PAGE_VIEW};
use wp2_core::stock::{
    APP_SECRET, AUTHKEY, CARDS_SAVE, CARDS_STORE, KV_GET, OS_SLEEP, SESSION_KEY, STREAM_SPACE, SYS_CLOCK_TICK_GET,
    WEWEAR_CTX,
};

const READ_PAYLOAD: u32 = 0x100CC285;
const DECRYPT: u32 = 0x101B608D;
const ENCRYPT: u32 = 0x101B605B;
const SEND: u32 = 0x100CC2D5;
// stock key buffers (docs/recon.md): 0x804B answers with whatever authkey is set, all zero after boot or after the
// bind popup is confirmed, so a session counts only when both keys are non-zero. This only closes the zero-key
// path: any client in BLE range reads the pairing secret (AT GETSECRET, also on the real watch) and binds without
// a tap, so it can install silently; same exposure as stock OTA, accepted by the owner (docs/recon.md).
const EEXIST: i32 = -17;
const HEADER_LEN: usize = 11;
// longest reply: status + 64-byte header
const MAX_REPLY: usize = 1 + HEADER_SIZE;

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wp2_ble_default() {
    core::arch::naked_asm!("mov r2, r5", "b.w {c}", c = sym wp2_ble_c)
}

// Replaces `bl 0x100CC284` (read the payload into buf) in the stock default branch; frame is host-order.
extern "C" fn wp2_ble_c(buf: *mut u8, len: u16, frame: *const u8) -> i32 {
    // the app thread may be draining (wp2_kick): wait for it, it never blocks on the stream
    let sleep: extern "C" fn(u32) = unsafe { transmute(OS_SLEEP as usize) };
    while stub_ram().busy.swap(1, Ordering::Acquire) != 0 {
        sleep(1);
    }
    let rc = frame_c(buf, len, frame);
    stub_ram().busy.store(0, Ordering::Release);
    rc
}

/// Called by the loader on the app thread right after the app queued a message (api >= 2): a K host gets it at
/// once, so a phone app asleep in the background is woken by the notification instead of having to poll. The
/// stock sends from other threads too (the bind popup's 0x0049 reply goes out from the UI thread); the stock
/// crypto buffers are not touched here, K frames are sealed in software.
pub extern "C" fn wp2_kick() {
    let ram = stub_ram();
    if ram.busy.swap(1, Ordering::Acquire) != 0 {
        return; // wewear_read is in the frame path and drains after its reply
    }
    if ram.chan_mode == 1 {
        drain(ram);
    }
    ram.busy.store(0, Ordering::Release);
}

fn frame_c(buf: *mut u8, len: u16, frame: *const u8) -> i32 {
    let read: extern "C" fn(*mut u8, u32) -> i32 = unsafe { transmute(READ_PAYLOAD as usize) };
    let rc = read(buf, len as u32);
    let f = unsafe { core::slice::from_raw_parts(frame, HEADER_LEN) };
    let c1 = u16::from_le_bytes([f[3], f[4]]);
    let c2 = u16::from_le_bytes([f[9], f[10]]);
    if rc != 0 || c1 != 0xFFFF || !(INFO..=PUSH).contains(&c2) {
        return rc;
    }
    let ram = stub_ram();
    let frame = unsafe { core::slice::from_raw_parts(buf, len as usize) };
    let mut k_plain = [0u8; 160];
    // the stock session first (Mac tools); else the own channel key K (docs/abi-1.4.md): no bind, so a phone app
    // works next to Nothing X, whose stock session stays untouched
    let (payload, k): (&[u8], bool) = 'found: {
        if keyed(AUTHKEY) && keyed(SESSION_KEY) {
            let decrypt: extern "C" fn(*mut u16, *const u8, u32, u32) -> *const u8 =
                unsafe { transmute(DECRYPT as usize) };
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
    // also under K: the loader lets the app send only while session_word() == host_key (svc.rs, loader 1.3 on the
    // watch), so a K host is "listening" while the stock session it rides next to stays the same. Needs a stock
    // bind since the factory reset (NVRAM APP_SESSION_KEY), which a watch paired with Nothing X always has.
    let key = session_word();
    if ram.host_key != key || ram.chan_mode != k as u8 {
        ram.outbox.clear();
        ram.host_key = key;
        ram.chan_mode = k as u8;
    }
    let mut out = [0u8; MAX_REPLY];
    let size = handle(ram, c2, payload, &mut out);
    let _ = reply(ram, c2, &out[..size]);
    drain(ram);
    rc
}

// K, read once from NVRAM (the secret only changes with a factory reset, which also reboots)
fn chan_key(ram: &mut StubRam) -> Option<[u8; 16]> {
    if ram.chan_ready != 1 {
        let get: extern "C" fn(*const u8, *mut u8, u32) -> i32 = unsafe { transmute(KV_GET as usize) };
        let mut hex = [0u8; 32];
        if get(APP_SECRET.as_ptr(), hex.as_mut_ptr(), 32) != 32 {
            return None;
        }
        ram.chan_key = aes::channel_key(&hex)?;
        ram.chan_ready = 1;
    }
    Some(ram.chan_key)
}

// a host listens: the stream is connected and the stock session word is still the one of the last wp2 frame; a
// stock host also needs the authkey (a confirmed bind popup zeroes it)
fn host_listens(ram: &StubRam) -> bool {
    listening(ram.host_key) && (ram.chan_mode == 1 || keyed(AUTHKEY))
}

fn keyed(addr: u32) -> bool {
    unsafe { core::slice::from_raw_parts(addr as *const u8, 16) }.iter().any(|&b| b != 0)
}

// true once the frame is in the stream; the stock send waits up to 1 s for room and writes all or nothing
fn reply(ram: &mut StubRam, c2: u16, plain: &[u8]) -> bool {
    let encrypt: extern "C" fn(*mut u16, *const u8, u32, u32) -> *const u8 = unsafe { transmute(ENCRYPT as usize) };
    let send: extern "C" fn(*const u8, u32) -> i32 = unsafe { transmute(SEND as usize) };
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

// the package of the loader or of the running app is in use (DELETE refuses then)
fn busy(ram: &StubRam, slot: u8) -> bool {
    if slot == LOADER_SLOT { ram.loader_active != 0 } else { slot == ram.running_slot }
}

// Stub 6: the loader is held only by an app page that is not in view (behind the face). Its code and the app's run
// from RES heap copies, not from the files, so an install may replace them; `end` marks the page for a reload on
// its next FOCUS (view.rs). The menu (0xF0) or a page in view stays BUSY: that is an app in the foreground.
fn behind_face(ram: &StubRam) -> bool {
    ram.loader_active != 0 && page_slot(u32::from(ram.loader_view)).is_some() && ram.loader_focus == 0
}

fn status(out: &mut [u8], s: Status) -> usize {
    out[0] = s as u8;
    1
}

fn handle(ram: &mut StubRam, c2: u16, payload: &[u8], out: &mut [u8; MAX_REPLY]) -> usize {
    if c2 == INFO || c2 == DELETE {
        abort(ram); // a host only sends these between transfers, so an open one was abandoned
    }
    match parse(c2, payload) {
        Err(s) => status(out, s),
        Ok(Request::Info(slot)) => info(slot, out),
        // status, the stub's release (u32 LE), the highest package abi_minor it accepts
        Ok(Request::StubVersion) => {
            out[0] = Status::Ok as u8;
            out[1..5].copy_from_slice(&STUB_VERSION.to_le_bytes());
            out[5] = LOADER_ABI_MINOR as u8;
            6
        }
        Ok(Request::Begin { slot, size, crc }) => {
            let s = begin(ram, slot, size, crc);
            status(out, s)
        }
        Ok(Request::Data { offset, bytes }) => {
            let s = data(ram, offset, bytes);
            out[1..5].copy_from_slice(&ram.xfer_written.to_le_bytes());
            out[0] = s as u8;
            5
        }
        Ok(Request::End) => {
            let s = end(ram);
            status(out, s)
        }
        Ok(Request::Delete(slot)) => {
            // stub 7: a page's app behind the face goes with its page (forget), as "Remove page" + Delete on the watch
            let s = if busy(ram, slot) && (slot == LOADER_SLOT || !behind_face(ram)) {
                Status::Busy
            } else if fs::unlink(&slot_path(slot, false)) < 0 {
                Status::Io
            } else {
                forget(slot);
                drop_data(slot);
                Status::Ok
            };
            status(out, s)
        }
        Ok(Request::Msg { slot, data }) => status(out, msg(ram, slot, data)),
        Ok(Request::Ping) => {
            out[..3].copy_from_slice(&[Status::Ok as u8, ram.running_slot, ram.app_flags]);
            3
        }
        Ok(Request::Push { slot, kind, data }) => push(ram, slot, kind, data, out),
    }
}

// only to the running ABI 1.1 app, and not while a stock popup covers it
fn msg(ram: &mut StubRam, slot: u8, data: &[u8]) -> Status {
    let running = ram.running_slot;
    if ram.app_flags & APP_V11 == 0 || running == LOADER_SLOT || (slot != ANY_SLOT && slot != running) {
        Status::NoApp
    } else if ram.app_flags & APP_PAUSED != 0 {
        Status::Paused
    } else if !ram.inbox.push(running, data) {
        Status::Full
    } else {
        Status::Ok
    }
}

// Stub 7: the app's widget (0x40 / slot + 1) and page (view 0xF8 + slot) leave the cards, stored as the phone's 0x906D
// set does (same BLE thread). Same result as wp2_core::cards::forget, which does not fit in the stub's 0x3000 bytes
// (unrolled loops, bounds-check formatting): plain loops over the 8 × 12-byte entries in place.
#[inline(never)]
fn forget(slot: u8) {
    let c = CARDS_STORE as *mut u8;
    let mut changed = false;
    let mut i = 0;
    while i < 96 {
        let e = unsafe { c.add(i) };
        let drop = match unsafe { *e } {
            1 => unsafe {
                let mut n = usize::from(*e.add(2)).min(4);
                let mut k = 0;
                while k < n {
                    let p = e.add(3 + 2 * k);
                    if *p == APP_WIDGET && *p.add(1) == slot + 1 {
                        n -= 1;
                        shift(p, 2 * (n - k) + 2, 2);
                        changed = true;
                    } else {
                        k += 1;
                    }
                }
                *e.add(2) = n as u8;
                n == 0
            },
            2 => unsafe { u16::from_le_bytes([*e.add(2), *e.add(3)]) == PAGE_VIEW + u16::from(slot) },
            _ => break,
        };
        if drop {
            unsafe { shift(e, 96 - i, 12) };
            changed = true;
        } else {
            i += 12;
        }
    }
    if changed {
        let save: extern "C" fn(u32, u32) -> u32 = unsafe { transmute(CARDS_SAVE as usize) };
        save(0xE4, 0);
    }
}

// drops `by` bytes at `p` from the `len` bytes there; the freed tail becomes zero
#[inline(never)]
unsafe fn shift(p: *mut u8, len: usize, by: usize) {
    for j in 0..len {
        unsafe { *p.add(j) = if j + by < len { *p.add(j + by) } else { 0 } };
    }
}

fn drop_data(slot: u8) {
    fs::unlink(&data_path(slot, false));
    fs::unlink(&data_path(slot, true));
    fs::unlink(&widget_path(slot));
    fs::unlink(&blob_path(slot));
}

// Stub 8: the phone sets an app's widget record or start blob, running or not (docs/widget-push.md). The same bytes
// again are no write; a changed record is written at most once per PUSH_EVERY_MS (each one is a NAND write).
fn push(ram: &mut StubRam, slot: u8, kind: u8, data: &[u8], out: &mut [u8; MAX_REPLY]) -> usize {
    let mut file: File = [0; 0x2C];
    if fs::open(&mut file, &slot_path(slot, false), fs::READ) < 0 {
        return status(out, Status::NoApp);
    }
    fs::close(&mut file);
    let path = if kind == PUSH_WIDGET { widget_path(slot) } else { blob_path(slot) };
    let mut old = [0u8; MSG_MAX + 1];
    let mut n = 0;
    if fs::open(&mut file, &path, fs::READ) >= 0 {
        n = fs::read(&mut file, &mut old).clamp(0, old.len() as i32) as usize;
        fs::close(&mut file);
    }
    if &old[..n] == data {
        return status(out, Status::Ok); // a missing file is the empty record
    }
    let tick: extern "C" fn() -> u64 = unsafe { transmute(SYS_CLOCK_TICK_GET as usize) };
    let now = tick() as u32;
    let at = &mut ram.push_at[usize::from(slot)][usize::from(kind)];
    let wait = push_wait(*at, now);
    if wait > 0 {
        out[0] = Status::Later as u8;
        out[1..5].copy_from_slice(&wait.to_le_bytes());
        return 5;
    }
    *at = now | 1; // a failed write counts too: a broken NAND is not retried every frame
    fs::unlink(&path);
    if !data.is_empty() {
        if fs::open(&mut file, &path, fs::WRITE | fs::CREATE) < 0 {
            return status(out, Status::Io);
        }
        let ok = fs::write(&mut file, data) == data.len() as i32;
        if fs::close(&mut file) < 0 || !ok {
            fs::unlink(&path);
            return status(out, Status::Io);
        }
    }
    status(out, Status::Ok)
}

// EVENT frames go out right after the reply to a host frame, from wewear_read: the thread that runs the stock
// dispatcher and owns the stock encrypt buffers. That thread blocks in the stream read while a host is connected,
// so the host polls (PING) to collect them. A new host key drops what an earlier host left queued.
fn drain(ram: &mut StubRam) {
    while let Some(m) = ram.outbox.peek(0).copied() {
        if host_listens(ram) {
            let mut plain = [0u8; 2 + MSG_MAX];
            plain[0] = ram.out_seq;
            plain[1] = m.slot;
            let n = usize::from(m.len).min(MSG_MAX);
            // the stream writes what fits: a frame only when all of it fits, else the next pass
            let frame = HEADER_LEN + (2 + n + 4) / 16 * 16 + 16;
            let stream = unsafe { *((*(WEWEAR_CTX as *const u32) + 0x18) as *const u32) };
            let space: extern "C" fn(u32) -> i32 = unsafe { transmute(STREAM_SPACE as usize) };
            if space(stream) < frame as i32 {
                return;
            }
            plain[2..2 + n].copy_from_slice(&m.data[..n]);
            // a full stream keeps the message for the next pass
            if !reply(ram, EVENT, &plain[..2 + n]) {
                return;
            }
            ram.out_seq = ram.out_seq.wrapping_add(1);
            ram.outbox.pop();
        } else {
            ram.outbox.clear();
        }
    }
}

fn info(slot: u8, out: &mut [u8; MAX_REPLY]) -> usize {
    let mut file: File = [0; 0x2C];
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
    if busy(ram, slot) && !behind_face(ram) {
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
    if ram.xfer_state != 1 {
        return Status::Sequence;
    }
    if offset != ram.xfer_written {
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
    let mut file: File = [0; 0x2C];
    let mut raw = [0u8; 0x20];
    if fs::open(&mut file, path, fs::READ) >= 0 {
        if !fs::read_exact(&mut file, &mut raw) {
            raw = [0; 0x20];
        }
        fs::close(&mut file);
    }
    raw[8..].try_into().unwrap()
}

// Re-reads the closed file: CRC of all of it, header checks, size, CRC of image and relocation table. Stub 10: bytes
// after the relocation table are the widget icon trailer (docs/app-icons.md), outside body_crc; a bad one is
// neutralised (its magic zeroed) and the app still installs, without icons.
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
    // checked first: file_size of an unchecked header may overflow
    let h = parse_header(&raw).ok().filter(|h| check_slot(h, ram.xfer_slot).is_ok());
    // image + relocations; the rest is the trailer
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
        let t = (off + b).saturating_sub(body_len); // trailer offset of chunk[b]
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
        return (Status::Invalid, 0); // the loader carries no icons: its size stays exact
    }
    (Status::Ok, bad)
}

