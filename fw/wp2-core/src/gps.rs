//! GPS for apps (docs/abi-1.4.md, "GPS"): the loader turns the stock GPS on and off as the stock workouts do
//! (ux2sys 0x63, docs/recon.md "GPS") and hands the app the stock's parsed GGA. The hold is the heart rate's: one
//! start per hold, one stop at its end, and a GPS the stock already runs is only read.
use crate::events::{Event, GPS};
pub use crate::heart::Heart as Hold;

/// One GPS event per this many ms while the app holds the GPS (the stock parses a GGA about once a second).
pub const EMIT_MS: u32 = 1000;
/// SENSOR record value for the GPS (lvcmd::sensor).
pub const SENSOR_GPS: u8 = 2;
/// Mode the loader sets: the stock workouts' (0x1007B2AC, 0x1007EF3C call sport_view_set_gps_enable(1)).
pub const MODE_ON: u32 = 1;
/// Bytes of the stock info block the loader reads (GGA cells up to the time to first fix at +0x54).
pub const RAW_LEN: usize = 0x58;
/// GPS_FIX record payload: lat_e7 i32, lon_e7 i32, alt_dm i32, ms u32, fix u8, sats u8, ttff_s u16 (LE).
pub const FIX_LEN: usize = 20;

/// Stock info block (0x38016C40, written by the NMEA parser 0x10197D40 on gps_service) to the app's fix. `ms`: the
/// loader's clock when it was read. Degrees are f64 in the block; the app gets 1e-7 degrees.
pub fn fix(raw: &[u8; RAW_LEN], ms: u32) -> [u8; FIX_LEN] {
    let u64_at = |o: usize| u64::from_le_bytes(raw[o..o + 8].try_into().unwrap());
    let u32_at = |o: usize| u32::from_le_bytes(raw[o..o + 4].try_into().unwrap());
    let mut out = [0u8; FIX_LEN];
    out[0..4].copy_from_slice(&f64_scaled(u64_at(0x38), 10_000_000).to_le_bytes());
    out[4..8].copy_from_slice(&f64_scaled(u64_at(0x40), 10_000_000).to_le_bytes());
    out[8..12].copy_from_slice(&f32_scaled(u32_at(0x48), 10).to_le_bytes());
    out[12..16].copy_from_slice(&ms.to_le_bytes());
    out[16] = u32_at(0x4C).min(255) as u8; // GGA fix quality, 0 = no fix
    out[17] = u32_at(0x50).min(255) as u8; // satellites in use
    out[18..20].copy_from_slice(&(u32_at(0x54).min(0xFFFF) as u16).to_le_bytes());
    out
}

// The block's floats × a scale, rounded half away from zero, saturating, NaN 0, in integers only: the loader is
// soft-float, and f64 arithmetic pulled ~2 KiB of compiler-builtins into its 64 KiB.

/// An f64's bits × `scale`, rounded.
pub fn f64_scaled(bits: u64, scale: u32) -> i32 {
    let exp = ((bits >> 52) & 0x7FF) as i32;
    let frac = bits & ((1 << 52) - 1);
    scaled(bits >> 63 != 0, exp, frac | 1 << 52, 1075, frac, 0x7FF, scale)
}

/// An f32's bits × `scale`, rounded.
pub fn f32_scaled(bits: u32, scale: u32) -> i32 {
    let exp = ((bits >> 23) & 0xFF) as i32;
    let frac = u64::from(bits & ((1 << 23) - 1));
    scaled(bits >> 31 != 0, exp, frac | 1 << 23, 150, frac, 0xFF, scale)
}

/// value = m · 2^(exp − bias); exp 0 is zero or subnormal (far below 1 / scale: 0), exp `max` infinity or NaN.
fn scaled(neg: bool, exp: i32, m: u64, bias: i32, frac: u64, max: i32, scale: u32) -> i32 {
    if exp == 0 || (exp == max && frac != 0) {
        return 0;
    }
    let sat = if neg { i32::MIN } else { i32::MAX };
    if exp == max || exp >= bias {
        // infinite, or at least 2^(mantissa bits): beyond any i32 once scaled
        return sat;
    }
    let p = u128::from(m) * u128::from(scale);
    let shift = (bias - exp) as u32;
    let q = if shift >= 127 { 0 } else { (p + (1u128 << (shift - 1))) >> shift };
    match (neg, q) {
        (false, q) if q > i32::MAX as u128 => i32::MAX,
        (true, q) if q > 1u128 << 31 => i32::MIN,
        (false, q) => q as i32,
        (true, q) => (q as i64).wrapping_neg() as i32,
    }
}

