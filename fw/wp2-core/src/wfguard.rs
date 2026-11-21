// Boot watch face guard (docs/recon.md, "Watch face boot guard"). The stub arms a NAND flag before the stock loads
// the active face at boot and clears it once the face has run for CONFIRM_MS; a flag still set at the next boot means
// the face took the watch down, and `fall_back` makes a built-in face active in WfmPara before the stock loads it.

// stock built-ins (table 0x101F3D54); never deleted from NAND
pub const BUILTIN: [u32; 6] = [0x111, 0x112, 0x113, 0x114, 0x115, 0x118];
pub const FLAG: &[u8] = b"/NAND:/wp2wfgrd\0";
// the repro faults ~0.15 s after the load, a hung parser trips the watchdog at 11 s
pub const CONFIRM_MS: u32 = 20_000;

// WfmPara (0x80 B, 0x3801AE50): u8 count at 4, u8 active index at 5, u8 att at 6, u8 kind at 7, u32 active id at 8,
// entries of 16 B at 0xC (u32 id, u8 att, photo data); 7 fit before the checksum at 0x7C
pub const PARA_SIZE: usize = 0x80;
const ENTRIES: usize = 0xC;
const MAX: usize = (0x7C - ENTRIES) / 16;

fn id_at(para: &[u8; PARA_SIZE], i: usize) -> u32 {
    let o = ENTRIES + i * 16;
    u32::from_le_bytes([para[o], para[o + 1], para[o + 2], para[o + 3]])
}

/// Makes a built-in face active: the first listed built-in other than the active face, else the active entry
/// becomes 0x111 (the other entries are downloads or the photo face and stay listed). Returns the new index, None
/// for an empty list. The caller fills the entry's att byte and saves (checksum).
pub fn fall_back(para: &mut [u8; PARA_SIZE]) -> Option<usize> {
    let n = usize::from(para[4]).min(MAX);
    if n == 0 {
        return None;
    }
    let cur = usize::from(para[5]) % n;
    let bad = id_at(para, cur);
    let pick = (0..n).find(|&i| i != cur && BUILTIN.contains(&id_at(para, i)) && id_at(para, i) != bad);
    let i = pick.unwrap_or_else(|| {
        let o = ENTRIES + cur * 16;
        let id = if bad == BUILTIN[0] { BUILTIN[1] } else { BUILTIN[0] };
        para[o..o + 16].fill(0);
        para[o..o + 4].copy_from_slice(&id.to_le_bytes());
        cur
    });
    para[5] = i as u8;
    para[7] = 0;
    let id = id_at(para, i);
    para[8..12].copy_from_slice(&id.to_le_bytes());
    Some(i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(ids: &[u32], idx: u8) -> [u8; PARA_SIZE] {
        let mut p = [0u8; PARA_SIZE];
        p[4] = ids.len() as u8;
        p[5] = idx;
        for (i, id) in ids.iter().enumerate() {
            let o = ENTRIES + i * 16;
            p[o..o + 4].copy_from_slice(&id.to_le_bytes());
            p[o + 4] = 0xC;
        }
        p
    }

    fn ids(p: &[u8; PARA_SIZE]) -> Vec<u32> {
        (0..usize::from(p[4])).map(|i| id_at(p, i)).collect()
    }

    #[test]
    fn switches_to_another_builtin() {
        let mut p = para(&[0x4000_0001, 0x112, 0x111], 0);
        assert_eq!(fall_back(&mut p), Some(1));
        assert_eq!((p[5], id_at(&p, 1), ids(&p)), (1, 0x112, vec![0x4000_0001, 0x112, 0x111]));
        assert_eq!(&p[8..12], &0x112u32.to_le_bytes());
    }

    #[test]
    fn bad_builtin_skipped() {
        // the emulator repro: local273.wfc itself is the bad file
        let mut p = para(&[0x111, 0x112, 0x113], 0);
        assert_eq!(fall_back(&mut p), Some(1));
        let mut p = para(&[0x111, 0xFFFF_FFFF, 0x112], 2);
        assert_eq!(fall_back(&mut p), Some(0));
    }

    #[test]
    fn no_builtin_listed_replaces_the_active_entry() {
        let mut p = para(&[0xFFFF_FFFF, 0x4000_0001, 0x4000_0002], 1);
        assert_eq!(fall_back(&mut p), Some(1));
        assert_eq!(ids(&p), vec![0xFFFF_FFFF, 0x111, 0x4000_0002]);
        assert_eq!(p[ENTRIES + 16 + 4], 0);
        let mut p = para(&[0x111], 0);
        assert_eq!(fall_back(&mut p), Some(0));
        assert_eq!(ids(&p), vec![0x112]);
    }

    #[test]
    fn index_wraps_and_empty_list() {
        let mut p = para(&[0x4000_0001, 0x113], 2);
        assert_eq!(fall_back(&mut p), Some(1));
        assert_eq!(fall_back(&mut para(&[], 0)), None);
        // a count past the 7 entries that fit is clamped
        let mut p = para(&[0x4000_0001; 7], 0);
        p[4] = 10;
        assert_eq!(fall_back(&mut p), Some(0));
    }
}
