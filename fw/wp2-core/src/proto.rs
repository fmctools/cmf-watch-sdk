use crate::chan::MSG_MAX;
use crate::layout::{LOADER_SLOT, SLOTS, STUB_SLOT};

pub const MAX_DATA: usize = 128;
pub const INFO: u16 = 0x90A0;
pub const BEGIN: u16 = 0x90A1;
pub const DATA: u16 = 0x90A2;
pub const END: u16 = 0x90A3;
pub const DELETE: u16 = 0x90A4;
pub const MSG: u16 = 0x90A5;
// watch -> host, unsolicited: seq, slot, data (docs/abi-1.1.md)
pub const EVENT: u16 = 0xB0A6;
pub const ANY_SLOT: u8 = 0xFF;
// keep-alive: refreshes the host key, answers running_slot and app_flags (0x90A6 is skipped: its reply is EVENT)
pub const PING: u16 = 0x90A7;
// stub 8, host -> stub: an app's widget record or start blob while it does not run (docs/widget-push.md)
pub const PUSH: u16 = 0x90A8;
pub const PUSH_WIDGET: u8 = 0;
pub const PUSH_BLOB: u8 = 1;
// one write per slot and kind per minute: each one is a NAND write
pub const PUSH_EVERY_MS: u32 = 60_000;

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum Status {
    Ok = 0,
    BadRequest = 1,
    Io = 2,
    Sequence = 3,
    Crc = 4,
    Busy = 5,
    Invalid = 6,
    NoApp = 7,
    Full = 8,
    Paused = 9,
    // PUSH sooner than PUSH_EVERY_MS after the last write of that record; the reply carries the ms left
    Later = 10,
}

