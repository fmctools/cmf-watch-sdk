//! IMA ADPCM (the IMA/DVI standard, 4 bits a sample): 16 kHz mono in 8 KB/s, half of what the phone link carries.
//! Each block carries its own start state, so a lost message costs only its own samples. The host encoder is
//! `wp2apps.adpcm` (same test vectors).

const STEPS: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97,
    107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796,
    876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871,
    5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623,
    27086, 29794, 32767,
];
const INDEX: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State {
    pub predictor: i16,
    pub index: u8,
}

impl State {
    /// One 4-bit code → the next sample.
    pub fn decode(&mut self, code: u8) -> i16 {
        let step = STEPS[self.index as usize];
        let mut diff = step >> 3;
        if code & 4 != 0 {
            diff += step;
        }
        if code & 2 != 0 {
            diff += step >> 1;
        }
        if code & 1 != 0 {
            diff += step >> 2;
        }
        let p = i32::from(self.predictor) + if code & 8 != 0 { -diff } else { diff };
        self.predictor = p.clamp(-32768, 32767) as i16;
        self.index = (i32::from(self.index) + INDEX[(code & 7) as usize]).clamp(0, 88) as u8;
        self.predictor
    }

    /// The 4-bit code for `sample` (the standard IMA encoder); the state moves as `decode` of that code does, so
    /// encoder and decoder stay in step.
    pub fn encode(&mut self, sample: i16) -> u8 {
        let step = STEPS[self.index as usize];
        let mut diff = i32::from(sample) - i32::from(self.predictor);
        let mut code = 0;
        if diff < 0 {
            code = 8;
            diff = -diff;
        }
        let mut s = step;
        for bit in [4, 2, 1] {
            if diff >= s {
                code |= bit;
                diff -= s;
            }
            s >>= 1;
        }
        self.decode(code);
        code
    }
}

/// Encodes `samples` (an even count) as one block into `out`: the header (the state before), then the codes;
/// returns the bytes written, 0 if `out` is too short.
pub fn encode_block(state: &mut State, samples: &[i16], out: &mut [u8]) -> usize {
    let n = BLOCK_HEADER + samples.len() / 2;
    if out.len() < n {
        return 0;
    }
    out[..2].copy_from_slice(&state.predictor.to_le_bytes());
    out[2] = state.index;
    for (i, pair) in samples.chunks_exact(2).enumerate() {
        out[BLOCK_HEADER + i] = state.encode(pair[0]) | state.encode(pair[1]) << 4;
    }
    n
}

/// Block header: predictor i16 LE, index u8, then the codes, low nibble first.
pub const BLOCK_HEADER: usize = 3;

/// Decodes one block into `out` as little-endian i16 bytes; returns the bytes written (4 per code byte), or 0 for a
/// block with a bad header or an `out` too short.
pub fn decode_block(block: &[u8], out: &mut [u8]) -> usize {
    if block.len() < BLOCK_HEADER || block[2] > 88 {
        return 0;
    }
    let codes = &block[BLOCK_HEADER..];
    if out.len() < codes.len() * 4 {
        return 0;
    }
    let mut s = State { predictor: i16::from_le_bytes([block[0], block[1]]), index: block[2] };
    for (i, &b) in codes.iter().enumerate() {
        out[4 * i..4 * i + 2].copy_from_slice(&s.decode(b & 0xF).to_le_bytes());
        out[4 * i + 2..4 * i + 4].copy_from_slice(&s.decode(b >> 4).to_le_bytes());
    }
    codes.len() * 4
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    #[test]
    fn decodes_the_shared_vector() {
        // tests/fixtures/adpcm-vector.json, from wp2apps.adpcm: a 440 Hz sine at 16 kHz in two blocks of 32 samples
        let blocks: [&[u8]; 2] = [&[0x00, 0x00, 0x00, 0x70, 0x77, 0x77, 0x77, 0x77, 0x80, 0x98, 0x98, 0x9a, 0xba, 0xba, 0xba, 0xaa, 0x89, 0x10, 0x42],
                                  &[0x43, 0xda, 0x35, 0x44, 0x34, 0x34, 0x34, 0x32, 0x22, 0x01, 0x90, 0xca, 0xcc, 0xbc, 0xbc, 0xcb, 0xab, 0xab, 0x99]];
        let want: [i16; 64] = [0, 11, 41, 104, 240, 533, 1164, 2521, 5431, 11667, 12558, 11748, 11012, 9004, 8396, 6736, 4220, 2848, 770, -1876, -3593, -5778, -7198, -9005, -10178, -11244, -11826, -12002, -11842, -11406, -10744, -9661, -8350, -6763, -4843, -3036, -924, 1064, 3388, 5573, 6993, 8800, 9973, 11039, 11621, 11797, 11957, 11521, 10859, 9776, 8465, 6878, 4958, 3151, 1039, -949, -2756, -4868, -6856, -8147, -9789, -10855, -11437, -11965];
        let mut got = std::vec::Vec::new();
        for block in blocks {
            let mut out = [0u8; 64];
            assert_eq!(decode_block(block, &mut out), 64);
            got.extend(out.chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]])));
        }
        assert_eq!(got, want);
    }

    #[test]
    fn encodes_what_decodes_back() {
        let pcm: std::vec::Vec<i16> = (0..256).map(|i| ((i as f32 * 0.2).sin() * 9000.0) as i16).collect();
        let mut st = State::default();
        let mut block = [0u8; 3 + 128];
        assert_eq!(encode_block(&mut st, &pcm, &mut block), 131);
        let mut out = [0u8; 512];
        assert_eq!(decode_block(&block, &mut out), 512);
        let err = pcm.iter().zip(out.chunks(2)).skip(16).map(|(a, b)| (i32::from(*a) - i32::from(i16::from_le_bytes([b[0], b[1]]))).abs()).max();
        assert!(err.unwrap() < 1500, "{err:?}");
    }

    #[test]
    fn clamps() {
        let mut s = State { predictor: 32000, index: 88 };
        assert_eq!(s.decode(7), 32767);
        assert_eq!(s.index, 88);
        let mut s = State { predictor: -32000, index: 0 };
        s.decode(0xF);
        assert_eq!(s.index, 8);
        assert!(s.predictor < -32000);
    }

    #[test]
    fn rejects_bad_blocks() {
        let mut out = [0u8; 8];
        assert_eq!(decode_block(&[0, 0], &mut out), 0);
        assert_eq!(decode_block(&[0, 0, 89, 0], &mut out), 0, "index past the table");
        assert_eq!(decode_block(&[0, 0, 0, 1, 2, 3], &mut out), 0, "out too short");
    }
}
