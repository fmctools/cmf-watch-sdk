// The procedural widgets of Watch 3 Pro faces (wfc tag 0x82 + 0x5C): type 4 (Rainbow Lollipop's rings), 5 (Briqbot's
// clips), 6 (DataPulse's five panels), 7 (Luminous Beam's calories). Written from their observed behaviour and fed
// from the WP2's own ux2sys data (docs/recon.md, "Watch 3 Pro faces"). Pure: the stub's wfproc.rs draws it.


/// images in the weather panel's list; the other panels have 13 (digits, the two axes, the dash)
pub const IMAGES: usize = 88;
/// sleep: 50 stages and 8 time glyphs
pub const ITEMS: usize = 60;
/// Item.img of a rectangle
pub const RECT: u8 = 0xFF;
pub const GREY: u16 = 0x73AE;
pub const WHITE: u16 = 0xFFFF;
/// by stage, one row of the panel each
pub const STAGES: [u16; 4] = [0xE71C, 0x73AE, 0x3186, 0xFA23];

/// ux2sys 0x14, by value: u8 start hour +2, the current code +0xC and temperature +0x10, 24 hours of
/// {i32 degrees C, u8 code} from +0x98 (the first is start + 1), the city name +0x158
pub const WEATHER_LEN: usize = 0x1B0;
/// ux2sys 0xD, by pointer: 4 bytes per hour from +0x120 (two half hours, max then min), the day's max +0x180, min
/// +0x181. Not the 3 Pro's offsets (+0x5A0, +0x600)
pub const HEART_LEN: usize = 0x182;
/// ux2sys 0x17 (SpO2) and 0x80 (stress), by pointer: 2 bytes per hour from +0x30 and +0x31
pub const HOURLY_LEN: usize = 0x70;
/// ux2sys 0x16, by pointer: u8 valid +0x5E, u32 start and end (UTC) +0x64 and +0x68, stages {u8 kind +0x6C,
/// u32 seconds +0x70} 8 bytes each, u16 count +0x1FC
pub const SLEEP_LEN: usize = 0x1FE;

// the panels inside the 466x466 widget
const PANEL_X: i32 = 11;
const TOP_Y: i32 = 2;
const LOW_Y: i32 = 293;
const PLOT_X: i32 = PANEL_X + 79;
const PLOT_Y: i32 = LOW_Y + 4;
const PLOT_H: i32 = 79;
const BOTTOM: i32 = PLOT_H - 1;
const HOURS: usize = 24;
const BAR_W: i32 = 8;
const GLYPH_W: i32 = 14;

// image indices in the weather list
const ICONS: u8 = 0x1B;
const DIGIT: u8 = 0x1B;
const DEGREE: u8 = 0x25;
const MINUS: u8 = 0x26;
const NOW: u8 = 0x27;
const LABEL_12: u8 = 0x28;
const LABEL_24: u8 = 0x40;
// in the other panels' lists
const AXIS_24: u8 = 10;
const AXIS_12: u8 = 11;
const DASH: u8 = 12;

#[derive(Clone, Copy)]
pub struct Clock {
    pub hour: u8,
    pub twelve: bool,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Item {
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub h: i16,
    pub color: u16,
    /// images: tenths of a degree, clockwise around the centre
    pub angle: i16,
    pub img: u8,
    pub radius: u8,
}

const EMPTY: Item = Item { x: 0, y: 0, w: 0, h: 0, color: 0, angle: 0, img: RECT, radius: 0 };

pub struct Scene {
    pub items: [Item; ITEMS],
    pub len: usize,
}

impl Scene {
    pub fn new() -> Self {
        Scene { items: [EMPTY; ITEMS], len: 0 }
    }

    fn push(&mut self, item: Item) {
        if let Some(slot) = self.items.get_mut(self.len) {
            *slot = item;
            self.len += 1;
        }
    }

    fn image(&mut self, img: u8, x: i32, y: i32) {
        self.push(Item { x: x as i16, y: y as i16, img, ..EMPTY });
    }

    fn rect(&mut self, (x, y, w, h): (i32, i32, i32, i32), color: u16, radius: u8) {
        self.push(Item { x: x as i16, y: y as i16, w: w as i16, h: h as i16, color, radius, ..EMPTY });
    }