/// GPS event: `x` = fix quality (0 = none), `y` = satellites in use. The values come with `Api::gps_fix`.
pub fn event(fix: &[u8; FIX_LEN]) -> Event {
    Event { kind: GPS, x: fix[16].into(), y: fix[17].into(), ..Event::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(lat: f64, lon: f64, alt: f32, quality: u32, sats: u32, ttff: u32) -> [u8; RAW_LEN] {
        let mut r = [0u8; RAW_LEN];
        r[0x38..0x40].copy_from_slice(&lat.to_le_bytes());
        r[0x40..0x48].copy_from_slice(&lon.to_le_bytes());
        r[0x48..0x4C].copy_from_slice(&alt.to_le_bytes());
        r[0x4C..0x50].copy_from_slice(&quality.to_le_bytes());
        r[0x50..0x54].copy_from_slice(&sats.to_le_bytes());
        r[0x54..0x58].copy_from_slice(&ttff.to_le_bytes());
        r
    }

    #[test]
    fn converts_the_stock_block() {
        let f = fix(&raw(12.345678, -98.765432, 123.4, 1, 8, 31), 5000);
        assert_eq!(i32::from_le_bytes(f[0..4].try_into().unwrap()), 123_456_780);
        assert_eq!(i32::from_le_bytes(f[4..8].try_into().unwrap()), -987_654_320);
        assert_eq!(i32::from_le_bytes(f[8..12].try_into().unwrap()), 1234);
        assert_eq!(u32::from_le_bytes(f[12..16].try_into().unwrap()), 5000);
        assert_eq!((f[16], f[17], u16::from_le_bytes([f[18], f[19]])), (1, 8, 31));
        assert_eq!(event(&f), Event { kind: GPS, x: 1, y: 8, ..Event::default() });
    }

    #[test]
    fn integer_scaling_matches_the_float_maths() {
        let f = |v: f64| (v * 1e7 + if v < 0.0 { -0.5 } else { 0.5 }) as i32;
        let g = |v: f32| (v * 10.0 + if v < 0.0 { -0.5 } else { 0.5 }) as i32;
        // (exact half-way values, 0.5e-7 apart, can differ by one: the float sum rounds first; this is exact)
        assert_eq!(f64_scaled(0.00000005f64.to_bits(), 10_000_000), 0);
        for v in [0.0, -0.0, 6.9271, 79.8612, -33.8688197, 151.2092955, 89.9999999, -179.9999999, 1e-9, -1e-9,
                  214.7483647, 214.7483648, -214.7483648, -300.0, 1e300,
                  f64::INFINITY, f64::NEG_INFINITY, f64::NAN, f64::MIN_POSITIVE / 2.0] {
            assert_eq!(f64_scaled(v.to_bits(), 10_000_000), f(v), "{v}");
        }
        for v in [0.0f32, 123.4, -12.0, 8848.86, -430.5, 0.05, -0.05, 0.04, 3.0e9, f32::NAN, f32::INFINITY] {
            assert_eq!(f32_scaled(v.to_bits(), 10), g(v), "{v}");
        }
        // a sweep of coordinates
        let mut x = 0x1234_5678u32;
        for _ in 0..20_000 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let v = (f64::from(x) / f64::from(u32::MAX) - 0.5) * 360.0;
            let (a, b) = (f64_scaled(v.to_bits(), 10_000_000), f(v));
            assert!((a - b).abs() <= 1, "{v}: {a} {b}");
        }
    }

    #[test]
    fn no_fix_is_zeros() {
        let f = fix(&[0; RAW_LEN], 7);
        assert_eq!(&f[..12], &[0; 12]);
        assert_eq!(&f[16..], &[0; 4]);
        assert_eq!(event(&f).x, 0);
    }

    #[test]
    fn layout_matches_sdk() {
        assert_eq!(FIX_LEN, cmf_wp2_app::GPS_FIX_LEN);
        assert_eq!(GPS, cmf_wp2_app::GPS);
        let f = fix(&raw(-0.5, 179.9999999, -12.0, 2, 300, 70_000), 9);
        let s = cmf_wp2_app::GpsFix::from_bytes(&f);
        assert_eq!((s.lat_e7, s.lon_e7, s.alt_dm, s.ms), (-5_000_000, 1_799_999_999, -120, 9));
        assert_eq!((s.fix, s.sats, s.ttff_s), (2, 255, 0xFFFF));
    }
}
