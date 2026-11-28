// Timed input for headless runs: one command per line, `#` comments, times relative to the app start.

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Down(i16, i16),
    Up(i16, i16),
    Crown(i16),
    Button,
    Pause,
    Msg(Vec<u8>),
    Shot(String),
    /// the watch's GPS receiver: lat, lon (1e-7°), altitude (dm), fix quality, satellites
    Gps(i32, i32, i32, u8, u8),
}

const TAP_MS: u32 = 80;

/// `t X Y [MS]` touch down, up MS later (80) | `down X Y` | `up X Y` | `c N` N crown detents, sign = direction |
/// `b` button | `p` pause/resume toggle | `m TEXT` host message | `mx HEX` binary message | `wait MS` | `shot NAME` |
/// `gps LAT LON [ALT_M [FIX [SATS]]]` the GPS receiver's position from now (fix 1, 8 satellites; `gps none`: no fix).
/// Returns the timeline sorted by time; commands after a `t` do not wait for its release.
pub fn parse(src: &str) -> Result<Vec<(u32, Action)>, String> {
    let mut at = 0u32;
    let mut out = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (cmd, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        let args: Vec<&str> = rest.split_whitespace().collect();
        let err = || format!("script line {}: {line:?}", i + 1);
        let n = |k: usize| -> Result<i64, String> { args.get(k).and_then(|a| a.parse().ok()).ok_or_else(err) };
        let xy = || -> Result<(i16, i16), String> { Ok((n(0)? as i16, n(1)? as i16)) };
        match cmd {
            "t" => {
                let (x, y) = xy()?;
                let ms = if args.len() > 2 { n(2)? as u32 } else { TAP_MS };
                out.push((at, Action::Down(x, y)));
                out.push((at + ms, Action::Up(x, y)));
            }
            "down" | "up" => {
                let (x, y) = xy()?;
                out.push((at, if cmd == "up" { Action::Up(x, y) } else { Action::Down(x, y) }));
            }
            "c" => out.push((at, Action::Crown(n(0)? as i16))),
            "b" => out.push((at, Action::Button)),
            "p" => out.push((at, Action::Pause)),
            "m" if !rest.is_empty() => out.push((at, Action::Msg(rest.as_bytes().to_vec()))),
            "mx" => out.push((at, Action::Msg(crate::out::unhex(rest).ok_or_else(err)?))),
            "wait" => at += n(0)? as u32,
            "shot" if !rest.is_empty() => out.push((at, Action::Shot(rest.into()))),
            "gps" if rest == "none" => out.push((at, Action::Gps(0, 0, 0, 0, 0))),
            "gps" => {
                let f = |k: usize| -> Result<f64, String> { args.get(k).and_then(|a| a.parse().ok()).ok_or_else(err) };
                let e7 = |d: f64| (d * 1e7).round() as i32;
                let alt = if args.len() > 2 { (f(2)? * 10.0).round() as i32 } else { 0 };
                let fix = if args.len() > 3 { n(3)? as u8 } else { 1 };
                let sats = if args.len() > 4 { n(4)? as u8 } else { 8 };
                out.push((at, Action::Gps(e7(f(0)?), e7(f(1)?), alt, fix, sats)));
            }
            _ => return Err(err()),
        }
    }
    out.sort_by_key(|(t, _)| *t);
    Ok(out)
}
