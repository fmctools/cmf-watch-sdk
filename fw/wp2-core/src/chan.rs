// ABI 1.1 host channel queues and the app data file format (docs/abi-1.1.md).
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU8, Ordering};

use crate::pkg::crc32;

pub const MSG_MAX: usize = 128;
pub const DEPTH: u8 = 4;
pub const STORE_MAX: usize = 4096;
pub const STORE_HEADER: usize = 12;
const STORE_MAGIC: &[u8; 4] = b"WP2D";

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Msg {
    pub len: u8,
    pub slot: u8,
    pub data: [u8; MSG_MAX],
}

// Single producer, single consumer. head/tail run free modulo 256 (N divides 256); zeroed memory is an empty ring.
#[repr(C)]
pub struct RingN<const N: usize> {
    head: AtomicU8,
    tail: AtomicU8,
    msgs: UnsafeCell<[Msg; N]>,
}

/// The stub's inbox and outbox (StubRam layout: unchanged).
pub type Ring = RingN<{ DEPTH as usize }>;

unsafe impl<const N: usize> Sync for RingN<N> {}

impl<const N: usize> RingN<N> {
    pub const fn new() -> Self {
        const EMPTY: Msg = Msg { len: 0, slot: 0, data: [0; MSG_MAX] };
        assert!(N > 0 && N < 256 && 256 % N == 0);
        RingN { head: AtomicU8::new(0), tail: AtomicU8::new(0), msgs: UnsafeCell::new([EMPTY; N]) }
    }

    fn at(&self, i: u8) -> *mut Msg {
        unsafe { (self.msgs.get() as *mut Msg).add(usize::from(i) % N) }
    }

    pub fn len(&self) -> u8 {
        self.tail.load(Ordering::Acquire).wrapping_sub(self.head.load(Ordering::Acquire))
    }

    // producer
    pub fn push(&self, slot: u8, data: &[u8]) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        if data.is_empty() || data.len() > MSG_MAX || usize::from(tail.wrapping_sub(self.head.load(Ordering::Acquire))) >= N {
            return false;
        }
        let m = unsafe { &mut *self.at(tail) };
        m.len = data.len() as u8;
        m.slot = slot;
        m.data[..data.len()].copy_from_slice(data);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    // consumer: the message `n` places after the head, if there is one
    pub fn peek(&self, n: u8) -> Option<&Msg> {
        (n < self.len()).then(|| unsafe { &*self.at(self.head.load(Ordering::Relaxed).wrapping_add(n)) })
    }

    pub fn head(&self) -> u8 {
        self.head.load(Ordering::Acquire)
    }

    pub fn tail(&self) -> u8 {
        self.tail.load(Ordering::Acquire)
    }

    // consumer
    pub fn pop(&self) {
        if self.len() > 0 {
            self.head.store(self.head.load(Ordering::Relaxed).wrapping_add(1), Ordering::Release);
        }
    }

    // consumer
    pub fn clear(&self) {
        self.head.store(self.tail.load(Ordering::Acquire), Ordering::Release);
    }
}

// A wp2apps host listens: the stock stream is connected and the session key is still the one of the last
// authenticated wp2 frame (a new bind changes it). Callers: the stub (wewear_read) and the app thread.
#[cfg(target_os = "none")]
pub fn listening(host_key: u32) -> bool {
    use crate::stock::WEWEAR_CTX;
    let ctx = unsafe { (WEWEAR_CTX as *const u32).read_volatile() };
    host_key != 0 && ctx != 0 && unsafe { ((ctx + 4) as *const u8).read_volatile() } & 1 != 0 && session_word() == host_key
}

#[cfg(target_os = "none")]
pub fn session_word() -> u32 {
    unsafe { (crate::stock::SESSION_KEY as *const u32).read_unaligned() }
}

pub fn store_header(data: &[u8]) -> [u8; STORE_HEADER] {
    let mut h = [0u8; STORE_HEADER];
    h[..4].copy_from_slice(STORE_MAGIC);
    h[4..8].copy_from_slice(&(data.len() as u32).to_le_bytes());
    h[8..].copy_from_slice(&crc32(0, data).to_le_bytes());
    h
}

// (length, crc) of a stored blob, from its header
pub fn parse_store(h: &[u8; STORE_HEADER]) -> Option<(usize, u32)> {
    let len = u32::from_le_bytes(h[4..8].try_into().unwrap()) as usize;
    (&h[..4] == STORE_MAGIC && len <= STORE_MAX).then(|| (len, u32::from_le_bytes(h[8..].try_into().unwrap())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_bounds_and_order() {
        let r = Ring::new();
        assert!(r.peek(0).is_none());
        assert!(!r.push(0, &[]));
        assert!(!r.push(0, &[0; MSG_MAX + 1]));
        for i in 0..DEPTH {
            assert!(r.push(i, &[i; MSG_MAX]));
        }
        assert!(!r.push(9, &[1]));
        assert_eq!(r.peek(3).map(|m| m.slot), Some(3));
        for i in 0..DEPTH {
            let m = r.peek(0).unwrap();
            assert_eq!((m.slot, m.len, m.data[0]), (i, MSG_MAX as u8, i));
            r.pop();
        }
        for i in 0..300u32 {
            assert!(r.push(1, &i.to_le_bytes()));
            assert_eq!(r.peek(0).unwrap().data[..4], i.to_le_bytes());
            r.pop();
        }
        r.push(1, &[1]);
        r.clear();
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn store_format() {
        let h = store_header(b"abc");
        assert_eq!(parse_store(&h), Some((3, crc32(0, b"abc"))));
        let mut bad = h;
        bad[0] = b'X';
        assert_eq!(parse_store(&bad), None);
        let mut big = h;
        big[4..8].copy_from_slice(&(STORE_MAX as u32 + 1).to_le_bytes());
        assert_eq!(parse_store(&big), None);
    }
}