#[derive(Debug, PartialEq)]
pub enum Request<'a> {
    Info(u8),
    StubVersion,
    Begin { slot: u8, size: u32, crc: u32 },
    Data { offset: u32, bytes: &'a [u8] },
    End,
    Delete(u8),
    Msg { slot: u8, data: &'a [u8] },
    Ping,
    Push { slot: u8, kind: u8, data: &'a [u8] },
}

fn slot(raw: u8) -> Result<u8, Status> {
    if raw < SLOTS || raw == LOADER_SLOT { Ok(raw) } else { Err(Status::BadRequest) }
}

fn word(raw: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(raw[off..off + 4].try_into().unwrap())
}

pub fn parse(c2: u16, payload: &[u8]) -> Result<Request<'_>, Status> {
    match (c2, payload.len()) {
        (INFO, 1) if payload[0] == STUB_SLOT => Ok(Request::StubVersion),
        (INFO, 1) => Ok(Request::Info(slot(payload[0])?)),
        (BEGIN, 9) => Ok(Request::Begin { slot: slot(payload[0])?, size: word(payload, 1), crc: word(payload, 5) }),
        (DATA, 5..=0x84) => Ok(Request::Data { offset: word(payload, 0), bytes: &payload[4..] }),
        // the stock decrypt helper returns NULL for an empty plaintext, so END carries one dummy byte
        (END, 1) => Ok(Request::End),
        (DELETE, 1) => Ok(Request::Delete(slot(payload[0])?)),
        (PING, 1) => Ok(Request::Ping),
        // empty data removes the record; a widget record is the whole file: v1 (64 B) or, stub 9, a well-formed v2
        // (up to 128 B; icon indices are checked against the package when the card is drawn); a blob up to MSG_MAX
        (PUSH, n) if n >= 2 && payload[0] < SLOTS => match (payload[1], n - 2) {
            (PUSH_WIDGET, 0) | (PUSH_BLOB, 0..=MSG_MAX) => {
                Ok(Request::Push { slot: payload[0], kind: payload[1], data: &payload[2..] })
            }
            (PUSH_WIDGET, _) if crate::widget::card(&payload[2..], crate::widget::ICONS_MAX).is_some() => {
                Ok(Request::Push { slot: payload[0], kind: payload[1], data: &payload[2..] })
            }
            _ => Err(Status::BadRequest),
        },
        (MSG, n) if (2..=1 + MSG_MAX).contains(&n) && (payload[0] < SLOTS || payload[0] == ANY_SLOT) => Ok(Request::Msg { slot: payload[0], data: &payload[1..] }),
        _ => Err(Status::BadRequest),
    }
}

// ms left before a PUSH may write the record last written at `last` (millis | 1; 0 = not since boot)
pub fn push_wait(last: u32, now: u32) -> u32 {
    if last == 0 { 0 } else { PUSH_EVERY_MS.saturating_sub(now.wrapping_sub(last)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_malformed() {
        assert_eq!(parse(0x90A0, &[]), Err(Status::BadRequest));
        assert_eq!(parse(0x90A0, &[0, 0]), Err(Status::BadRequest));
        assert_eq!(parse(0x90A2, &[0; 4]), Err(Status::BadRequest));
        assert_eq!(parse(0x90A3, &[]), Err(Status::BadRequest));
        assert_eq!(parse(0x90A3, &[0, 0]), Err(Status::BadRequest));
        assert_eq!(parse(0x90A3, &[0]), Ok(Request::End));
        assert_eq!(parse(0x90A6, &[]), Err(Status::BadRequest));
        assert_eq!(parse(0x90A4, &[16]), Err(Status::BadRequest));
    }

    #[test]
    fn msg_bounds() {
        assert_eq!(parse(MSG, &[0xFF, 7]), Ok(Request::Msg { slot: 0xFF, data: &[7] }));
        assert!(matches!(parse(MSG, &[3; 1 + MSG_MAX]), Ok(Request::Msg { slot: 3, .. })));
        assert_eq!(parse(MSG, &[3; 2 + MSG_MAX]), Err(Status::BadRequest));
        assert_eq!(parse(MSG, &[3]), Err(Status::BadRequest));
        assert_eq!(parse(MSG, &[16, 1]), Err(Status::BadRequest));
        assert!(matches!(parse(MSG, &[15, 1]), Ok(Request::Msg { slot: 15, .. })));
        assert_eq!(parse(PING, &[0]), Ok(Request::Ping));
        assert_eq!(parse(0x90A6, &[0]), Err(Status::BadRequest));
    }

    #[test]
    fn push_bounds() {
        let mut p = [0u8; 2 + 128];
        (p[0], p[1]) = (15, PUSH_WIDGET);
        assert_eq!(parse(PUSH, &p[..66]), Ok(Request::Push { slot: 15, kind: 0, data: &p[2..66] }));
        assert_eq!(parse(PUSH, &p[..2]), Ok(Request::Push { slot: 15, kind: 0, data: &[] }));
        assert_eq!(parse(PUSH, &p[..65]), Err(Status::BadRequest));
        // stub 9: a v2 record (up to 128 B) only when well-formed
        let mut v2 = [0u8; 2 + 3 + 21];
        v2[..5].copy_from_slice(&[15, PUSH_WIDGET, 0xFF, 1, 1]);
        v2[5 + 10] = 0xFF;
        assert_eq!(parse(PUSH, &v2), Ok(Request::Push { slot: 15, kind: 0, data: &v2[2..] }), "one column, 24 B");
        v2[5] = b'x';
        v2[5..15].fill(b'x');
        assert_eq!(parse(PUSH, &v2), Err(Status::BadRequest), "caption without a NUL");
        assert_eq!(parse(PUSH, &[15, PUSH_WIDGET, 0xFF, 9]), Err(Status::BadRequest), "unknown template");
        p[1] = PUSH_BLOB;
        assert!(matches!(parse(PUSH, &p), Ok(Request::Push { kind: 1, .. })));
        assert_eq!(parse(PUSH, &[15, PUSH_BLOB, 9]), Ok(Request::Push { slot: 15, kind: 1, data: &[9] }));
        assert_eq!(parse(PUSH, &[16, PUSH_BLOB, 9]), Err(Status::BadRequest));
        assert_eq!(parse(PUSH, &[0xFF, PUSH_BLOB, 9]), Err(Status::BadRequest));
        assert_eq!(parse(PUSH, &[0, 2, 9]), Err(Status::BadRequest));
        assert_eq!(parse(PUSH, &[0]), Err(Status::BadRequest));
    }

    #[test]
    fn push_rate() {
        assert_eq!(push_wait(0, 5), 0);
        assert_eq!(push_wait(1001, 1001 + PUSH_EVERY_MS), 0);
        assert_eq!(push_wait(1001, 1001 + 1000), PUSH_EVERY_MS - 1000);
        assert_eq!(push_wait(u32::MAX, 100), PUSH_EVERY_MS - 101, "wraps");
    }

    #[test]
    fn data_bounds() {
        assert_eq!(parse(0x90A2, &[4, 0, 0, 0, 7]), Ok(Request::Data { offset: 4, bytes: &[7] }));
        assert!(matches!(parse(0x90A2, &[0; 4 + MAX_DATA]), Ok(Request::Data { .. })));
        assert_eq!(parse(0x90A0, &[7]), Ok(Request::Info(7)));
        assert_eq!(parse(0x90A0, &[0xFE]), Ok(Request::StubVersion));
        assert_eq!(parse(0x90A0, &[0xFD]), Err(Status::BadRequest));
    }

    #[test]
    fn parse_requests() {
        assert_eq!(parse(0x90A1, &[3, 0x10, 0, 0, 0, 0xEF, 0xBE, 0xAD, 0xDE]), Ok(Request::Begin { slot: 3, size: 16, crc: 0xDEADBEEF }));
        assert_eq!(parse(0x90A1, &[16, 0, 0, 0, 0, 0, 0, 0, 0]), Err(Status::BadRequest));
        assert!(matches!(parse(0x90A1, &[15, 0, 0, 0, 0, 0, 0, 0, 0]), Ok(Request::Begin { slot: 15, .. })));
        assert_eq!(parse(0x90A2, &[0; 4 + 129]), Err(Status::BadRequest));
        assert_eq!(parse(0x90A4, &[0xFF]), Ok(Request::Delete(0xFF)));
    }
}
