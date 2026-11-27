//! App widget records (`<d>.wgt`, docs/widget-push.md) and the package's widget icon trailer (docs/app-icons.md,
//! "Widget icons"). The stub draws a record only when `card` accepts it; anything else shows the package name.

use crate::layout::{WIDGET_REC, WIDGET_TITLE};

/// v2 records: `FF ‖ template ‖ fields`, at most this long; v1 is exactly `WIDGET_REC` bytes not starting with FF
pub const REC_MAX: usize = 128;
pub const V2: u8 = 0xFF;
pub const T_TEXT: u8 = 0;
pub const T_COLUMNS: u8 = 1;
pub const COLUMNS_MAX: u8 = 4;
/// one column: caption[10] ‖ icon:u8 ‖ value[10]
pub const CAPTION: usize = 10;
pub const VALUE: usize = 10;
pub const COLUMN: usize = CAPTION + 1 + VALUE;
pub const COLUMNS_AT: usize = 3;
pub const NO_ICON: u8 = 0xFF;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Card {
    /// title at this offset (24 B field), value 24 bytes later (40 B field)
    Text(usize),
    /// n columns from `COLUMNS_AT`, `COLUMN` bytes each
    Columns(u8),
}

/// A text field: UTF-8, then at least one NUL and only NULs to its end.
fn field(f: &[u8]) -> bool {
    match f.iter().position(|&b| b == 0) {
        Some(n) => f[n..].iter().all(|&b| b == 0) && core::str::from_utf8(&f[..n]).is_ok(),
        None => false,
    }
}

/// What the stub draws for `rec`; None = the package name. v1 is taken as before (the stub terminates its fields);
/// v2 only when every field is well-formed. An icon index is well-formed below `ICONS_MAX`, whatever the package
/// carries: one it does not have costs that icon, not the card (`icon_at`).
pub fn card(rec: &[u8]) -> Option<Card> {
    match rec {
        [V2, T_TEXT, f @ ..] => (f.len() == WIDGET_REC && field(&f[..WIDGET_TITLE]) && field(&f[WIDGET_TITLE..]))
            .then_some(Card::Text(2)),
        [V2, T_COLUMNS, n, cols @ ..] => ((1..=COLUMNS_MAX).contains(n)
            && cols.len() == COLUMN * usize::from(*n)
            && cols.chunks_exact(COLUMN).all(|c| {
                field(&c[..CAPTION]) && field(&c[CAPTION + 1..]) && (c[CAPTION] == NO_ICON || c[CAPTION] < ICONS_MAX)
            }))
        .then_some(Card::Columns(*n)),
        [V2, ..] => None,
        _ => (rec.len() == WIDGET_REC).then_some(Card::Text(0)),
    }
}

/// The first stub that draws `template`; None = unknown. The loader answers -22 below it so the app falls back.
pub fn first_stub(template: u8) -> Option<u32> {
    match template {
        T_TEXT | T_COLUMNS => Some(10), // not 9: that is the watch face guard, without templates
        _ => None,
    }
}

/// The loader takes a WIDGET record from the app (op 0x14): v1 on any stub, v2 only when well-formed (`card`) and
/// drawn by the running stub (`stub` = StubRam::version()).
pub fn accepts(rec: &[u8], stub: u32) -> bool {
    match rec {
        [V2, t, ..] => first_stub(*t).is_some_and(|first| stub >= first) && card(rec).is_some(),
        _ => rec.len() == WIDGET_REC,
    }
}

// Widget icon trailer after the relocation table, outside body_crc (docs/app-icons.md):
// "WP2W" ‖ version 1 ‖ count 1..=16 ‖ size 40 ‖ 0 ‖ crc32 of the pixels (u32 LE) ‖ count × size² × 3 B (CF 5)
pub const TRAILER_MAGIC: &[u8; 4] = b"WP2W";
pub const TRAILER_HEAD: usize = 12;
pub const ICONS_MAX: u8 = 16;
/// stock widget 97's condition images are 40x40 CF 5 (emulator, docs/recon.md "Widget cards")
pub const ICON_SIDE: u8 = 40;
pub const ICON_BYTES: usize = ICON_SIDE as usize * ICON_SIDE as usize * 3;

