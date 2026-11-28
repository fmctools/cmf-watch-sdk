//! Antialiased shapes on top of opaque `rect`. Every pixel is written once per call (no flicker, no overdraw),
//! and same-colour neighbours on a row/column merge into one rect. Edges blend with `bg` (or `under(x, y)`).
//! Radial bands, strokes and runs are agent-glow's code (aa-report.md).

use crate::color::{BLACK, Rgb};
use crate::motion::{Q14, angle, cos, count, isqrt, sin};
use crate::{ONE, Surface};

const CHUNK: i32 = 8;
// coverage steps of an edge pixel in radial bands and strokes
const LEVELS: i32 = 4;
/// `soft` for a gradient across the whole band (clamped to the band widths).
pub const GRAD: i32 = 64;

/// Merges adjacent same-colour pieces of one row (or column, `vert`) into one rect.
pub struct Run<'a, S: Surface> {
    s: &'a mut S,
    at: i32,
    vert: bool,
    from: i32,
    len: i32,
    c: u16,
}

impl<'a, S: Surface> Run<'a, S> {
    pub fn new(s: &'a mut S, at: i32, vert: bool) -> Self {
        Run { s, at, vert, from: 0, len: 0, c: 0 }
    }

    pub fn put(&mut self, from: i32, len: i32, c: u16) {
        if len <= 0 {
            return;
        }
        if self.len > 0 && from == self.from + self.len && c == self.c {
            self.len += len;
            return;
        }
        self.flush();
        (self.from, self.len, self.c) = (from, len, c);
    }

    fn flush(&mut self) {
        if self.len > 0 {
            if self.vert {
                self.s.rect(self.at, self.from, 1, self.len, self.c);
            } else {
                self.s.rect(self.from, self.at, self.len, 1, self.c);
            }
            self.len = 0;
        }
    }
}

