//! Read-only storage query framing and checked ACPT records. No filesystem writes.
pub const OP: u8 = 0x19;
pub const VERSION: i32 = 1;
pub const TABLE_BYTES: usize = 744;
pub const MAX_PARTITIONS: usize = 30;

/// Standalone storage request: 128-byte payload, op/index/reserved followed by output space.
pub const PAYLOAD: usize = 128;
pub fn request(b: &[u8]) -> Option<(u8, u16)> {
    if b.len() == 4 + PAYLOAD && b[..4] == [OP, PAYLOAD as u8, 0, 0] && b[7] == 0 && b[4] <= 8 {
        Some((b[4], u16::from_le_bytes([b[5], b[6]])))
    } else {
        None
    }
}
fn word(b: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(b[n..n + 4].try_into().unwrap())
}
pub fn partition_count(b: &[u8]) -> Result<usize, i32> {
    if b.len() != TABLE_BYTES || &b[..4] != b"ACPT" || b[6..8] != [0xe8, 2] || b[10..12] != [24, 0]
    {
        return Err(-22);
    }
    let n = u16::from_le_bytes([b[8], b[9]]) as usize;
    if n > MAX_PARTITIONS {
        return Err(-22);
    }
    let mut crc = !0u32;
    for &v in &b[..740] {
        crc ^= v as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    if !crc != word(b, 740) {
        return Err(-5);
    }
    Ok(n)
}
pub fn partition(b: &[u8], index: usize, field: u8) -> Result<u32, i32> {
    if index >= partition_count(b)? {
        return Err(-22);
    }
    let e = &b[16 + 24 * index..16 + 24 * (index + 1)];
    match field {
        0 => Ok(u32::from(e[8]) | (u32::from(e[9]) << 8) | (u32::from(e[10]) << 16)),
        1 => Ok(word(e, 12)),
        2 => Ok(word(e, 16)),
        3 => Ok(word(e, 0)),
        4 => Ok(word(e, 4)),
        _ => Err(-22),
    }
}
pub fn capacity(s: &[u32; 4]) -> Result<(u32, u32), i32> {
    if s[1] == 0 || s[3] > s[2] {
        return Err(-95);
    }
    let total = s[1].checked_mul(s[2]).ok_or(-75)?;
    let free = s[1].checked_mul(s[3]).ok_or(-75)?;
    Ok((total, free))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_framing() {
        let mut b = [0u8; 132];
        b[..4].copy_from_slice(&[OP, 128, 0, 0]);
        assert_eq!(request(&b), Some((0, 0)));
        b[4] = 8;
        b[5] = 95;
        assert_eq!(request(&b), Some((8, 95)));
        b[7] = 1;
        assert_eq!(request(&b), None);
        b[7] = 0;
        b[4] = 9;
        assert_eq!(request(&b), None);
        b[4] = 0;
        b[2] = 1;
        assert_eq!(request(&b), None);
        b[2] = 0;
        assert_eq!(request(&b[..131]), None);
        assert_eq!(request(&[OP, 4, 0, 0, 0, 0, 0, 0]), None);
    }
    #[test]
    fn zero_sdfs_is_unknown_not_empty() {
        assert_eq!(capacity(&[512, 0, 0, 0]), Err(-95));
    }
    #[test]
    fn checks_units_and_overflow() {
        assert_eq!(capacity(&[512, 4096, 100, 25]), Ok((409600, 102400)));
        assert_eq!(capacity(&[512, 4096, 100, 101]), Err(-95));
        assert_eq!(capacity(&[512, 4096, u32::MAX, 0]), Err(-75));
    }
    #[test]
    fn malformed_partition_table() {
        assert!(partition_count(&[0; 744]).is_err());
        assert!(partition_count(&[]).is_err());
    }
    #[test]
    fn valid_table_and_crc_corruption() {
        let mut b = [0u8; 744];
        b[..4].copy_from_slice(b"ACPT");
        b[6..8].copy_from_slice(&744u16.to_le_bytes());
        b[8] = 1;
        b[10] = 24;
        b[16..20].copy_from_slice(b"app\0");
        b[24] = 2;
        b[25] = 4;
        b[26] = 0x20;
        b[28..32].copy_from_slice(&4096u32.to_le_bytes());
        b[32..36].copy_from_slice(&8192u32.to_le_bytes());
        let mut crc = !0u32;
        for &v in &b[..740] {
            crc ^= u32::from(v);
            for _ in 0..8 {
                crc = (crc >> 1) ^ if crc & 1 != 0 { 0xedb88320 } else { 0 };
            }
        }
        b[740..].copy_from_slice(&(!crc).to_le_bytes());
        assert_eq!(partition_count(&b), Ok(1));
        assert_eq!(partition(&b, 0, 1), Ok(4096));
        assert_eq!(partition(&b, 0, 2), Ok(8192));
        assert_eq!(partition(&b, 0, 0), Ok(0x200402));
        assert_eq!(partition(&b, 1, 0), Err(-22));
        b[32] ^= 1;
        assert_eq!(partition_count(&b), Err(-5));
    }
}