/// (count, crc) of a well-formed trailer head
pub fn trailer_head(h: &[u8; TRAILER_HEAD]) -> Option<(u8, u32)> {
    let ok = &h[..4] == TRAILER_MAGIC && h[4] == 1 && (1..=ICONS_MAX).contains(&h[5]) && h[6] == ICON_SIDE && h[7] == 0;
    ok.then(|| (h[5], u32::from_le_bytes(h[8..12].try_into().unwrap())))
}

pub fn trailer_len(count: u8) -> usize {
    TRAILER_HEAD + usize::from(count) * ICON_BYTES
}

/// Offset of icon `i` from the start of a trailer of `count` icons (0: no trailer or a bad one). None = the column
/// has no icon: `NO_ICON`, or one the package lacks. The only way to an offset, so nothing is read past the trailer.
pub fn icon_at(i: u8, count: u8) -> Option<usize> {
    (i < count).then(|| TRAILER_HEAD + usize::from(i) * ICON_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pkg::crc32;

    fn columns(cols: &[(&[u8], u8, &[u8])]) -> Vec<u8> {
        let mut r = vec![V2, T_COLUMNS, cols.len() as u8];
        for (c, i, v) in cols {
            let mut col = [0u8; COLUMN];
            col[..c.len()].copy_from_slice(c);
            col[CAPTION] = *i;
            col[CAPTION + 1..CAPTION + 1 + v.len()].copy_from_slice(v);
            r.extend_from_slice(&col);
        }
        r
    }

    #[test]
    fn v1_unchanged() {
        assert_eq!(card(&[b'a'; 64]), Some(Card::Text(0)), "v1: the stub terminates its fields");
        assert_eq!(card(&[0; 64]), Some(Card::Text(0)));
        assert_eq!(card(&[b'a'; 63]), None);
        assert_eq!(card(&[b'a'; 65]), None);
    }

    #[test]
    fn columns_valid() {
        let r = columns(&[(b"Now", 0, "-3°".as_bytes()), (b"23:00", 1, b"4"), (b"00:00", NO_ICON, b""), (b"", 2, b"5")]);
        assert_eq!(r.len(), 87);
        assert_eq!(card(&r), Some(Card::Columns(4)));
        assert_eq!(card(&r[..3 + COLUMN * 1]), None, "n says 4, one column follows");
        let one = columns(&[(b"Now", NO_ICON, b"1")]);
        assert_eq!(card(&one), Some(Card::Columns(1)), "no icon needs no trailer");
    }

    #[test]
    fn every_malformed_record_falls_back() {
        let good = columns(&[(b"Now", 0, b"1"), (b"01", 1, b"2")]);
        assert!(card(&good).is_some());
        let mut bad: Vec<Vec<u8>> = vec![
            vec![],
            vec![V2],
            vec![V2, T_COLUMNS],
            vec![V2, 2, 1],                      // unknown template
            columns(&[]),                        // n = 0
            columns(&[(&b"a"[..], NO_ICON, &b"b"[..]); 5]), // n = 5
            [good.clone(), vec![0]].concat(),    // one byte long
            good[..good.len() - 1].to_vec(),     // one byte short
        ];
        let caption: [&[u8]; 3] = [b"abcdefghij", b"No\0w\0\0\0\0\0\0", b"\xC3\0\0\0\0\0\0\0\0\0"]; // no NUL, text after it, cut UTF-8
        for f in caption {
            let mut r = good.clone();
            r[3..3 + CAPTION].copy_from_slice(f);
            bad.push(r.clone());
            let mut r = good.clone();
            r[3 + COLUMN + CAPTION + 1..][..VALUE].copy_from_slice(f); // the same in the second value
            bad.push(r);
        }
        let mut icon = good.clone();
        icon[3 + CAPTION] = ICONS_MAX; // no package has it
        bad.push(icon);
        let mut text = vec![V2, T_TEXT];
        text.extend_from_slice(&[b'a'; 64]);
        bad.push(text.clone()); // no NUL in the title
        text[2 + 23] = 0;
        bad.push(text.clone()); // no NUL in the value
        text[2 + 63] = 0;
        assert_eq!(card(&text), Some(Card::Text(2)));
        bad.push(text[..65].to_vec());
        for r in &bad {
            assert_eq!(card(r), None, "{r:02x?}");
        }
    }

    #[test]
    fn sdk_records_pass() {
        let c = |caption, icon, value| cmf_wp2_app::Column { caption, icon, value };
        let (r, n) = cmf_wp2_app::columns_record(&[c("Now", Some(0), "-3°"), c("23:00", Some(2), "4°")]).unwrap();
        assert_eq!(card(&r[..n]), Some(Card::Columns(2)));
        assert!(accepts(&r[..n], 10));
        let v1 = cmf_wp2_app::widget_record("Hello", "x");
        assert_eq!(card(&v1[4..]), Some(Card::Text(0)));
    }

    #[test]
    fn missing_icon_keeps_the_card() {
        // stub 10, owner 2026-11-22: a package stored stripped on stub 8 or 9 still shows its columns after the OTA
        let r = columns(&[(b"Now", 0, b"1"), (b"01", 2, b"2"), (b"02", 15, b"3")]);
        assert_eq!(card(&r), Some(Card::Columns(3)), "the card whatever the package carries");
        for i in [0, 2, 15] {
            assert_eq!(icon_at(i, 0), None, "no trailer, or a damaged one: count 0");
        }
        assert!(icon_at(0, 1).is_some());
        assert_eq!((icon_at(2, 1), icon_at(15, 1)), (None, None), "count 1, below the indices 2 and 15");
        for count in 0..=ICONS_MAX {
            for i in 0..=255 {
                match icon_at(i, count) {
                    Some(at) => assert!(i < count && at + ICON_BYTES <= trailer_len(count), "inside the trailer"),
                    None => assert!(i >= count),
                }
            }
        }
        for index in [16, 17, 0x80, 254] {
            let r = columns(&[(b"Now", index, b"1")]);
            assert_eq!(card(&r), None, "index {index} is malformed whatever the package has");
        }
    }

    #[test]
    fn loader_gate() {
        let r = columns(&[(b"Now", 15, b"1")]);
        assert!(accepts(&r, 10));
        assert!(accepts(&r, 11), "and every later stub");
        assert!(!accepts(&r, 9), "stub 9 is the watch face guard: no templates");
        assert!(!accepts(&[&[V2, T_TEXT][..], &[0; 64]].concat(), 9), "template text neither");
        assert!(accepts(&[b'a'; 64], 9), "v1 on stub 10");
        assert!(!accepts(&r, 8), "stub 8 does not draw columns");
        assert_eq!(first_stub(T_COLUMNS), Some(10), "the stub that brings them");
        assert!(!accepts(&r, 0));
        assert!(!accepts(&columns(&[(b"Now", 16, b"1")]), 10), "no package has 17 icons");
        assert!(!accepts(&[V2, 7, 0], 99), "unknown template");
        assert!(accepts(&[b'a'; 64], 0), "v1 on any stub");
        assert!(!accepts(&[b'a'; 65], 10));
    }

    #[test]
    fn trailer() {
        let px = vec![7u8; 2 * ICON_BYTES];
        let mut h = [0u8; TRAILER_HEAD];
        h[..8].copy_from_slice(b"WP2W\x01\x02\x28\x00");
        h[8..].copy_from_slice(&crc32(0, &px).to_le_bytes());
        assert_eq!(trailer_head(&h), Some((2, crc32(0, &px))));
        assert_eq!(trailer_len(2), 12 + 9600);
        assert_eq!(icon_at(1, 2), Some(12 + 4800));
        for (i, v) in [(0, b'X'), (4, 2), (5, 0), (5, 17), (6, 48), (7, 1)] {
            let mut b = h;
            b[i] = v;
            assert_eq!(trailer_head(&b), None, "byte {i} = {v}");
        }
    }
}