    /// FNV-1a of what is drawn, never 0: the stub rebuilds the objects only when it changes.
    pub fn digest(&self) -> u32 {
        let mut hash = 0x811C_9DC5u32;
        for i in &self.items[..self.len.min(ITEMS)] {
            let look = u16::from_le_bytes([i.img, i.radius]);
            for word in [i.x as u16, i.y as u16, i.w as u16, i.h as u16, i.color, i.angle as u16, look] {
                hash = (hash ^ u32::from(word)).wrapping_mul(0x0100_0193);
            }
        }
        hash | 1
    }
}

fn word(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]])
}

/// This hour and the next three as the stock weather card reads them (0x100ACB20).
fn hours(d: &[u8; WEATHER_LEN], hour: u8, linked: bool) -> [Option<(i32, u8)>; 4] {
    let mut out = [None; 4];
    if usize::from(hour) >= HOURS || hour < d[2] || !(linked || d[0x158] != 0) {
        return out;
    }
    let k = usize::from(hour - d[2]);
    for (i, slot) in out.iter_mut().enumerate() {
        if k + i >= HOURS {
            break;
        }
        let (temp, code) = if k + i == 0 {
            (word(d, 0x10), d[0x0C])
        } else {
            let at = 0x98 + (k + i - 1) * 8;
            (word(d, at), d[at + 4])
        };
        if temp != 0xFF {
            *slot = Some((temp as i32, if code < ICONS { code } else { 1 }));
        }
    }
    out
}

/// (image, x in the column) of an hour's label
fn label(hour: u8, twelve: bool) -> (u8, i32) {
    if !twelve {
        (LABEL_24 + hour, 17)
    } else {
        (LABEL_12 + hour, if matches!(hour, 0 | 10 | 11 | 12 | 22 | 23) { 0 } else { 7 })
    }
}

pub fn weather(s: &mut Scene, d: &[u8; WEATHER_LEN], clock: Clock, fahrenheit: bool, linked: bool) {
    let y = TOP_Y + 49;
    for (i, value) in hours(d, clock.hour, linked).iter().enumerate() {
        let x = PANEL_X + 76 + 77 * i as i32;
        let (img, dx) = if i == 0 { (NOW, 7) } else { label((clock.hour % 24 + i as u8) % 24, clock.twelve) };
        s.image(img, x + dx, y);
        s.image(value.map_or(0, |v| v.1), x + 17, y + 40);

        let mut glyphs = [MINUS; 4];
        let mut n = 2;
        if let Some((celsius, _)) = *value {
            // three digits fit the column; clamped first, the conversion must not overflow
            let t = celsius.max(-999).min(999);
            let t = if fahrenheit { (t * 9 / 5 + 32).max(-999).min(999) } else { t };
            let v = t.unsigned_abs();
            n = usize::from(t < 0);
            for (scale, shown) in [(100, v >= 100), (10, v >= 10), (1, true)] {
                if shown {
                    glyphs[n] = DIGIT + (v / scale % 10) as u8;
                    n += 1;
                }
            }
        }
        let centred = if value.is_some() { n } else { 1 };
        let mut gx = x + (65 - GLYPH_W * centred as i32) / 2;
        for &glyph in &glyphs[..n] {
            s.image(glyph, gx, y + 97);
            gx += GLYPH_W;
        }
        s.image(DEGREE, gx, y + 97);
    }
}

fn axis(s: &mut Scene, clock: Clock) {
    s.image(if clock.twelve { AXIS_12 } else { AXIS_24 }, PANEL_X + 69, LOW_Y + 2);
}

/// One hour's bar; top and bottom are rows of the plot, at least `least` + 1 rows are drawn.
fn bar(s: &mut Scene, clock: Clock, hour: usize, top: i32, bottom: i32, least: i32) {
    let top = top.max(0).min(BOTTOM - least);
    let bottom = bottom.max(top + least).min(BOTTOM);
    let x = PLOT_X + 277 * hour as i32 / (HOURS as i32 - 1);
    let color = if hour == usize::from(clock.hour) { WHITE } else { GREY };
    s.rect((x, PLOT_Y + top, BAR_W, bottom - top + 1), color, BAR_W as u8 / 2);
}

fn upper(v: i32, min: i32, max: i32) -> i32 {
    PLOT_H - (v - min) * PLOT_H / (max - min).max(1)
}

fn lower(v: i32, min: i32, max: i32) -> i32 {
    BOTTOM - (v - min) * BOTTOM / (max - min).max(1)
}

