//! Host side of the stub's decoder for the emulator e2e (tests/test_wp3faces_e2e.py): `etc2 W H` reads the image's
//! blocks from stdin and writes its RGB565 + alpha pixels to stdout.
use std::io::{Read, Write};
use wp2_core::etc2;

fn main() {
    let mut size = std::env::args().skip(1).map(|arg| arg.parse::<usize>().expect("W H"));
    let (w, h) = (size.next().expect("W"), size.next().expect("H"));
    let mut blocks = Vec::new();
    std::io::stdin().read_to_end(&mut blocks).expect("stdin");
    let mut pixels = vec![0u8; w * h * etc2::PIXEL];
    assert!(etc2::decode(&blocks, w, h, &mut pixels), "sizes");
    std::io::stdout().write_all(&pixels).expect("stdout");
}
