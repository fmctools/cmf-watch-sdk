// ETC2 RGBA8 (EAC alpha), the image format 28 of Watch 3 Pro faces, to the WP2's RGB565 + alpha (cf 5, 3 bytes per
// pixel: the colour little-endian, then alpha). Written from the Khronos Data Format Specification, "ETC2 Compressed
// Texture Image Formats"; the test blocks' expected pixels come from an independent decoder.

pub const BLOCK: usize = 16;
pub const PIXEL: usize = 3;
/// larger than any watch image; keeps the size arithmetic from overflowing
const MAX_SIDE: usize = 2048;

/// Bytes of a w x h image's blocks.
pub fn packed_len(w: usize, h: usize) -> usize {
    (w + 3) / 4 * ((h + 3) / 4) * BLOCK
}

// per table, the small and the large distance
const COLOR: [(i32, i32); 8] = [(2, 8), (5, 17), (9, 29), (13, 42), (18, 60), (24, 80), (33, 106), (47, 183)];
const DISTANCE: [i32; 8] = [3, 6, 11, 16, 23, 32, 41, 64];
const ALPHA: [[i8; 8]; 16] = [
    [-3, -6, -9, -15, 2, 5, 8, 14],
    [-3, -7, -10, -13, 2, 6, 9, 12],
    [-2, -5, -8, -13, 1, 4, 7, 12],
    [-2, -4, -6, -13, 1, 3, 5, 12],
    [-3, -6, -8, -12, 2, 5, 7, 11],
    [-3, -7, -9, -11, 2, 6, 8, 10],
    [-4, -7, -8, -11, 3, 6, 7, 10],
    [-3, -5, -8, -11, 2, 4, 7, 10],
    [-2, -6, -8, -10, 1, 5, 7, 9],
    [-2, -5, -8, -10, 1, 4, 7, 9],
    [-2, -4, -8, -10, 1, 3, 7, 9],
    [-2, -5, -7, -10, 1, 4, 6, 9],
    [-3, -4, -7, -10, 2, 3, 6, 9],
    [-1, -2, -3, -10, 0, 1, 2, 9],
    [-4, -6, -8, -9, 3, 5, 7, 8],
    [-3, -5, -7, -9, 2, 4, 6, 8],
];

type Rgb = [i32; 3];

fn byte(v: i32) -> i32 {
    v.max(0).min(255)
}

fn add(c: Rgb, d: i32) -> Rgb {
    [byte(c[0] + d), byte(c[1] + d), byte(c[2] + d)]
}

/// Bits from..from + n of the block's high word (the word's bit 0 is the format's bit 32).
fn bits(high: u32, from: u32, n: u32) -> i32 {
    ((high >> (from - 32)) & ((1 << n) - 1)) as i32
}

fn from4(v: i32) -> i32 {
    v << 4 | v
}

fn from5(v: i32) -> i32 {
    v << 3 | v >> 2
}

fn from6(v: i32) -> i32 {
    v << 2 | v >> 4
}

fn from7(v: i32) -> i32 {
    v << 1 | v >> 6
}

enum Colors {
    /// individual and differential: two halves, a distance table each
    Halves { base: [Rgb; 2], table: [usize; 2], flip: bool },
    /// T and H
    Paints([Rgb; 4]),
    Planar { o: Rgb, h: Rgb, v: Rgb },
}

fn colors(high: u32, low: u32) -> Colors {
    let b = |from, n| bits(high, from, n);
    let table = [b(37, 3) as usize, b(34, 3) as usize];
    let flip = b(32, 1) != 0;
    if b(33, 1) == 0 {
        let base = [[from4(b(60, 4)), from4(b(52, 4)), from4(b(44, 4))], [from4(b(56, 4)), from4(b(48, 4)), from4(b(40, 4))]];
        return Colors::Halves { base, table, flip };
    }
    let signed = |v: i32| if v >= 4 { v - 8 } else { v };
    let first = [b(59, 5), b(51, 5), b(43, 5)];
    let second = [first[0] + signed(b(56, 3)), first[1] + signed(b(48, 3)), first[2] + signed(b(40, 3))];
    let overflows = |v: i32| v < 0 || v > 31;
    if overflows(second[0]) {
        // T
        let one = [from4(b(59, 2) << 2 | b(56, 2)), from4(b(52, 4)), from4(b(48, 4))];
        let two = [from4(b(44, 4)), from4(b(40, 4)), from4(b(36, 4))];
        let d = DISTANCE[(b(34, 2) << 1 | b(32, 1)) as usize];
        Colors::Paints([one, add(two, d), two, add(two, -d)])
    } else if overflows(second[1]) {
        // H
        let one = [from4(b(59, 4)), from4(b(56, 3) << 1 | b(52, 1)), from4(b(51, 1) << 3 | b(47, 3))];
        let two = [from4(b(43, 4)), from4(b(39, 4)), from4(b(35, 4))];
        let order = i32::from(one >= two);
        let d = DISTANCE[(b(34, 1) << 2 | b(32, 1) << 1 | order) as usize];
        Colors::Paints([add(one, d), add(one, -d), add(two, d), add(two, -d)])
    } else if overflows(second[2]) {
        // planar: the low word continues the fields
        let l = |from: u32, n: u32| ((low >> from) & ((1 << n) - 1)) as i32;
        let o = [from6(b(57, 6)), from7(b(56, 1) << 6 | b(49, 6)), from6(b(48, 1) << 5 | b(43, 2) << 3 | b(39, 3))];
        let h = [from6(b(34, 5) << 1 | b(32, 1)), from7(l(25, 7)), from6(l(19, 6))];
        let v = [from6(l(13, 6)), from7(l(6, 7)), from6(l(0, 6))];
        Colors::Planar { o, h, v }
    } else {
        let five = |c: Rgb| [from5(c[0]), from5(c[1]), from5(c[2])];
        Colors::Halves { base: [five(first), five(second)], table, flip }
    }
}