pub fn heart(s: &mut Scene, d: &[u8; HEART_LEN], clock: Clock) {
    axis(s, clock);
    let (max, min) = (i32::from(d[0x180]), i32::from(d[0x181]));
    if max == 0 || min == 0 {
        return;
    }
    for h in 0..HOURS {
        let v = &d[0x120 + h * 4..0x124 + h * 4];
        let high = i32::from(v[0].max(v[2]));
        if high == 0 {
            continue;
        }
        let low = i32::from(if v[1] == 0 { v[3] } else if v[3] == 0 { v[1] } else { v[1].min(v[3]) });
        if min == max {
            bar(s, clock, h, upper(high, min - 30, max), 4, 4);
        } else {
            bar(s, clock, h, upper(high, min, max), lower(low, min, max), 1);
        }
    }
}

pub fn spo2(s: &mut Scene, d: &[u8; HOURLY_LEN], clock: Clock) {
    axis(s, clock);
    let hours = &d[0x30..0x30 + 2 * HOURS];
    // the 3 Pro's getter fills in the day's max and min, the WP2's does not
    let (mut min, mut max) = (i32::MAX, 0);
    for &v in hours.iter().filter(|&&v| v != 0) {
        min = min.min(i32::from(v));
        max = max.max(i32::from(v));
    }
    if min >= max {
        // one value all day
        min = 85;
        max = 99;
    }
    for (h, v) in hours.chunks_exact(2).enumerate() {
        let (high, low) = (i32::from(v[0]), i32::from(v[1]));
        if high == 0 {
            continue;
        }
        if high < 90 {
            bar(s, clock, h, BOTTOM - 4, BOTTOM, 4);
        } else {
            let low = if low == 0 { high } else { low };
            bar(s, clock, h, upper(high, min, max), lower(low, min, max), 4);
        }
    }
}

pub fn stress(s: &mut Scene, d: &[u8; HOURLY_LEN], clock: Clock) {
    axis(s, clock);
    for (h, v) in d[0x31..0x31 + 2 * HOURS].chunks_exact(2).enumerate() {
        if v[0] != 0 {
            bar(s, clock, h, upper(i32::from(v[0]), 0, 100), lower(i32::from(v[1]), 0, 100), 1);
        }
    }
}

/// tz: seconds east of UTC
pub fn sleep(s: &mut Scene, d: &[u8; SLEEP_LEN], tz: i32) {
    let count = usize::from(u16::from_le_bytes([d[0x1FC], d[0x1FD]]));
    let valid = d[0x5E] != 0 && count != 0;
    let (start, end) = (word(d, 0x64), word(d, 0x68));
    if valid && count <= 50 && end > start {
        // the stages share 284 px with 2 px between them
        let room = 284 - 2 * count as i32;
        let mut x = 0;
        for i in 0..count {
            let seconds = word(d, 0x70 + i * 8);
            if seconds == 0 || x >= 284 {
                continue;
            }
            let w = ((room as u32).saturating_mul(seconds) / (end - start)).max(1).min((284 - x) as u32) as i32;
            let kind = usize::from(d[0x6C + i * 8]);
            let (row, color) = STAGES.get(kind).map_or((0, 0), |&c| (kind as i32, c));
            s.rect((PANEL_X + 80 + x, LOW_Y + 6 + 17 * row, w, 17), color, 0);
            x += w + 2;
        }
    }
    for (time, xs) in [(start, [0x50, 0x60, 0x80, 0x92]), (end, [0x11E, 0x12E, 0x14E, 0x15E])] {
        let day = (time % 86_400) as i32 + tz % 86_400 + 2 * 86_400;
        let (h, m) = (day / 3600 % 24, day / 60 % 60);
        for (&x, v) in xs.iter().zip([h / 10, h % 10, m / 10, m % 10]) {
            s.image(if valid { v as u8 } else { DASH }, PANEL_X + x, LOW_Y + 0x55);
        }
    }
}

/// Type 7, percent of the calorie goal along the dial: three digits of the widget's list, units first.
pub fn calories(s: &mut Scene, goal: u32, burned: u32) {
    let pct = if goal == 0 { 100 } else { (burned.saturating_mul(100) / goal).min(100) };
    let digits = [(pct % 10, true, 275, 332, 3350), (pct / 10 % 10, pct >= 10, 259, 338, 3440), (pct / 100, pct >= 100, 242, 341, 3530)];
    for (digit, shown, x, y, angle) in digits {
        if shown {
            s.push(Item { img: digit as u8, x, y, angle, ..EMPTY });
        }
    }
}