impl<S: Surface> Drop for Run<'_, S> {
    fn drop(&mut self) {
        self.flush();
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Paint {
    /// not painted: what is underneath shows through
    Skip,
    Solid(Rgb),
    /// already on screen in this colour: not repainted, only blended into the neighbouring bands
    Keep(Rgb),
    /// colour by angle: `hue[(angle + rot) / 256]` at brightness `level`; `tail`: brightness also falls off behind
    /// `rot` (a comet arc)
    Conic { hue: &'static [Rgb; 256], rot: u16, level: i32, tail: bool },
}

impl Paint {
    fn at(self, a: u16) -> Option<Rgb> {
        match self {
            Paint::Skip => None,
            Paint::Solid(c) | Paint::Keep(c) => Some(c),
            Paint::Conic { hue, rot, level, tail } => {
                let k = if tail { level * (a.wrapping_sub(rot) >> 6) as i32 / ONE } else { level };
                Some(hue[(a.wrapping_add(rot) >> 8) as usize].scale(k))
            }
        }
    }
}

/// A disc or ring up to radius `r`. `soft`: blend width of its outer edge in px: 0 hard, 1 antialiased, more is a
/// gradient into the next band.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Band {
    pub r: i32,
    pub paint: Paint,
    pub soft: i32,
}

fn span<S: Surface>(run: &mut Run<S>, x: i32, w: i32, cx: i32, cy: i32, e: &Edge) {
    if w <= 0 {
        return;
    }
    let p = e.inner;
    match p {
        Paint::Skip | Paint::Keep(_) => {}
        Paint::Solid(_) => run.put(x, w, e.fill),
        Paint::Conic { level, .. } => {
            // dim rings change colour slowly along the row: longer chunks
            let step = if level < ONE / 8 { 4 * CHUNK } else if level < ONE / 2 { 2 * CHUNK } else { CHUNK };
            let mut at = x;
            while at < x + w {
                let n = step.min(x + w - at);
                run.put(at, n, p.at(angle(at + n / 2 - cx, run.at - cy)).unwrap_or(BLACK).rgb565());
                at += n;
            }
        }
    }
}

/// Blend zone of one band edge: pixels with `lo < 4·d² <= hi`, outer weight linear in d².
#[derive(Clone, Copy)]
struct Edge {
    inner: Paint,
    outer: Paint,
    conic: bool,
    kept: bool,
    fill: u16,
    lo: i32,
    hi: i32,
}

/// Colours of an edge at angle `a`, and its blends by coverage step (bit set in `done` once computed).
#[derive(Clone, Copy)]
struct Tint {
    a: u32,
    inner: Option<Rgb>,
    outer: Option<Rgb>,
    done: u32,
    c: [u16; LEVELS as usize + 1],
}

const NO_TINT: Tint = Tint { a: u32::MAX, inner: None, outer: None, done: 0, c: [0; LEVELS as usize + 1] };

#[allow(clippy::too_many_arguments)]
fn edge<S: Surface>(run: &mut Run<S>, x0: i32, x1: i32, cx: i32, cy: i32, e: &Edge, tint: &mut Tint, under: &impl Fn(i32, i32) -> Rgb) {
    if e.kept {
        return;
    }
    let dy = run.at - cy;
    for x in x0..x1 {
        let dx = x - cx;
        if (x - x0) % CHUNK == 0 {
            let a = if e.conic { angle(dx, dy) as u32 } else { 0 };
            if a != tint.a {
                *tint = Tint { a, inner: e.inner.at(a as u16), outer: e.outer.at(a as u16), ..NO_TINT };
            }
        }
        let t = ((4 * (dx * dx + dy * dy) - e.lo) * LEVELS + (e.hi - e.lo) / 2) / (e.hi - e.lo);
        let c = match (tint.inner, tint.outer) {
            (Some(i), Some(o)) => {
                if tint.done >> t & 1 == 0 {
                    tint.c[t as usize] = i.mix(o, t * ONE / LEVELS).rgb565();
                    tint.done |= 1 << t;
                }
                tint.c[t as usize]
            }
            (i, o) => {
                let u = under(x, run.at);
                i.unwrap_or(u).mix(o.unwrap_or(u), t * ONE / LEVELS).rgb565()
            }
        };
        run.put(x, 1, c);
    }
}

pub const MAX_BANDS: usize = 24;

/// Concentric bands (ascending `r`) around (cx, cy), every pixel written once, edges blended per `soft`.
/// `clear`: [x0, y0, x1, y1), containing the disc; the rest of each of its rows is painted `under`.
/// `under`: the colour below a pixel, for Skip bands and the outside of the last band.
/// Takes up to [`MAX_BANDS`] bands and ~2.5 KB of stack (the app has 8 KiB); [`radial_n`] sizes it to fewer.
pub fn radial(s: &mut impl Surface, cx: i32, cy: i32, bands: &[Band], clear: Option<[i32; 4]>, under: impl Fn(i32, i32) -> Rgb) {
    radial_n::<MAX_BANDS>(s, cx, cy, bands, clear, under);
}

/// [`radial`] for at most `N` bands (more are ignored): about 100 B of stack per band.
pub fn radial_n<const N: usize>(s: &mut impl Surface, cx: i32, cy: i32, bands: &[Band], clear: Option<[i32; 4]>, under: impl Fn(i32, i32) -> Rgb) {
    let n = bands.len().min(N);
    if n == 0 {
        return;
    }
    let mut e = [Edge { inner: Paint::Skip, outer: Paint::Skip, conic: false, kept: false, fill: 0, lo: 0, hi: 0 }; N];
    let mut prev = 0;
    for i in 0..n {
        let b = bands[i];
        let next = if i + 1 < n { Some(bands[i + 1]) } else { None };
        let k = b.soft.min(b.r - prev).min(next.map_or(i32::MAX, |o| o.r - b.r)).max(0);
        let r2 = 2 * b.r;
        let outer = next.map_or(Paint::Skip, |o| o.paint);
        let conic = matches!(b.paint, Paint::Conic { .. }) || matches!(outer, Paint::Conic { .. });
        let kept = matches!((b.paint, outer), (Paint::Keep(_), Paint::Keep(_)));
        let fill = b.paint.at(0).unwrap_or(BLACK).rgb565();
        e[i] = Edge { inner: b.paint, outer, conic, kept, fill, lo: (r2 - k).pow(2), hi: (r2 + k).pow(2) };
        prev = b.r;
    }
    let reach = isqrt(e[n - 1].hi / 4);
    let (y0, y1) = clear.map_or((cy - reach, cy + reach + 1), |b| (b[1], b[3]));
    let mut h = [[0i32; 2]; N];
    let mut tints = [[NO_TINT; 2]; N];
    let (mut a, mut b) = ([0i32; N], [0i32; N]);
    for y in y0..y1 {
        let dy2 = (y - cy) * (y - cy);
        for i in 0..n {
            a[i] = count(&mut h[i][0], e[i].lo / 4 - dy2);
            b[i] = if e[i].hi == e[i].lo { a[i] } else { count(&mut h[i][1], e[i].hi / 4 - dy2) };
        }
        let out = b[n - 1];
        let mut run = Run::new(s, y, false);
        if let Some([x0, _, x1, _]) = clear {
            run.put(x0, if out == 0 { x1 } else { cx - out + 1 } - x0, under(x0, y).rgb565());
        }
        // left half (dx >= 1) from the outside in, then the right half (dx >= 0) outwards
        for i in (0..n).rev() {
            let inner = if i == 0 { 1 } else { b[i - 1].max(1) };
            if b[i] > a[i].max(1) {
                edge(&mut run, cx - b[i] + 1, cx - a[i].max(1) + 1, cx, cy, &e[i], &mut tints[i][0], &under);
            }
            span(&mut run, cx - a[i] + 1, a[i] - inner, cx, cy, &e[i]);
        }
        for i in 0..n {
            let inner = if i == 0 { 0 } else { b[i - 1] };
            span(&mut run, cx + inner, a[i] - inner, cx, cy, &e[i]);
            if b[i] > a[i] {
                edge(&mut run, cx + a[i], cx + b[i], cx, cy, &e[i], &mut tints[i][1], &under);
            }
        }
        if let Some([_, _, x1, _]) = clear
            && out > 0
        {
            run.put(cx + out, x1 - cx - out, under(x1 - 1, y).rgb565());
        }
    }
}

/// Antialiased filled disc of radius `r` (edge blended over `bg`).
pub fn disc(s: &mut impl Surface, cx: i32, cy: i32, r: i32, c: Rgb, bg: Rgb) {
    radial_n::<1>(s, cx, cy, &[Band { r, paint: Paint::Solid(c), soft: 1 }], None, |_, _| bg);
}

/// Antialiased ring between radii `r0` and `r1`; the hole is not touched.
pub fn ring(s: &mut impl Surface, cx: i32, cy: i32, r0: i32, r1: i32, c: Rgb, bg: Rgb) {
    let bands = [Band { r: r0, paint: Paint::Skip, soft: 1 }, Band { r: r1, paint: Paint::Solid(c), soft: 1 }];
    radial_n::<2>(s, cx, cy, &bands, None, |_, _| bg);
}

/// Antialiased stroke `w` px wide from `r0` to `r1` px along the Q14 unit vector (ux, uy) from (ox, oy), butt ends,
/// coloured by `shade(distance from the origin, Q14)`. `old`: the `r1` drawn last time on the same line: pixels the
/// old stroke covered and this one does not turn `bg` in the same pass (0 for none).
#[allow(clippy::too_many_arguments)]
pub fn stroke(s: &mut impl Surface, ox: i32, oy: i32, ux: i32, uy: i32, r0: i32, r1: i32, w: i32, old: i32, shade: impl Fn(i32) -> Rgb, bg: Rgb) {
    const Q: i32 = Q14;
    // walk the major axis, runs go across the stroke
    let vert = ux.abs() >= uy.abs();
    let (dm, dn) = if vert { (ux, uy) } else { (uy, ux) };
    if dm == 0 {
        return;
    }
    let (om, on) = if vert { (ox, oy) } else { (oy, ox) };
    let half = (w + 1) * Q / 2;
    let (m0, m1) = (((r0 - 1) * dm) >> 14, ((r1.max(old) + 1) * dm) >> 14);
    let ext = half / dm.abs() + 1;
    let cover = |v: i32| (v.clamp(0, Q) * LEVELS + Q / 2) / Q;
    let bg565 = bg.rgb565();
    for m in m0.min(m1) - 4..=m0.max(m1) + 4 {
        let nc = m * dn / dm;
        let col = shade(m * dm + nc * dn);
        let full = col.rgb565();
        let mut run = Run::new(s, om + m, vert);
        for n in nc - ext..=nc + ext {
            let along = m * dm + n * dn;
            let base = (half - (n * dm - m * dn).abs()).min(along - r0 * Q + Q / 2);
            let k = cover(base.min(r1 * Q + Q / 2 - along));
            if k == LEVELS {
                run.put(on + n, 1, full);
            } else if k > 0 {
                run.put(on + n, 1, bg.mix(col, k * ONE / LEVELS).rgb565());
            } else if cover(base.min(old * Q + Q / 2 - along)) > 0 {
                run.put(on + n, 1, bg565);
            }
        }
    }
}

/// Antialiased line `w` px wide between two points, butt ends.
#[allow(clippy::too_many_arguments)]
pub fn line(s: &mut impl Surface, x0: i32, y0: i32, x1: i32, y1: i32, w: i32, c: Rgb, bg: Rgb) {
    let (dx, dy) = (x1 - x0, y1 - y0);
    // length in 1/16 px, so the direction is a unit vector to Q14 precision
    let len = isqrt(256 * (dx * dx + dy * dy));
    if len == 0 {
        return;
    }
    stroke(s, x0, y0, dx * Q14 * 16 / len, dy * Q14 * 16 / len, 0, (len + 8) / 16, w, 0, |_| c, bg);
}

// coverage steps of arc and rounded-rect edges
const STEPS: i32 = 8;

/// Antialiased arc of the ring `r0..r1` from angle `start` clockwise over `sweep` (65536 = full turn, butt ends).
/// `off`: also paint the rest of the ring in this colour (a progress track), in the same pass.
#[allow(clippy::too_many_arguments)]
pub fn arc(s: &mut impl Surface, cx: i32, cy: i32, r0: i32, r1: i32, start: u16, sweep: u32, on: Rgb, off: Option<Rgb>, bg: Rgb) {
    let full = sweep >= 65536;
    let (u0, u1) = ((cos(start), sin(start)), {
        let a = start.wrapping_add(sweep.min(65535) as u16);
        (cos(a), sin(a))
    });
    let wide = sweep > 32768;
    let (lo_i, hi_i) = ((2 * r0 - 1).max(0).pow(2), (2 * r0 + 1).pow(2));
    let (lo_o, hi_o) = ((2 * r1 - 1).pow(2), (2 * r1 + 1).pow(2));
    let level = |v: i32, lo: i32, hi: i32| (((v - lo) * STEPS + (hi - lo) / 2) / (hi - lo)).clamp(0, STEPS);
    // signed distance (Q14 px) to a ray → coverage steps
    let side = |d: i32| ((d + Q14 / 2) * STEPS / Q14).clamp(0, STEPS);
    for y in cy - r1 - 1..=cy + r1 + 1 {
        let dy = y - cy;
        let reach = isqrt((hi_o / 4 - dy * dy).max(0)) + 1;
        let m = lo_i - 4 * dy * dy;
        let hole = if r0 > 0 && m >= 0 { isqrt(m / 4) + 1 } else { 0 };
        let mut run = Run::new(s, y, false);
        for dx in (-reach..=reach).filter(|dx| dx.abs() >= hole) {
            let q = 4 * (dx * dx + dy * dy);
            let radial = (STEPS - level(q, lo_o, hi_o)).min(if r0 > 0 { level(q, lo_i, hi_i) } else { STEPS });
            if radial <= 0 {
                continue;
            }
            let a = if full {
                STEPS
            } else {
                let d0 = side(u0.0 * dy - u0.1 * dx);
                let d1 = side(u1.1 * dx - u1.0 * dy);
                if wide { d0.max(d1) } else { d0.min(d1) }
            };
            let c = match off {
                Some(o) => bg.mix(o.mix(on, a * ONE / STEPS), radial * ONE / STEPS),
                None if a == 0 => continue,
                None => bg.mix(on, radial.min(a) * ONE / STEPS),
            };
            run.put(cx + dx, 1, c.rgb565());
        }
    }
}

/// Antialiased rounded rectangle; `r` is clamped to half the shorter side (so `r = h / 2` is a pill). Corner pixels
/// outside the shape are not touched; the straight middle rows are one rect.
#[allow(clippy::too_many_arguments)]
pub fn rrect(s: &mut impl Surface, x: i32, y: i32, w: i32, h: i32, r: i32, c: Rgb, bg: Rgb) {
    if w <= 0 || h <= 0 {
        return;
    }
    let r = r.clamp(0, w.min(h) / 2);
    let fill = c.rgb565();
    let (lo, hi) = ((2 * r - 1).max(0).pow(2), (2 * r + 1).pow(2));
    for row in (0..r).chain(h - r..h) {
        // doubled distance from the corner centre to the pixel centre
        let dy = if row < r { 2 * row + 1 - 2 * r } else { 2 * row + 1 - 2 * (h - r) };
        let k = |dx: i32| {
            let q = dx * dx + dy * dy;
            if q <= lo { STEPS } else { (((hi - q) * STEPS + (hi - lo) / 2) / (hi - lo)).clamp(0, STEPS) }
        };
        let px = |k: i32| if k == STEPS { fill } else { bg.mix(c, k * ONE / STEPS).rgb565() };
        let mut run = Run::new(s, y + row, false);
        for col in 0..r {
            let k = k(2 * col + 1 - 2 * r);
            if k > 0 {
                run.put(x + col, 1, px(k));
            }
        }
        run.put(x + r, w - 2 * r, fill);
        for col in w - r..w {
            let k = k(2 * col + 1 - 2 * (w - r));
            if k > 0 {
                run.put(x + col, 1, px(k));
            }
        }
    }
    s.rect(x, y + r, w, h - 2 * r, fill);
}

/// Linear gradient `from` → `to`, top to bottom (`vert`) or left to right; one rect per distinct 565 colour.
#[allow(clippy::too_many_arguments)]
pub fn gradient(s: &mut impl Surface, x: i32, y: i32, w: i32, h: i32, from: Rgb, to: Rgb, vert: bool) {
    let n = if vert { h } else { w };
    let (mut at, mut c) = (0, from.rgb565());
    for i in 1..=n {
        let next = if i < n { from.mix(to, i * ONE / (n - 1).max(1)).rgb565() } else { !c };
        if next != c {
            if vert { s.rect(x, y + at, w, i - at, c) } else { s.rect(x + at, y, i - at, h, c) }
            (at, c) = (i, next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::WHITE;
    use crate::{C, Count, Fb, SIZE};
    use std::vec;
    use std::vec::Vec;

    pub(crate) struct Buf {
        pub px: Vec<u16>,
        pub writes: Vec<u8>,
        pub rects: u32,
    }

    impl Buf {
        pub fn new(c: u16) -> Self {
            Buf { px: vec![c; (SIZE * SIZE) as usize], writes: vec![0; (SIZE * SIZE) as usize], rects: 0 }
        }

        pub fn at(&self, x: i32, y: i32) -> u16 {
            self.px[(y * SIZE + x) as usize]
        }

        pub fn g(&self, x: i32, y: i32) -> f64 {
            ((self.at(x, y) >> 5) & 63) as f64 / 63.0
        }
    }

    impl Surface for Buf {
        fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
            if w <= 0 || h <= 0 {
                return;
            }
            self.rects += 1;
            for yy in y.max(0)..(y + h).min(SIZE) {
                for xx in x.max(0)..(x + w).min(SIZE) {
                    let i = (yy * SIZE + xx) as usize;
                    self.px[i] = c;
                    self.writes[i] = self.writes[i].saturating_add(1);
                }
            }
        }

        fn text(&mut self, _: i32, _: i32, _: &str, _: u16) {}
    }

    fn dist(x: i32, y: i32, cx: i32, cy: i32) -> f64 {
        (((x - cx) * (x - cx) + (y - cy) * (y - cy)) as f64).sqrt()
    }

    static RAMP: [Rgb; 256] = {
        let mut t = [BLACK; 256];
        let mut i = 0;
        while i < 256 {
            t[i] = Rgb(i as u8, 255 - i as u8, 128);
            i += 1;
        }
        t
    };

    // agent-glow's radial tests: bands with an AA edge, a conic gradient band and a clear box
    #[test]
    fn radial_writes_each_pixel_once() {
        let mut b = Buf::new(0xFFFF);
        let red = Rgb(0xFF, 0, 0);
        let bands = [
            Band { r: 10, paint: Paint::Solid(red), soft: 1 },
            Band { r: 25, paint: Paint::Conic { hue: &RAMP, rot: 0, level: ONE, tail: false }, soft: GRAD },
            Band { r: 31, paint: Paint::Solid(WHITE), soft: 1 },
        ];
        radial(&mut b, 100, 120, &bands, Some([60, 70, 150, 160]), |_, _| BLACK);
        for y in 0..SIZE {
            for x in 0..SIZE {
                let i = (y * SIZE + x) as usize;
                let inside = (60..150).contains(&x) && (70..160).contains(&y);
                assert_eq!(b.writes[i], inside as u8, "({x},{y})");
                let d = dist(x, y, 100, 120);
                if d >= 31.5 && inside {
                    assert_eq!(b.px[i], 0);
                }
                if d < 9.5 {
                    assert_eq!(b.px[i], red.rgb565());
                }
            }
        }
    }

    #[test]
    fn radial_gradient_band_falls_off() {
        let mut b = Buf::new(0);
        let bands = [Band { r: 10, paint: Paint::Solid(WHITE), soft: GRAD }, Band { r: 20, paint: Paint::Solid(BLACK), soft: 1 }];
        radial(&mut b, 100, 100, &bands, None, |_, _| BLACK);
        let row: Vec<f64> = (100..125).map(|x| b.g(x, 100)).collect();
        assert!(row.windows(2).all(|w| w[1] <= w[0]), "{row:?}");
        assert!(row[0] == 1.0 && (row[10] - 0.5).abs() < 0.15 && row[16] == 0.0, "{row:?}");
    }

    // agent-glow's bars: shrinking a stroke erases what it no longer covers, in the same pass
    #[test]
    fn stroke_antialiases_and_erases() {
        let bar = |b: &mut Buf, th: u16, r1: i32, old: i32| stroke(b, C, C, cos(th), sin(th), 98, r1, 5, old, |_| WHITE, BLACK);
        for th in [0u16, 5000, 16384, 30000, 47000] {
            let (mut grown, mut fresh) = (Buf::new(0), Buf::new(0));
            bar(&mut grown, th, 170, 0);
            bar(&mut grown, th, 120, 170);
            bar(&mut fresh, th, 120, 0);
            assert!(grown.px == fresh.px, "th {th}: shrinking left pixels behind");
            assert!(fresh.writes.iter().all(|&w| w <= 1), "th {th}");
            let (ux, uy) = (cos(th) as f64 / 16384.0, sin(th) as f64 / 16384.0);
            let mut blended = 0;
            for y in 0..SIZE {
                for x in 0..SIZE {
                    let (px, py) = ((x - C) as f64, (y - C) as f64);
                    let (along, perp) = (px * ux + py * uy, (py * ux - px * uy).abs());
                    let cover = (3.0 - perp).min(along - 97.5).min(120.5 - along).clamp(0.0, 1.0);
                    assert!((fresh.g(x, y) - cover).abs() <= 0.2, "th {th} ({x},{y})");
                    blended += (fresh.g(x, y) > 0.0 && fresh.g(x, y) < 1.0) as i32;
                }
            }
            assert!(blended >= 10, "th {th}: {blended}");
        }
    }

    #[test]
    fn disc_coverage_and_single_writes() {
        let mut b = Buf::new(0);
        disc(&mut b, 100, 100, 20, WHITE, BLACK);
        let mut blended = 0;
        for y in 70..130 {
            for x in 70..130 {
                let cover = (20.5 - dist(x, y, 100, 100)).clamp(0.0, 1.0);
                assert!((b.g(x, y) - cover).abs() <= 0.2, "({x},{y})");
                assert!(b.writes[(y * SIZE + x) as usize] <= 1);
                blended += (b.g(x, y) > 0.0 && b.g(x, y) < 1.0) as i32;
            }
        }
        assert!(blended > 100);
        let mut n = Count::default();
        disc(&mut n, 100, 100, 20, WHITE, BLACK);
        assert!(n.rects < 200, "{} rects", n.rects);
    }

    #[test]
    fn ring_leaves_hole_and_blends_with_bg() {
        let bg = Rgb(0, 0, 255);
        let mut b = Buf::new(0x1234);
        ring(&mut b, 200, 200, 30, 40, WHITE, bg);
        assert_eq!(b.at(200, 200), 0x1234, "hole untouched");
        assert_eq!(b.at(235, 200), 0xFFFF);
        assert_eq!(b.at(200, 241), 0x1234);
        let edge = Rgb::from565(b.at(240, 200));
        assert!(edge.2 > 200 && edge.0 > 60 && edge.0 < 200, "{edge:?}");
    }

    #[test]
    fn shapes_clip_at_screen_edges() {
        let mut px = vec![0u16; (SIZE * SIZE) as usize];
        let mut fb = Fb::new(&mut px);
        disc(&mut fb, 0, 0, 30, WHITE, BLACK);
        disc(&mut fb, SIZE - 1, SIZE - 1, 30, WHITE, BLACK);
        arc(&mut fb, SIZE, C, 20, 40, 0, 65536, WHITE, None, BLACK);
        rrect(&mut fb, -20, SIZE - 10, 60, 40, 12, WHITE, BLACK);
        line(&mut fb, -50, -50, 30, 30, 5, WHITE, BLACK);
        assert_eq!(fb.at(0, 0), 0xFFFF);
        assert_eq!(fb.at(SIZE - 1, SIZE - 1), 0xFFFF);
        assert_eq!((fb.at(SIZE - 30, C), fb.at(SIZE - 1, C)), (0xFFFF, 0));
        assert_eq!(fb.at(C, C), 0);
    }

    #[test]
    fn line_covers_and_counts() {
        for (x1, y1) in [(300, 100), (100, 300), (300, 300), (40, 240), (250, 101)] {
            let mut b = Buf::new(0);
            line(&mut b, 100, 100, x1, y1, 6, WHITE, BLACK);
            let (dx, dy) = ((x1 - 100) as f64, (y1 - 100) as f64);
            let len = (dx * dx + dy * dy).sqrt();
            let mut blended = 0;
            for y in 0..SIZE {
                for x in 0..SIZE {
                    let (px, py) = ((x - 100) as f64, (y - 100) as f64);
                    let (along, perp) = ((px * dx + py * dy) / len, ((py * dx - px * dy) / len).abs());
                    let cover = (3.5 - perp).min(along + 0.5).min(len.round() + 0.5 - along).clamp(0.0, 1.0);
                    assert!((b.g(x, y) - cover).abs() <= 0.3, "to ({x1},{y1}) at ({x},{y}): {} vs {cover}", b.g(x, y));
                    assert!(b.writes[(y * SIZE + x) as usize] <= 1);
                    blended += (b.g(x, y) > 0.0 && b.g(x, y) < 1.0) as i32;
                }
            }
            assert!(blended > 20);
            assert!(b.rects < 4 * len as u32, "{} rects for {len:.0} px", b.rects);
        }
    }

    #[test]
    fn arc_sweeps_and_tracks() {
        let (on, off) = (WHITE, Rgb(0, 0, 255));
        // a quarter from 12 o'clock clockwise: top-right quadrant lit, the rest track
        let mut b = Buf::new(0);
        arc(&mut b, C, C, 100, 120, 49152, 16384, on, Some(off), BLACK);
        assert_eq!(b.at(C + 78, C - 78), 0xFFFF);
        assert_eq!(b.at(C - 78, C - 78), off.rgb565());
        assert_eq!(b.at(C, C + 110), off.rgb565());
        assert_eq!(b.at(C, C), 0, "hole untouched");
        assert!(b.writes.iter().all(|&w| w <= 1));
        let (mut lit, mut soft) = (0, 0);
        for y in 0..SIZE {
            for x in 0..SIZE {
                let d = dist(x, y, C, C);
                let r = (120.5 - d).min(d - 99.5).clamp(0.0, 1.0);
                let k = Rgb::from565(b.at(x, y));
                assert!((k.2 as f64 / 255.0 - r).abs() <= 0.2, "({x},{y}) radial");
                lit += (k.0 > 128) as i32;
                soft += (k.0 > 0 && k.0 < 240) as i32;
            }
        }
        let quarter = std::f64::consts::PI * (120.0f64.powi(2) - 100.0f64.powi(2)) / 4.0;
        assert!((lit as f64 - quarter).abs() < quarter * 0.05, "{lit} vs {quarter}");
        assert!(soft > 30);
        // more than half a turn, no track: only the lit part is written
        let mut b = Buf::new(0);
        arc(&mut b, C, C, 100, 120, 0, 49152, on, None, BLACK);
        assert_eq!(b.at(C + 110, C + 1), 0xFFFF);
        assert_eq!(b.at(C - 110, C + 1), 0xFFFF);
        assert_eq!(b.writes[((C - 110) * SIZE + C + 30) as usize], 0);
        let mut n = Count::default();
        arc(&mut n, C, C, 130, 150, 49152, 40000, on, Some(off), BLACK);
        assert!(n.rects < 2500, "{} rects", n.rects);
    }

    #[test]
    fn rrect_corners_and_pill() {
        let mut b = Buf::new(0);
        rrect(&mut b, 100, 100, 120, 40, 20, WHITE, BLACK);
        assert_eq!(b.at(160, 120), 0xFFFF);
        assert_eq!(b.at(100, 100), 0, "corner outside");
        assert_eq!(b.at(100, 120), 0xFFFF, "pill side");
        assert_eq!(b.at(160, 100), 0xFFFF);
        assert_eq!(b.at(160, 140), 0, "below");
        for y in 95..145 {
            for x in 95..225 {
                let (qx, qy) = ((x as f64 + 0.5).clamp(120.0, 200.0), (y as f64 + 0.5).clamp(120.0, 120.0));
                let d = ((x as f64 + 0.5 - qx).powi(2) + (y as f64 + 0.5 - qy).powi(2)).sqrt();
                let cover = (20.0 - d + 0.5).clamp(0.0, 1.0);
                assert!((b.g(x, y) - cover).abs() <= 0.25, "({x},{y}) {} vs {cover}", b.g(x, y));
            }
        }
        assert!(b.writes.iter().all(|&w| w <= 1));
        assert!(b.rects < 160, "{}", b.rects);
        let mut b = Buf::new(0);
        rrect(&mut b, 10, 10, 50, 50, 0, WHITE, BLACK);
        assert_eq!((b.rects, b.writes.iter().filter(|&&w| w > 0).count()), (1, 2500));
    }

    #[test]
    fn gradient_merges_equal_colours() {
        let mut b = Buf::new(0);
        gradient(&mut b, 0, 0, 100, 200, BLACK, Rgb(0, 255, 0), true);
        assert_eq!((b.at(5, 0), b.at(5, 199)), (0, Rgb(0, 255, 0).rgb565()));
        assert!(b.rects <= 64, "{}", b.rects);
        assert!((1..200).all(|y| b.g(5, y) >= b.g(5, y - 1)));
        assert_eq!(b.writes.iter().filter(|&&w| w == 1).count(), 100 * 200);
        let mut b = Buf::new(0);
        gradient(&mut b, 10, 10, 1, 1, WHITE, BLACK, false);
        assert_eq!(b.at(10, 10), 0xFFFF);
    }
}

/// The round scroll bar of the stock lists (the Apps menu's `jx_scroll_bar`): a short grey arc on the right edge with
/// a white thumb, round ends. `at` of `max` scrolled (0..=max), `view`: how much shows at once (same unit); nothing
/// when it does not scroll. Repaints the whole bar, so it can be drawn again after a scroll without clearing.
/// A row of pixels at a time between two circles (no arc code of its own: the apps using it are near their limit).
pub fn scroll_bar(s: &mut impl Surface, at: i32, max: i32, view: i32, bg: Rgb) {
    if max <= 0 {
        return;
    }
    // rows -H..=H about the centre (±24°), between radius 220 and 226
    const H: i32 = 90;
    const R0: i32 = 220;
    const R1: i32 = 226;
    let len = 2 * H;
    let thumb = (len * view / (max + view)).clamp(len / 5, len);
    let t0 = -H + (len - thumb) * at.clamp(0, max) / max;
    let (white, grey) = (Rgb(0xFF, 0xFF, 0xFF), Rgb(0x3A, 0x3A, 0x3A));
    let x = |r: i32, dy: i32| crate::C + isqrt(r * r - dy * dy);
    for dy in -H..=H {
        let c = if (t0..=t0 + thumb).contains(&dy) { white } else { grey };
        let (x0, x1) = (x(R0, dy), x(R1, dy));
        s.rect(x0, crate::C + dy, x1 - x0, 1, c.rgb565());
    }
    // round ends: the bar's, then the thumb's
    for (dy, c) in [(-H, grey), (H, grey), (t0, white), (t0 + thumb, white)] {
        let xm = (x(R0, dy) + x(R1, dy)) / 2;
        disc(s, xm, crate::C + dy, (R1 - R0) / 2, c, bg);
    }
}
