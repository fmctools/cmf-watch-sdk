//! Decode of the temporary phone-to-watch image stream (docs/bitmap-assets.md, "Streaming images").
#![no_std]

/// First byte of every message of the protocol.
pub const MAGIC: u8 = 0xB5;
pub const START: u8 = 1;
pub const DATA: u8 = 2;
pub const DONE: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg<'a> {
    Start { w: u16, h: u16 },
    Data { offset: u32, payload: &'a [u8] },
    Done,
}

/// One stream message; None for anything else (or a frame the image cannot use: odd offset or payload).
pub fn parse(m: &[u8]) -> Option<Msg<'_>> {
    match m {
        [MAGIC, START, w0, w1, h0, h1] => {
            let (w, h) = (u16::from_le_bytes([*w0, *w1]), u16::from_le_bytes([*h0, *h1]));
            ((1..=466).contains(&w) && (1..=466).contains(&h)).then(|| Msg::Start { w, h })
        }
        [MAGIC, DATA, o0, o1, o2, o3, payload @ ..]
            if !payload.is_empty() && payload.len() % 2 == 0
                && u32::from_le_bytes([*o0, *o1, *o2, *o3]) % 2 == 0 => {
            Some(Msg::Data { offset: u32::from_le_bytes([*o0, *o1, *o2, *o3]), payload })
        }
        [MAGIC, DONE] => Some(Msg::Done),
        _ => None,
    }
}

/// Row spans of a DATA payload: (x, y, count, source pixel index in the payload) into a w * h image, in order.
/// The range clips at the image edge; an offset past the end gives nothing.
pub fn segments(offset: u32, payload: usize, w: usize, h: usize) -> Seg {
    let start = offset as usize / 2;
    let total = w * h;
    let left = if w == 0 || start >= total { 0 } else { (payload / 2).min(total - start) };
    Seg { at: start, left, src: 0, w, h }
}

pub struct Seg {
    at: usize,
    left: usize,
    src: usize,
    w: usize,
    h: usize,
}

impl Iterator for Seg {
    type Item = (usize, usize, usize, usize);

    fn next(&mut self) -> Option<Self::Item> {
        if self.left == 0 || self.w == 0 {
            return None;
        }
        let (x, y) = (self.at % self.w, self.at / self.w);
        if y >= self.h {
            return None;
        }
        let count = self.left.min(self.w - x);
        self.at += count;
        self.left -= count;
        let src = self.src;
        self.src += count;
        Some((x, y, count, src))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    #[test]
    fn parses_the_three_messages() {
        assert_eq!(parse(&[0xB5, 1, 64, 0, 32, 0]), Some(Msg::Start { w: 64, h: 32 }));
        assert_eq!(parse(&[0xB5, 1, 0, 2, 1, 0]), None, "467 wide");
        assert_eq!(parse(&[0xB5, 3]), Some(Msg::Done));
        assert_eq!(parse(&[0xB5, 3, 0]), None);
        assert_eq!(parse(&[0x55, 3]), None, "bad magic");
        match parse(&[0xB5, 2, 2, 0, 0, 0, 7, 0, 9, 0]) {
            Some(Msg::Data { offset: 2, payload }) => assert_eq!(payload, &[7, 0, 9, 0]),
            other => panic!("{other:?}"),
        }
        assert_eq!(parse(&[0xB5, 2, 1, 0, 0, 0, 7, 0]), None, "odd offset misaligns pixels");
        assert_eq!(parse(&[0xB5, 2, 0, 0, 0, 0, 7]), None, "odd payload");
        assert_eq!(parse(&[0xB5, 2, 0, 0, 0, 0]), None, "empty payload");
    }

    fn spans(offset: u32, payload: usize, w: usize, h: usize) -> Vec<(usize, usize, usize, usize)> {
        segments(offset, payload, w, h).collect()
    }

    #[test]
    fn spans_stay_in_order_and_clip() {
        // 4 x 3 image
        assert_eq!(spans(0, 8, 4, 3), [(0, 0, 4, 0)], "one whole row");
        assert_eq!(spans(2, 6, 4, 3), [(1, 0, 3, 0)], "starts mid-row");
        assert_eq!(spans(6, 12, 4, 3), [(3, 0, 1, 0), (0, 1, 4, 1), (0, 2, 1, 5)], "crosses two rows");
        assert_eq!(spans(20, 16, 4, 3), [(2, 2, 2, 0)], "clips at the end");
        assert_eq!(spans(48, 8, 4, 3), [], "offset past the end");
        assert_eq!(spans(0, 0, 4, 3), [], "no payload");
    }
}