// type 4: rings 4 px wide, 16 px apart, the colours repeat after 9
const CENTRE: i32 = 233;
const RINGS: usize = 14;
const RING_IN: i32 = 21;
const RING_STEP: i32 = 16;
pub const RING_W: i32 = 4;
const RING_REACH: i32 = 4;
const RING_COLORS: [u16; 9] = [64737, 65185, 22244, 26297, 3135, 19834, 23292, 47838, 64039];

/// Type 4: the ring under a touch.
pub fn ring(x: i32, y: i32) -> Option<usize> {
    let (dx, dy) = (x.saturating_sub(CENTRE), y.saturating_sub(CENTRE));
    let d2 = dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy));
    (0..RINGS).find(|&i| {
        let inner = RING_IN + RING_STEP * i as i32 - RING_REACH;
        // up to outer + reach, the distance rounded down
        let past = RING_IN + RING_STEP * i as i32 + RING_W + RING_REACH + 1;
        d2 >= inner * inner && d2 < past * past
    })
}

/// (x and y, diameter, colour) of the band that lights a ring up.
pub fn ring_band(ring: usize) -> (i32, i32, u16) {
    let r = RING_IN + RING_STEP * ring as i32 + RING_W;
    (CENTRE - r, 2 * r, RING_COLORS[ring % RING_COLORS.len()])
}

// type 5: first and last frame of each clip
const CLIPS: [(u8, u8); 3] = [(0, 39), (40, 89), (90, 158)];
/// Type 5: ms per frame.
pub const FRAME_MS: u32 = 33;

pub fn next_clip(clip: u8) -> u8 {
    if usize::from(clip) + 1 < CLIPS.len() { clip + 1 } else { 0 }
}