impl Colors {
    fn at(&self, low: u32, x: usize, y: usize) -> Rgb {
        let i = x * 4 + y;
        let index = ((low >> (16 + i) & 1) << 1 | (low >> i & 1)) as usize;
        match *self {
            Colors::Halves { base, table, flip } => {
                let half = usize::from(if flip { y >= 2 } else { x >= 2 });
                let (small, large) = COLOR[table[half]];
                add(base[half], [small, large, -small, -large][index])
            }
            Colors::Paints(paint) => paint[index],
            Colors::Planar { o, h, v } => {
                let (x, y) = (x as i32, y as i32);
                let mut out = [0; 3];
                for c in 0..3 {
                    out[c] = byte((x * (h[c] - o[c]) + y * (v[c] - o[c]) + 4 * o[c] + 2) >> 2);
                }
                out
            }
        }
    }
}

/// `src`: the image's blocks, row by row; `dst`: w * h pixels. False when either is the wrong size.
pub fn decode(src: &[u8], w: usize, h: usize, dst: &mut [u8]) -> bool {
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE || src.len() != packed_len(w, h) || dst.len() != w * h * PIXEL {
        return false;
    }
    let across = (w + 3) / 4;
    for (n, block) in src.chunks_exact(BLOCK).enumerate() {
        let (left, top) = (n % across * 4, n / across * 4);
        let (base, multiplier) = (i32::from(block[0]), i32::from(block[1] >> 4));
        let modifiers = &ALPHA[usize::from(block[1] & 15)];
        // 16 indices of 3 bits: the first 5 and a third end in the high word
        let alpha_high = u32::from_be_bytes([0, 0, block[2], block[3]]);
        let alpha_low = u32::from_be_bytes([block[4], block[5], block[6], block[7]]);
        let high = u32::from_be_bytes([block[8], block[9], block[10], block[11]]);
        let low = u32::from_be_bytes([block[12], block[13], block[14], block[15]]);
        let colors = colors(high, low);
        for x in 0..4.min(w - left) {
            for y in 0..4.min(h - top) {
                let shift = 45 - 3 * (x * 4 + y) as u32;
                let index = if shift >= 32 {
                    alpha_high >> (shift - 32)
                } else if shift >= 30 {
                    alpha_high << (32 - shift) | alpha_low >> shift
                } else {
                    alpha_low >> shift
                } & 7;
                let a = byte(base + i32::from(modifiers[index as usize]) * multiplier);
                let [r, g, b] = colors.at(low, x, y);
                let c = ((r >> 3) << 11 | (g >> 2) << 5 | b >> 3) as u16;
                let at = ((top + y) * w + left + x) * PIXEL;
                dst[at..at + PIXEL].copy_from_slice(&[c as u8, (c >> 8) as u8, a as u8]);
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    // one synthetic block per colour mode
    const BLOCKS: [(&str, &str); 5] = [
        // individual
        ("39c46014b08bec16e698fda44bb6942d", "5fff001cd4001b6451dd748d1cd415559a001b64007c6c15559a159ffd8ddd74bd1b64005fff001cd4151b648ddd748d"),
        // differential
        ("4975e59b9ed78dc788f4a6ea8f213d9b", "00028fffff73ce6d8196af81ffff184e5e8171867396af8ffabf00fabf00ce6d8ff39634ffff18ffff81ce6d3471868f"),
        // T
        ("61cd4612d2f1192af307a123dc389a32", "0eb83d43a0490eb8cd43a061a5b04943a000a5b0610eb8610eb8610eb83d84a83d84a86d84a8490eb83d43a04943a03d"),
        // planar
        ("fc186a629b9717dbb2d4f22ee3651ded", "aa62f2cd63f6f05cfdf35df2cd73f4f074f4136eff366fff1085fd3386f2567ffff97ff25396ff7697f2f98ff6fc8ff2"),
        // H
        ("841444eaa92601f2e40e7c66f2b589ac", "fcfc7cfcfc8996b37efcfc81fce47e96eb7cfcfc7efcfc8f96eb7efce489fce486fcfc8b96b38b96eb7e96b38196eb7c"),
    ];

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn blocks_match_an_independent_decoder() {
        for (src, want) in BLOCKS {
            let mut dst = [0u8; 48];
            assert!(decode(&hex(src), 4, 4, &mut dst));
            assert_eq!(dst.to_vec(), hex(want), "{src}");
        }
    }

    #[test]
    fn edge_blocks_are_cropped() {
        let (a, b) = (hex(BLOCKS[0].0), hex(BLOCKS[1].0));
        let (pa, pb) = (hex(BLOCKS[0].1), hex(BLOCKS[1].1));
        let mut dst = [0xAAu8; 5 * 2 * 3];
        assert!(decode(&[a, b].concat(), 5, 2, &mut dst));
        assert_eq!(&dst[..12], &pa[..12]);
        assert_eq!(&dst[12..15], &pb[..3]);
        assert_eq!(&dst[15..27], &pa[12..24]);
        assert_eq!(&dst[27..30], &pb[12..15]);
    }

    #[test]
    fn sizes_are_checked() {
        assert_eq!(packed_len(26, 36), 1008);
        assert_eq!(packed_len(456, 456), 207_936);
        assert!(!decode(&[0; 15], 4, 4, &mut [0; 48]));
        assert!(!decode(&[0; 16], 4, 4, &mut [0; 47]));
        assert!(!decode(&[0; 16], 0, 4, &mut [0; 48]));
        assert!(!decode(&[0; 16], usize::MAX, 4, &mut [0; 48]));
    }
}