pub fn next_frame(frame: u8, clip: u8) -> u8 {
    let (first, last) = CLIPS[usize::from(clip) % CLIPS.len()];
    if frame < first || frame > last {
        first
    } else if frame == last {
        last
    } else {
        frame + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(s: &Scene) -> &[Item] {
        &s.items[..s.len]
    }

    fn images(s: &Scene) -> std::vec::Vec<(u8, i16, i16)> {
        items(s).iter().filter(|i| i.img != RECT).map(|i| (i.img, i.x, i.y)).collect()
    }

    fn rects(s: &Scene) -> std::vec::Vec<(i16, i16, i16, i16, u16)> {
        items(s).iter().filter(|i| i.img == RECT).map(|i| (i.x, i.y, i.w, i.h, i.color)).collect()
    }

    fn forecast(start: u8) -> [u8; WEATHER_LEN] {
        let mut d = [0u8; WEATHER_LEN];
        d[2] = start;
        d[0x0C] = 2;
        d[0x10..0x14].copy_from_slice(&20i32.to_le_bytes());
        for h in 0..24 {
            let o = 0x98 + h * 8;
            // the entry h is the hour start + 1 + h
            d[o..o + 4].copy_from_slice(&(h as i32 - 3).to_le_bytes());
            d[o + 4] = 3 + h as u8;
        }
        d[0x158] = b'A';
        d
    }

    const C24: Clock = Clock { hour: 8, twelve: false };

    #[test]
    fn forecast_follows_the_stock_card() {
        let d = forecast(8);
        // the hour of the forecast itself is the current weather, then the entries
        assert_eq!(hours(&d, 8, false), [Some((20, 2)), Some((-3, 3)), Some((-2, 4)), Some((-1, 5))]);
        assert_eq!(hours(&d, 10, false), [Some((-2, 4)), Some((-1, 5)), Some((0, 6)), Some((1, 7))]);
        // the stock card reads no further than the forecast's day, and nothing from a later start hour
        assert_eq!(hours(&d, 23, false)[3], Some((14, 20)));
        assert_eq!(hours(&forecast(0), 22, false), [Some((18, 24)), Some((19, 25)), None, None]);
        assert_eq!(hours(&d, 7, false), [None; 4]);
    }

    #[test]
    fn forecast_needs_a_city_or_the_phone() {
        let mut d = forecast(8);
        d[0x158] = 0;
        assert_eq!(hours(&d, 8, false), [None; 4]);
        assert_eq!(hours(&d, 8, true)[0], Some((20, 2)));
    }

    #[test]
    fn forecast_holds_bad_values() {
        let mut d = forecast(8);
        d[0x10..0x14].copy_from_slice(&0xFFi32.to_le_bytes());
        d[0x98 + 4] = 0x40;
        // 0xFF is "no value"; a code past the face's 27 icons would show a digit
        assert_eq!(hours(&d, 8, false)[..2], [None, Some((-3, 1))]);
    }

    #[test]
    fn weather_columns_24h() {
        let mut s = Scene::new();
        weather(&mut s, &forecast(8), C24, false, false);
        let got = images(&s);
        // "now", icon, two digits, degree
        assert_eq!(got[..5], [(0x27, 94, 51), (2, 104, 91), (0x1B + 2, 105, 148), (0x1B, 119, 148), (0x25, 133, 148)]);
        // 9:00, icon, minus, 3, degree
        assert_eq!(got[5..10], [(0x40 + 9, 181, 51), (3, 181, 91), (0x26, 182, 148), (0x1B + 3, 196, 148), (0x25, 210, 148)]);
        assert_eq!(got.len(), 20);
    }

    #[test]
    fn hour_labels_12h() {
        for h in 0..24u8 {
            let wide = matches!(h, 0 | 10 | 11 | 12 | 22 | 23);
            assert_eq!(label(h, true), (0x28 + h, if wide { 0 } else { 7 }));
        }
        assert_eq!(label(13, false), (0x40 + 13, 17));
    }

    #[test]
    fn weather_fahrenheit() {
        let mut s = Scene::new();
        weather(&mut s, &forecast(8), C24, true, false);
        // 20 C = 68 F
        assert_eq!((images(&s)[2].0, images(&s)[3].0), (0x1B + 6, 0x1B + 8));
    }

    #[test]
    fn weather_without_data() {
        let mut s = Scene::new();
        weather(&mut s, &[0; WEATHER_LEN], C24, false, false);
        // the 3 Pro centres "--" as one digit (the 0 it has instead of a temperature), seen in its frame
        assert_eq!(images(&s)[1..5], [(0, 104, 91), (0x26, 112, 148), (0x26, 126, 148), (0x25, 140, 148)]);
    }

    #[test]
    fn weather_extremes_fit() {
        for t in [i32::MIN, -1000, -5, 0, 999, 100_000, i32::MAX] {
            let mut d = forecast(8);
            d[0x10..0x14].copy_from_slice(&t.to_le_bytes());
            for f in [false, true] {
                let mut s = Scene::new();
                weather(&mut s, &d, C24, f, false);
                assert!(items(&s).iter().all(|i| i.img == RECT || i.img < IMAGES as u8), "{t}");
            }
        }
    }

    #[test]
    fn heart_bars_span_min_to_max() {
        let mut d = [0u8; HEART_LEN];
        (d[0x180], d[0x181]) = (150, 50);
        // hour 0: two half hours; hour 8 (now): one reading at the top
        d[0x120..0x124].copy_from_slice(&[100, 80, 120, 70]);
        d[0x120 + 32..0x120 + 36].copy_from_slice(&[150, 150, 0, 0]);
        let mut s = Scene::new();
        heart(&mut s, &d, C24);
        assert_eq!(images(&s), [(10, 80, 295)]);
        assert_eq!(rects(&s), [(90, 321, 8, 40, GREY), (186, 297, 8, 2, WHITE)]);
    }

    #[test]
    fn heart_single_value_and_none() {
        let mut d = [0u8; HEART_LEN];
        let mut s = Scene::new();
        heart(&mut s, &d, Clock { hour: 8, twelve: true });
        assert_eq!((images(&s), rects(&s).len()), (std::vec![(11, 80, 295)], 0));
        (d[0x180], d[0x181]) = (70, 70);
        d[0x120..0x124].copy_from_slice(&[70, 70, 0, 0]);
        s = Scene::new();
        heart(&mut s, &d, C24);
        assert_eq!(rects(&s), [(90, 297, 8, 5, GREY)]);
    }

    #[test]
    fn spo2_bars() {
        let mut d = [0u8; HOURLY_LEN];
        d[0x30..0x36].copy_from_slice(&[99, 91, 0, 0, 89, 85]);
        let mut s = Scene::new();
        spo2(&mut s, &d, C24);
        // under 90 is a stub on the axis
        assert_eq!(rects(&s), [(90, 297, 8, 46, GREY), (114, 371, 8, 5, GREY)]);
    }

    #[test]
    fn stress_bars() {
        let mut d = [0u8; HOURLY_LEN];
        d[0x31..0x35].copy_from_slice(&[100, 50, 1, 1]);
        let mut s = Scene::new();
        stress(&mut s, &d, C24);
        assert_eq!(rects(&s), [(90, 297, 8, 40, GREY), (102, 374, 8, 2, GREY)]);
    }

    #[test]
    fn sleep_stages_and_times() {
        let mut d = [0u8; SLEEP_LEN];
        d[0x5E] = 1;
        d[0x64..0x68].copy_from_slice(&(22 * 3600 + 30 * 60u32).to_le_bytes());
        d[0x68..0x6C].copy_from_slice(&(24 * 3600 + 30 * 60u32).to_le_bytes());
        d[0x6C] = 1;
        d[0x70..0x74].copy_from_slice(&3600u32.to_le_bytes());
        d[0x74] = 3;
        d[0x78..0x7C].copy_from_slice(&3600u32.to_le_bytes());
        d[0x1FC] = 2;
        let mut s = Scene::new();
        sleep(&mut s, &d, 3600);
        assert_eq!(rects(&s), [(91, 316, 140, 17, STAGES[1]), (233, 350, 140, 17, STAGES[3])]);
        let glyphs: std::vec::Vec<u8> = images(&s).iter().map(|i| i.0).collect();
        assert_eq!(glyphs, [2, 3, 3, 0, 0, 1, 3, 0]);
        assert_eq!((images(&s)[0].1, images(&s)[0].2, images(&s)[7].1), (91, 378, 361));
    }

    #[test]
    fn sleep_without_data_or_with_bad_data() {
        let mut s = Scene::new();
        sleep(&mut s, &[0; SLEEP_LEN], 0);
        assert_eq!((rects(&s).len(), images(&s).len()), (0, 8));
        assert!(images(&s).iter().all(|i| i.0 == 12));
        for count in [50u8, 51, 255] {
            let mut d = [0xFFu8; SLEEP_LEN];
            (d[0x1FC], d[0x1FD]) = (count, 0);
            let mut s = Scene::new();
            sleep(&mut s, &d, i32::MAX);
            assert!(s.len <= ITEMS);
        }
    }

    #[test]
    fn calories_percent() {
        let glyphs = |goal, burned| {
            let mut s = Scene::new();
            calories(&mut s, goal, burned);
            items(&s).iter().map(|i| (i.img, i.x, i.y, i.angle)).collect::<std::vec::Vec<_>>()
        };
        assert_eq!(glyphs(500, 0), [(0, 275, 332, 3350)]);
        assert_eq!(glyphs(500, 210), [(2, 275, 332, 3350), (4, 259, 338, 3440)]);
        let full = [(0, 275, 332, 3350), (0, 259, 338, 3440), (1, 242, 341, 3530)];
        assert_eq!(glyphs(500, 500), full);
        assert_eq!(glyphs(500, u32::MAX), full);
        // the 3 Pro shows 100 without a goal
        assert_eq!(glyphs(0, 7), full);
    }

    #[test]
    fn rings_under_a_touch() {
        // 14 rings 16 px apart, each takes 4 px more on both sides
        assert_eq!(ring(233, 233), None);
        assert_eq!(ring(233 + 17, 233), Some(0));
        assert_eq!(ring(233, 233 - 29), Some(0));
        assert_eq!(ring(233, 233 + 30), None);
        assert_eq!(ring(233 + 225, 233), Some(13));
        assert_eq!(ring(233 + 238, 233), None);
        assert_eq!(ring(i32::MAX, i32::MIN), None);
        assert_eq!(ring_band(0), (208, 50, 0xFCE1));
        assert_eq!(ring_band(13), (0, 466, 0x0C3F));
    }

    #[test]
    fn robot_clips() {
        // a clip plays once and holds its last frame; a tap starts the next one
        assert_eq!(next_frame(0, 0), 1);
        assert_eq!(next_frame(39, 0), 39);
        assert_eq!(next_frame(39, 1), 40);
        assert_eq!(next_frame(89, 1), 89);
        assert_eq!(next_frame(89, 2), 90);
        assert_eq!(next_frame(158, 2), 158);
        assert_eq!(next_frame(158, 0), 0);
        assert_eq!((next_clip(0), next_clip(1), next_clip(2), next_clip(9)), (1, 2, 0, 0));
    }

    #[test]
    fn digest_sees_every_field() {
        let mut a = Scene::new();
        a.image(1, 2, 3);
        let mut b = Scene::new();
        b.image(1, 2, 4);
        assert_ne!(a.digest(), b.digest());
        assert_ne!(Scene::new().digest(), 0);
    }
}
