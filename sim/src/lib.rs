//! Host simulator for cmf-watch-sdk apps (docs/app-guide.md). The app, built for the host with `--cfg wp2_sim`, calls
//! `wp2_sim_run` from its `main`; this crate fills in the ABI 1.1 `Api` table with the loader's semantics.
use wp2_core::events::{escape_due, ESCAPE_MS, HANG_MS};
use cmf_wp2_app::{Api, Event, Time, ABI, ABI_MINOR, BUTTON, CROWN, MESSAGE, PAUSE, RESUME, TOUCH_DOWN, TOUCH_UP};
use std::collections::VecDeque;
use std::io::{BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{fs, io, thread};
use wp2_core::chan::{parse_store, store_header, DEPTH, MSG_MAX, STORE_HEADER, STORE_MAX};
use wp2_core::gfx::{Canvas, H, W};
use wp2_core::pkg::crc32;
use wp2_core::pwm;

pub mod out;
pub mod script;
mod window;

pub use script::Action;
use out::V;

// wp2_core::events::Queue holds LEN - 1
const QUEUE_MAX: usize = 15;
// wp2-loader app.rs
const TEXT_MAX: usize = 64;
// the host's PING poll (wp2apps.channel), which collects the outbox
const POLL_MS: u32 = 250;
const TRIP_MS: u32 = pwm::CAP_MS - 20;
const E_RANGE: i32 = -1;
const E_IO: i32 = -5;
const E_AGAIN: i32 = -11;
const E_NOTCONN: i32 = -107;

pub const USAGE: &str = "usage: scripts/sim <app> [options]
  window (default): mouse = touch, wheel/arrows = crown, Esc/B = button, P = pause, M = message, stdin lines = messages
  --headless            no window, virtual clock (runs as fast as the app allows)
  --realtime            headless on the wall clock
  --do 'CMDS'           script, commands separated by ';'
  --script FILE         script, one command per line ('-' = stdin):
                          t X Y [MS] | down X Y | up X Y | c N | b | p | m TEXT | mx HEX | wait MS | shot NAME
  --until MS            end the run at MS (headless default: last scripted input + 2000)
  --frames DIR          PNGs: shot/at frames, last.png at the end
  --at MS,MS,...        also shoot at these times
  --every-show          a PNG on every show()
  --log FILE|-          JSON lines log (headless default: DIR/log.jsonl with --frames, else runs/sim/<app>/log.jsonl)
  --battery N[c|f]|none battery percent, c = charging, f = full (default 80)
  --epoch SECS          wall clock start (default: now; zone from TZ)
  --store DIR           storage directory (default runs/sim/<app>), --fresh deletes the stored blob first
  --tcp PORT            data channel on 127.0.0.1:PORT, hex lines both ways (send() is -107 without a client)
  --scale N             window scale 1, 2 or 4
  --wall MS             headless: give up after MS of wall time without an end (default 60000)";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum End {
    Exit(i32),
    Killed,
    Timeout,
    Idle,
    Closed,
    Hung,
}

impl End {
    fn name(self) -> &'static str {
        match self {
            End::Exit(_) => "exit",
            End::Killed => "killed",
            End::Timeout => "timeout",
            End::Idle => "idle",
            End::Closed => "closed",
            End::Hung => "hung",
        }
    }

    pub fn status(self) -> i32 {
        match self {
            End::Exit(0) | End::Timeout | End::Idle | End::Closed => 0,
            End::Exit(_) => 1,
            End::Killed | End::Hung => 2,
        }
    }
}

pub struct Config {
    pub app: String,
    pub headless: bool,
    pub virtual_clock: bool,
    pub timeline: Vec<(u32, Action)>,
    pub until: Option<u32>,
    pub frames: Option<PathBuf>,
    pub every_show: bool,
    pub log: Option<PathBuf>,
    pub battery: i32,
    pub epoch_ms: Option<i64>,
    pub store: PathBuf,
    pub fresh: bool,
    pub tcp: Option<u16>,
    pub scale: usize,
    pub stdin: bool,
    pub wall_ms: u64,
}

impl Config {
    pub fn new(app: &str) -> Config {
        let root = std::env::var_os("WP2_SIM_ROOT").map_or_else(|| PathBuf::from("."), PathBuf::from);
        Config {
            app: app.into(),
            headless: true,
            virtual_clock: true,
            timeline: Vec::new(),
            until: None,
            frames: None,
            every_show: false,
            log: None,
            battery: 80,
            epoch_ms: None,
            store: root.join("runs/sim").join(app),
            fresh: false,
            tcp: None,
            scale: 1,
            stdin: false,
            wall_ms: 60_000,
        }
    }

    pub fn from_args(app: &str, args: impl IntoIterator<Item = String>) -> Result<Config, String> {
        let mut c = Config::new(app);
        let mut headless = false;
        let mut realtime = false;
        let mut script = String::new();
        let mut at = Vec::new();
        let mut store = None;
        let mut args = args.into_iter();
        while let Some(a) = args.next() {
            let mut val = || args.next().ok_or(format!("{a} needs a value"));
            let num = |s: String| s.parse::<u64>().map_err(|_| format!("bad number {s}"));
            match a.as_str() {
                "-h" | "--help" => return Err(String::new()),
                "--headless" => headless = true,
                "--realtime" => realtime = true,
                "--do" => script += &(val()?.replace(';', "\n") + "\n"),
                "--script" => {
                    let path = val()?;
                    let mut s = String::new();
                    let r = if path == "-" {
                        io::stdin().read_to_string(&mut s)
                    } else {
                        fs::File::open(&path).and_then(|mut f| f.read_to_string(&mut s))
                    };
                    r.map_err(|e| format!("{path}: {e}"))?;
                    script += &(s + "\n");
                }
                "--until" => c.until = Some(num(val()?)? as u32),
                "--frames" => c.frames = Some(val()?.into()),
                "--at" => {
                    for t in val()?.split(',') {
                        at.push(num(t.trim().into())? as u32);
                    }
                }
                "--every-show" => c.every_show = true,
                "--log" => c.log = Some(val()?.into()),
                "--battery" => c.battery = battery(&val()?)?,
                "--epoch" => c.epoch_ms = Some(num(val()?)? as i64 * 1000),
                "--store" => store = Some(PathBuf::from(val()?)),
                "--fresh" => c.fresh = true,
                "--tcp" => c.tcp = Some(num(val()?)? as u16),
                "--scale" => c.scale = num(val()?)? as usize,
                "--wall" => c.wall_ms = num(val()?)?,
                _ => return Err(format!("unknown option {a}")),
            }
        }
        c.timeline = script::parse(&script)?;
        for t in at {
            c.timeline.push((t, Action::Shot(format!("at-{t}"))));
        }
        c.timeline.sort_by_key(|(t, _)| *t);
        c.headless = headless || realtime;
        c.virtual_clock = c.headless && !realtime;
        c.stdin = !c.headless;
        if c.headless && c.until.is_none() {
            c.until = Some(c.timeline.last().map_or(0, |(t, _)| *t) + 2000);
        }
        if let Some(s) = store {
            c.store = s;
        }
        if c.headless && c.log.is_none() {
            c.log = Some(c.frames.clone().unwrap_or_else(|| c.store.clone()).join("log.jsonl"));
        }
        Ok(c)
    }
}

fn battery(s: &str) -> Result<i32, String> {
    if s == "none" {
        return Ok(E_IO);
    }
    let (n, flag) = match s.strip_suffix(['c', 'f']) {
        Some(n) => (n, s.as_bytes()[s.len() - 1]),
        None => (s, 0),
    };
    let p = n.parse::<i32>().map_err(|_| format!("bad battery {s}"))?.clamp(0, 100);
    Ok(match flag {
        b'c' => p | 1 << 8 | 1 << 10,
        b'f' => p | 1 << 9 | 1 << 10,
        _ => p,
    })
}

pub struct State {
    pub cfg: Config,
    start: Instant,
    base_ms: i64,
    us: u64,
    pub fb: Vec<u16>,
    pub shown: Vec<u16>,
    pub shows: u32,
    first_show: Option<(u32, f64)>,
    queue: VecDeque<Event>,
    inbox: VecDeque<Vec<u8>>,
    announced: usize,
    host: VecDeque<Vec<u8>>,
    outbox: VecDeque<Vec<u8>>,
    polled: u32,
    pub paused: bool,
    since: u32,
    locked: bool,
    pub motor: Option<(u16, u16)>,
    pub last_vibrate: Option<(u16, u16, i32)>,
    pub keep_on: bool,
    button_at: Option<u32>,
    // ABI 1.3: the app owns BUTTON; consumed = it popped the pending one
    owned: bool,
    consumed: bool,
    // now() of the app's last next_event call: a busy owner is not hung (escape_due)
    event_at: u32,
    timeline: VecDeque<(u32, Action)>,
    pub end: Option<End>,
    pub sent: VecDeque<String>,
    log: out::Log,
    tcp: Option<TcpStream>,
    /// the app holds the GPS (SENSOR record 2): a GPS event every GPS_MS from `gps_at`
    gps_on: bool,
    gps_at: u32,
    /// the receiver's latest: lat_e7, lon_e7, alt_dm, fix, sats (script `gps`)
    gps: (i32, i32, i32, u8, u8),
}

/// The loader's GPS event period (wp2_core::gps::EMIT_MS).
const GPS_MS: u32 = 1000;

impl State {
    pub fn new(cfg: Config) -> State {
        let log = out::Log::open(cfg.log.as_deref());
        let now = || SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
        let base_ms = cfg.epoch_ms.unwrap_or_else(now);
        State {
            timeline: cfg.timeline.clone().into(),
            cfg,
            start: Instant::now(),
            base_ms,
            us: 0,
            fb: vec![0; W * H],
            shown: vec![0; W * H],
            shows: 0,
            first_show: None,
            queue: VecDeque::new(),
            inbox: VecDeque::new(),
            announced: 0,
            host: VecDeque::new(),
            outbox: VecDeque::new(),
            polled: 0,
            paused: false,
            since: 0,
            locked: false,
            motor: None,
            last_vibrate: None,
            keep_on: false,
            button_at: None,
            owned: false,
            consumed: false,
            event_at: 0,
            end: None,
            sent: VecDeque::new(),
            log,
            tcp: None,
            gps_on: false,
            gps_at: 0,
            gps: (0, 0, 0, 0, 0),
        }
    }

    pub fn now(&self) -> u32 {
        if self.cfg.virtual_clock { (self.us / 1000) as u32 } else { self.start.elapsed().as_millis() as u32 }
    }

    fn wall_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    pub fn log(&mut self, ev: &str, fields: &[(&str, V)]) {
        let t = self.now();
        self.log.rec(t, ev, fields);
    }

    /// Runs what is due at `now`: scripted input, host messages into the inbox, the outbox poll, the 2 s motor cap,
    /// the 1 s button escape and the run limit.
    pub fn pump(&mut self) {
        let now = self.now();
        while self.timeline.front().is_some_and(|(t, _)| *t <= now) {
            let (_, a) = self.timeline.pop_front().unwrap();
            self.act(a);
        }
        while !self.paused && self.inbox.len() < DEPTH as usize {
            let Some(m) = self.host.pop_front() else { break };
            self.log("msg", &[("len", V::I(m.len() as i64)), out::data(&m)]);
            self.inbox.push_back(m);
        }
        if now >= self.polled + POLL_MS {
            self.polled = now - now % POLL_MS;
            while let Some(m) = self.outbox.pop_front() {
                self.deliver(&m);
            }
        }
        if self.gps_on && !self.paused && now >= self.gps_at {
            self.gps_at = now + GPS_MS;
            let (.., fix, sats) = self.gps;
            if self.queue.len() < QUEUE_MAX {
                self.queue.push_back(Event { kind: cmf_wp2_app::GPS, x: fix.into(), y: sats.into(), ..Event::default() });
            }
        }
        if self.since != 0 && pwm::over_cap(now, self.since) {
            self.since = 0;
            self.motor = None;
            self.locked = true;
            self.log("motor_cut", &[("reason", V::S("cap"))]);
        }
        if self.button_at.is_some_and(|b| escape_due(self.owned, b.max(1), self.consumed, self.event_at, now)) {
            self.finish(End::Killed);
        }
        if self.cfg.until.is_some_and(|u| now >= u) {
            self.finish(End::Timeout);
        }
    }

    // next time pump() has something to do
    fn due(&self) -> Option<u32> {
        [
            self.timeline.front().map(|(t, _)| *t),
            (self.since != 0).then(|| self.since + TRIP_MS),
            self.button_at
                .filter(|_| !(self.owned && self.consumed))
                .map(|b| if self.owned { b.max(self.event_at) + HANG_MS } else { b + ESCAPE_MS }),
            (!self.outbox.is_empty()).then(|| self.polled + POLL_MS),
            (self.gps_on && !self.paused).then_some(self.gps_at),
            self.cfg.until,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub fn act(&mut self, a: Action) {
        let ev = |kind, x, y, delta| Event { kind, x, y, delta, ..Event::default() };
        match a {
            Action::Down(x, y) => self.input(ev(TOUCH_DOWN, x, y, 0)),
            Action::Up(x, y) => self.input(ev(TOUCH_UP, x, y, 0)),
            Action::Crown(n) => {
                for _ in 0..n.unsigned_abs() {
                    self.input(ev(CROWN, 0, 0, n.signum()));
                }
            }
            Action::Button => {
                if !self.paused && (self.button_at.is_none() || self.owned && self.consumed) {
                    self.button_at = Some(self.now());
                    self.consumed = false;
                }
                self.input(ev(BUTTON, 0, 0, 0));
            }
            Action::Pause => self.set_paused(!self.paused),
            Action::Msg(m) => {
                if (1..=MSG_MAX).contains(&m.len()) {
                    self.host.push_back(m);
                } else {
                    self.log("host_msg_refused", &[("len", V::I(m.len() as i64))]);
                }
            }
            Action::Shot(name) => self.shot(&name),
            Action::Gps(lat, lon, alt, fix, sats) => {
                self.gps = (lat, lon, alt, fix, sats);
                self.log("gps", &[("fix", V::I(fix.into())), ("sats", V::I(sats.into()))]);
            }
        }
    }

    // the loader drops input while paused; its queue keeps 15
    fn input(&mut self, e: Event) {
        let dropped = self.paused || self.queue.len() >= QUEUE_MAX;
        if !dropped {
            self.queue.push_back(e);
        }
        let kind = ["", "touch_down", "touch_up", "crown", "button"][usize::from(e.kind).min(4)];
        let mut f = vec![("kind", V::S(kind))];
        match e.kind {
            TOUCH_DOWN | TOUCH_UP => f.extend([("x", V::I(e.x.into())), ("y", V::I(e.y.into()))]),
            CROWN => f.push(("delta", V::I(e.delta.into()))),
            _ => {}
        }
        if dropped {
            f.push(("dropped", V::B(true)));
        }
        self.log("input", &f);
    }

    // a stock popup: PAUSE replaces the queue, the motor is cut, show() is held until RESUME repaints
    pub fn set_paused(&mut self, on: bool) {
        if on == self.paused {
            return;
        }
        self.paused = on;
        if on {
            self.queue.clear();
            self.queue.push_back(Event { kind: PAUSE, ..Event::default() });
            if self.since != 0 {
                self.since = 0;
                self.motor = None;
                self.log("motor_cut", &[("reason", V::S("pause"))]);
            }
        } else {
            self.queue.push_back(Event { kind: RESUME, ..Event::default() });
            self.shown.copy_from_slice(&self.fb);
        }
        self.log(if on { "pause" } else { "resume" }, &[]);
    }

    fn pop(&mut self) -> Option<Event> {
        if let Some(e) = self.queue.pop_front() {
            self.consumed |= e.kind == BUTTON;
            return Some(e);
        }
        let m = self.inbox.get(self.announced)?;
        self.announced += 1;
        Some(Event { kind: MESSAGE, x: m.len() as i16, ..Event::default() })
    }

    fn deliver(&mut self, m: &[u8]) {
        let shown = out::shown(m);
        println!("send {shown}");
        if let Some(s) = self.tcp.as_mut() {
            if writeln!(s, "{}", out::hex(m)).is_err() {
                self.tcp = None;
            }
        }
        self.log("send", &[("len", V::I(m.len() as i64)), out::data(m)]);
        self.sent.push_back(shown);
        if self.sent.len() > 3 {
            self.sent.pop_front();
        }
    }

    pub fn finish(&mut self, end: End) {
        if self.end.is_some() {
            return;
        }
        self.end = Some(end);
        self.since = 0;
        self.motor = None;
        self.keep_on = false;
        let (ff, ffw) = self.first_show.map_or((-1, -1.0), |(t, w)| (t as i64, w));
        let wall = self.wall_ms();
        self.log("end", &[
            ("reason", V::S(end.name())),
            ("code", V::I(if let End::Exit(c) = end { c.into() } else { 0 })),
            ("shows", V::I(self.shows.into())),
            ("first_show_ms", V::I(ff)),
            ("first_show_wall_ms", V::F(ffw)),
            ("wall_ms", V::F(wall)),
        ]);
    }

    /// The panel as the watch shows it: the app's frame with the loader's status bar over it (fw status.rs: the
    /// GPS bars while the app holds the GPS, the time; the stock 24 px font there, the SDK's here).
    pub fn panel(&self) -> Vec<u16> {
        struct Px<'a>(&'a mut [u16]);
        impl cmf_wp2_ui::Surface for Px<'_> {
            fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
                for yy in y.max(0)..(y + h).min(H as i32) {
                    for xx in x.max(0)..(x + w).min(W as i32) {
                        self.0[yy as usize * W + xx as usize] = c;
                    }
                }
            }
            fn text(&mut self, _: i32, _: i32, _: &str, _: u16) {}
        }
        let mut px = self.shown.clone();
        let t = self.time().map_or("--:--".to_string(), |t| format!("{}:{:02}", if t.hour % 12 == 0 { 12 } else { t.hour % 12 }, t.minute));
        let bars = self.gps_on.then(|| match (self.gps.3, self.gps.4) {
            (0, _) => 0,
            (_, 8..) => 3,
            (_, 5..) => 2,
            _ => 1,
        });
        let mut s = Px(&mut px);
        let tw = cmf_wp2_ui::font::width(&t);
        // fw status.rs: bars 3 × 3 px, 3 px apart, then the time in a 72 px box, centred; top 4
        let (lw, bw) = (72, 15);
        let x0 = 233 - if bars.is_some() { (bw + lw) / 2 } else { lw / 2 };
        if let Some(l) = bars {
            for i in 0..3 {
                let c = if i < l { 0xFFFF } else { 0x4208 };
                cmf_wp2_ui::Surface::rect(&mut s, x0 + i * 6, 9, 3, 17, c);
            }
        }
        let lx = if bars.is_some() { x0 + bw } else { x0 };
        cmf_wp2_ui::font::draw(&mut s, lx + (lw - tw) / 2, 5, &t, 0xFFFF);
        px
    }

    pub fn shot(&mut self, name: &str) {
        let Some(dir) = self.cfg.frames.clone() else { return };
        let path = dir.join(format!("{name}.png"));
        let rc = out::png(&path, &self.panel());
        self.log("shot", &[("file", V::S(&path.to_string_lossy())), ("ok", V::B(rc.is_ok()))]);
    }

    fn store(&self) -> (PathBuf, PathBuf) {
        (self.cfg.store.join("data.dat"), self.cfg.store.join("data.new"))
    }

    // ---- the Api table, minus blocking and parking (Sim) ----

    pub fn fill_rect(&mut self, x: i16, y: i16, w: u16, h: u16, c: u16) {
        Canvas { px: &mut self.fb }.fill_rect(x, y, w, h, c);
    }

    pub fn draw_text(&mut self, x: i16, y: i16, s: &[u8], c: u16) {
        Canvas { px: &mut self.fb }.text(x, y, &s[..s.len().min(TEXT_MAX)], c);
    }

    pub fn present(&mut self) {
        self.shows += 1;
        if self.first_show.is_none() {
            self.first_show = Some((self.now(), self.wall_ms()));
            self.log("first_show", &[("wall_ms", V::F(self.wall_ms()))]);
        }
        if !self.paused {
            self.shown.copy_from_slice(&self.fb);
            if self.cfg.every_show {
                let name = format!("show-{:05}-{}ms", self.shows, self.now());
                self.shot(&name);
            }
        }
        self.pump();
    }

    pub fn motor(&mut self, duty: u16, hz: u16) -> i32 {
        self.pump();
        let rc = match pwm::decide(duty, hz, self.paused, self.locked) {
            Err(e) => e,
            Ok(None) => {
                self.since = 0;
                self.locked = false;
                self.motor = None;
                0
            }
            Ok(Some(_)) => {
                if self.since == 0 {
                    self.since = self.now() | 1;
                }
                self.motor = Some((duty, hz));
                0
            }
        };
        self.last_vibrate = Some((duty, hz, rc));
        self.log("vibrate", &[("duty", V::I(duty.into())), ("hz", V::I(hz.into())), ("rc", V::I(rc.into()))]);
        rc
    }

    pub fn millis(&mut self) -> u32 {
        // ponytail: a virtual µs per call, so apps that poll now() make progress; real cost of a call is not modelled
        if self.cfg.virtual_clock {
            self.us += 1;
        }
        self.pump();
        self.now()
    }

    pub fn send(&mut self, data: &[u8]) -> i32 {
        if !(1..=MSG_MAX).contains(&data.len()) {
            return E_RANGE;
        }
        if self.cfg.tcp.is_some() && self.tcp.is_none() {
            return E_NOTCONN;
        }
        if self.outbox.len() >= DEPTH as usize {
            return E_AGAIN;
        }
        self.outbox.push_back(data.to_vec());
        0
    }

    pub fn recv(&mut self, buf: &mut [u8]) -> i32 {
        let Some(m) = self.inbox.front() else { return 0 };
        if buf.len() < m.len() {
            return E_RANGE;
        }
        let n = m.len();
        buf[..n].copy_from_slice(m);
        self.inbox.pop_front();
        self.announced = self.announced.saturating_sub(1);
        self.pump();
        n as i32
    }

    /// Atomic like the loader's: a complete `data.new`, then rename over `data.dat`.
    pub fn save(&mut self, data: &[u8]) -> i32 {
        if data.len() > STORE_MAX {
            return E_RANGE;
        }
        let (dat, new) = self.store();
        let r = (|| {
            let _ = fs::remove_file(&new);
            if data.is_empty() {
                let gone = |e: io::Error| if e.kind() == io::ErrorKind::NotFound { Ok(()) } else { Err(e) };
                return fs::remove_file(&dat).or_else(gone);
            }
            fs::create_dir_all(&self.cfg.store)?;
            let mut f = fs::File::create(&new)?;
            f.write_all(&store_header(data))?;
            f.write_all(data)?;
            f.sync_all()?;
            fs::rename(&new, &dat)
        })();
        let rc = if r.is_ok() { 0 } else { E_IO };
        self.log("save", &[("len", V::I(data.len() as i64)), ("rc", V::I(rc.into()))]);
        rc
    }

    pub fn load(&mut self, buf: &mut [u8]) -> i32 {
        let (dat, new) = self.store();
        let mut rc = 0;
        for path in [dat, new] {
            let Ok(raw) = fs::read(&path) else { continue };
            let blob = raw.first_chunk::<STORE_HEADER>().and_then(parse_store).and_then(|(len, crc)| {
                let data = raw.get(STORE_HEADER..STORE_HEADER + len)?;
                (crc32(0, data) == crc).then_some(data)
            });
            match blob {
                Some(d) if d.len() > buf.len() => {
                    rc = E_RANGE;
                    break;
                }
                Some(d) => {
                    buf[..d.len()].copy_from_slice(d);
                    rc = d.len() as i32;
                    break;
                }
                None => rc = E_IO,
            }
        }
        self.log("load", &[("rc", V::I(rc.into()))]);
        rc
    }

    pub fn time(&self) -> Option<Time> {
        let ms = self.base_ms + i64::from(self.now());
        out::local(ms)
    }

    pub fn own_back(&mut self, on: bool) {
        self.owned = on;
    }

    pub fn screen_on(&mut self, on: bool) -> i32 {
        if on != self.keep_on {
            self.keep_on = on;
            self.log("keep_screen_on", &[("on", V::B(on))]);
        }
        0
    }
}

pub struct Sim {
    st: Mutex<State>,
    cv: Condvar,
}

impl Sim {
    pub fn new(cfg: Config) -> Sim {
        Sim { st: Mutex::new(State::new(cfg)), cv: Condvar::new() }
    }

    pub fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn notify(&self) {
        self.cv.notify_all();
    }

    /// `event(timeout)`: the virtual clock jumps to the next due time instead of sleeping.
    pub fn next_event(&self, timeout: u32) -> Option<Event> {
        let mut st = self.lock();
        let deadline = (timeout != u32::MAX).then(|| st.now().saturating_add(timeout));
        loop {
            st.pump();
            let now = st.now();
            st.event_at = now;
            if st.end.is_some() {
                return None;
            }
            if let Some(e) = st.pop() {
                return Some(e);
            }
            if deadline.is_some_and(|d| now >= d) {
                return None;
            }
            let next = deadline.into_iter().chain(st.due()).min();
            if st.cfg.virtual_clock {
                match next {
                    Some(t) => st.us = u64::from(t.max(now + 1)) * 1000,
                    None => {
                        st.finish(End::Idle);
                        self.notify();
                    }
                }
            } else {
                let wait = next.map_or(10, |t| t.saturating_sub(now).clamp(1, 10));
                st = self.cv.wait_timeout(st, Duration::from_millis(wait.into())).unwrap_or_else(|e| e.into_inner()).0;
            }
        }
    }

    /// Headless: blocks until the run ends; on the wall clock it also runs the timeline.
    pub fn wait_end(&self) -> End {
        let mut st = self.lock();
        let mut last = (Instant::now(), st.now());
        loop {
            if let Some(e) = st.end {
                return e;
            }
            if !st.cfg.virtual_clock {
                st.pump();
            }
            let now = st.now();
            if now != last.1 {
                last = (Instant::now(), now);
            } else if last.0.elapsed() > Duration::from_millis(st.cfg.wall_ms) {
                st.finish(End::Hung);
            }
            st = self.cv.wait_timeout(st, Duration::from_millis(5)).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
}

static SIM: OnceLock<Sim> = OnceLock::new();

fn sim() -> &'static Sim {
    SIM.get().expect("wp2-sim not started")
}

// the app thread stops here once the run ended (the loader aborts it)
fn call<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let s = sim();
    let mut st = s.lock();
    let r = f(&mut st);
    let ended = st.end.is_some();
    drop(st);
    s.notify();
    if ended {
        park();
    }
    r
}

fn park() -> ! {
    loop {
        thread::park();
    }
}

fn bytes<'a>(p: *const u8, len: usize) -> &'a [u8] {
    if p.is_null() || len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(p, len) } }
}

fn bytes_mut<'a>(p: *mut u8, len: usize) -> &'a mut [u8] {
    if p.is_null() || len == 0 { &mut [] } else { unsafe { std::slice::from_raw_parts_mut(p, len) } }
}

extern "C" fn fill_rect(x: i16, y: i16, w: u16, h: u16, c: u16) {
    sim().lock().fill_rect(x, y, w, h, c)
}

extern "C" fn draw_text(x: i16, y: i16, text: *const u8, len: usize, c: u16) {
    if !text.is_null() {
        sim().lock().draw_text(x, y, bytes(text, len), c)
    }
}

extern "C" fn present() {
    call(|s| s.present())
}

extern "C" fn next_event(ev: *mut Event, timeout: u32) -> bool {
    let e = sim().next_event(timeout);
    call(|_| ());
    match e {
        Some(e) if !ev.is_null() => {
            unsafe { ev.write(e) };
            true
        }
        e => e.is_some(),
    }
}

extern "C" fn motor(duty: u16, hz: u16) -> i32 {
    call(|s| s.motor(duty, hz))
}

extern "C" fn millis() -> u32 {
    call(|s| s.millis())
}

extern "C" fn exit(code: i32) -> ! {
    call(|s| s.finish(End::Exit(code)));
    park()
}

extern "C" fn send_msg(data: *const u8, len: usize) -> i32 {
    if data.is_null() {
        return E_RANGE;
    }
    call(|s| s.send(bytes(data, len)))
}

extern "C" fn recv_msg(buf: *mut u8, cap: usize) -> i32 {
    if buf.is_null() {
        return call(|s| if s.inbox.is_empty() { 0 } else { E_RANGE });
    }
    call(|s| s.recv(bytes_mut(buf, cap)))
}

extern "C" fn load_data(buf: *mut u8, cap: usize) -> i32 {
    call(|s| s.load(bytes_mut(buf, cap)))
}

extern "C" fn save_data(data: *const u8, len: usize) -> i32 {
    if data.is_null() && len > 0 {
        return E_RANGE;
    }
    call(|s| s.save(bytes(data, len)))
}

extern "C" fn get_time(out: *mut Time) -> i32 {
    match (out.is_null(), call(|s| s.time())) {
        (false, Some(t)) => {
            unsafe { out.write(t) };
            0
        }
        _ => E_IO,
    }
}

extern "C" fn get_battery() -> i32 {
    call(|s| s.cfg.battery)
}

extern "C" fn screen_on(on: bool) -> i32 {
    call(|s| s.screen_on(on))
}

extern "C" fn own_back(on: bool) {
    call(|s| s.own_back(on))
}

/// The speaker and mic. The speaker takes everything at once and keeps it in the store's `played.raw` (16-bit
/// mono at the opened rate); the mic plays `runs/sim/mic.raw` (16 kHz mono 16-bit, looped; a 440 Hz tone without
/// it) at the clock's pace, at most 256 ms kept, as the loader's.
static MIC: Mutex<Option<(u32, u64, Vec<i16>)>> = Mutex::new(None);

fn audio(cmd: wp2_core::lvcmd::AudioCmd) -> i32 {
    use wp2_core::lvcmd::AudioCmd as A;
    match cmd {
        A::Open { .. } => {
            let _ = fs::remove_file(sim().lock().cfg.store.join("played.raw"));
            0
        }
        A::Close | A::Volume { .. } | A::State => 0,
        // a 32-bit pointer means nothing here
        A::Write { .. } | A::MicRead { .. } => -22,
        A::MicOpen => {
            let root = std::env::var("WP2_SIM_ROOT").unwrap_or_default();
            let src = fs::read(std::path::Path::new(&root).join("runs/sim/mic.raw"))
                .map(|d| d.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect::<Vec<_>>())
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| (0..16000).map(|i| ((i as f32 * 440.0 * std::f32::consts::TAU / 16000.0).sin() * 8000.0) as i16).collect());
            let now = sim().lock().now();
            *MIC.lock().unwrap() = Some((now, 0, src));
            0
        }
        A::MicClose => {
            *MIC.lock().unwrap() = None;
            0
        }
    }
}

/// AUDIO write (op 2) and MIC read (op 7) with the app's host pointer.
fn audio_host(op: u8, ptr: usize, len: usize) -> i32 {
    if op == 2 {
        let data = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) }.to_vec();
        let p = sim().lock().cfg.store.join("played.raw");
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = f.write_all(&data);
        }
        return len as i32;
    }
    let now = sim().lock().now();
    let mut m = MIC.lock().unwrap();
    let Some((t0, taken, src)) = m.as_mut() else { return 0 };
    let made = u64::from(now.wrapping_sub(*t0)) * 16;
    // the loader keeps 4096 samples: older ones are gone
    *taken = (*taken).max(made.saturating_sub(4096));
    let n = ((made - *taken) as usize).min(len / 2);
    let out = unsafe { std::slice::from_raw_parts_mut(ptr as *mut u8, n * 2) };
    for i in 0..n {
        let v = src[((*taken as usize) + i) % src.len()];
        out[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
    }
    *taken += n as u64;
    (n * 2) as i32
}

/// The storage service's scan of the sim: `files/` as `/NAND:/wp2apps/0_<name>`, collected whole at once.
fn storage(rec: &mut [u8]) -> i32 {
    static SCAN: Mutex<Vec<(String, u32)>> = Mutex::new(Vec::new());
    let (op, index) = (rec[4], usize::from(u16::from_le_bytes([rec[5], rec[6]])));
    let out = &mut rec[8..];
    out.fill(0);
    match op {
        0 => {
            out[0] = 1;
            out[2] = 16;
            out[4] = 96;
            out[6] = 16;
            out[8] = 96;
        }
        5 => {
            let dir = sim().lock().cfg.store.join("files");
            let mut v: Vec<(String, u32)> = fs::read_dir(dir)
                .map(|d| d.flatten().map(|e| (format!("/NAND:/wp2apps/0_{}", e.file_name().to_string_lossy()), e.metadata().map(|m| m.len() as u32).unwrap_or(0))).collect())
                .unwrap_or_default();
            v.sort();
            *SCAN.lock().unwrap() = v;
        }
        6 => {
            let n = SCAN.lock().unwrap().len();
            out[..2].copy_from_slice(&(n.min(96) as u16).to_le_bytes());
            out[2] = 1;
            out[3] = u8::from(n > 96);
        }
        7 => SCAN.lock().unwrap().clear(),
        8 => {
            let v = SCAN.lock().unwrap();
            let Some((p, size)) = v.get(index) else { return -22 };
            out[..4].copy_from_slice(&size.to_le_bytes());
            let b = p.as_bytes();
            out[4..4 + b.len().min(95)].copy_from_slice(&b[..b.len().min(95)]);
        }
        1..=4 => return -95,
        _ => return -22,
    }
    0
}

/// ABI 1.2: LVGL apps are not simulated (a later plan); FILE records (offline data) are: runs/sim/<app>/files/.
extern "C" fn lv_submit(buf: *const u8, len: usize) -> i32 {
    use wp2_core::lvcmd::{file, FileCmd};
    if buf.is_null() || len == 0 {
        return -95; // an empty batch: nothing LVGL to run
    }
    let rec = unsafe { std::slice::from_raw_parts_mut(buf as *mut u8, len) };
    // storage inspection v1 (docs/storage-inspection.md), enough for a scan: the app's files as slot 0's
    if len == 132 && rec[..4] == [0x19, 128, 0, 0] {
        return storage(rec);
    }
    // GPS (loader of 2026-11-19): SENSOR record 2 on/off, GPS_FIX record 0x18 the latest fix
    if let [0x13, 2, 0, 0, 2, on @ (0 | 1)] = *rec {
        let mut st = sim().lock();
        st.gps_on = on == 1;
        st.gps_at = st.now();
        st.log("gps_hold", &[("on", V::B(on == 1))]);
        return 0;
    }
    if wp2_core::lvcmd::gps_fix(rec) {
        let st = sim().lock();
        if !st.gps_on {
            return -1;
        }
        let (lat, lon, alt, fix, sats) = st.gps;
        let p = &mut rec[4..];
        p[0..4].copy_from_slice(&lat.to_le_bytes());
        p[4..8].copy_from_slice(&lon.to_le_bytes());
        p[8..12].copy_from_slice(&alt.to_le_bytes());
        p[12..16].copy_from_slice(&st.now().to_le_bytes());
        p[16] = fix;
        p[17] = sats;
        p[18..20].copy_from_slice(&(if fix > 0 { 30u16 } else { 0 }).to_le_bytes());
        return 0;
    }
    // AUDIO write / MIC read with a host pointer (cmf_wp2_app pcm_record)
    if let [0x18, 13, 0, 0, op @ (2 | 7), ref rest @ ..] = *rec {
        if rest.len() == 12 {
            let ptr = u64::from_le_bytes(rest[..8].try_into().unwrap());
            let len = u32::from_le_bytes(rest[8..].try_into().unwrap());
            return audio_host(op, ptr as usize, len as usize);
        }
    }
    if let Some(cmd) = wp2_core::lvcmd::audio(rec) {
        return audio(cmd);
    }
    let Some(cmd) = file(unsafe { std::slice::from_raw_parts(buf, len) }) else { return -95 };
    let dir = sim().lock().cfg.store.join("files");
    let _ = fs::create_dir_all(&dir);
    let name = |n: &[u8]| dir.join(String::from_utf8_lossy(n).into_owned());
    match cmd {
        FileCmd::Read { name: n, skip } => match fs::read(name(n)) {
            Ok(d) => {
                let from = (skip as usize).min(d.len());
                let k = (d.len() - from).min(rec.len());
                rec[..k].copy_from_slice(&d[from..from + k]);
                k as i32
            }
            Err(_) => -2,
        },
        FileCmd::Write { name: n, data, append } => {
            let data = data.to_vec();
            let mut f = match fs::OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(name(n)) {
                Ok(f) => f,
                Err(_) => return E_IO,
            };
            if f.write_all(&data).is_ok() { data.len() as i32 } else { E_IO }
        }
        FileCmd::Delete { name: n } => {
            if fs::remove_file(name(n)).is_ok() { 0 } else { -2 }
        }
        // the watch's own emoji font, if one was put at runs/sim/emoji28.font (a vendor file: never committed)
        FileCmd::Emoji { skip } => {
            let root = std::env::var("WP2_SIM_ROOT").unwrap_or_default();
            match fs::read(std::path::Path::new(&root).join("runs/sim/emoji28.font")) {
                Ok(d) => {
                    let from = (skip as usize).min(d.len());
                    let k = (d.len() - from).min(rec.len());
                    rec[..k].copy_from_slice(&d[from..from + k]);
                    k as i32
                }
                Err(_) => -2,
            }
        }
    }
}

static API: Api = Api {
    abi: ABI,
    abi_minor: ABI_MINOR,
    width: W as u16,
    height: H as u16,
    fill_rect,
    draw_text,
    present,
    next_event,
    motor,
    millis,
    exit,
    send_msg,
    recv_msg,
    load_data,
    save_data,
    get_time,
    get_battery,
    screen_on,
    lv_submit,
    own_back,
};

fn app_name() -> String {
    std::env::args()
        .next()
        .and_then(|a| PathBuf::from(a).file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "app".into())
}

fn stdin_messages() {
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if !line.is_empty() {
            let s = sim();
            s.lock().act(Action::Msg(line.into_bytes()));
            s.notify();
        }
    }
}

fn tcp_channel(listener: TcpListener) {
    for conn in listener.incoming() {
        let Ok(conn) = conn else { continue };
        let Ok(reader) = conn.try_clone() else { continue };
        sim().lock().tcp = Some(conn);
        thread::spawn(move || {
            for line in io::BufReader::new(reader).lines() {
                let Ok(line) = line else { break };
                let s = sim();
                let mut st = s.lock();
                match out::unhex(line.trim()) {
                    Some(m) => st.act(Action::Msg(m)),
                    None => st.log("tcp_bad_line", &[("text", V::S(&line))]),
                }
                drop(st);
                s.notify();
            }
        });
    }
}

/// Called by the app's `main` (cmf_wp2_app::__sim_run): runs `start` on the app thread, the window or the
/// headless wait on this one, and exits the process when the run ends.
#[unsafe(no_mangle)]
pub extern "C" fn wp2_sim_run(start: extern "C" fn(&'static Api) -> i32) -> i32 {
    let app = app_name();
    let cfg = match Config::from_args(&app, std::env::args().skip(1)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}{USAGE}", if e.is_empty() { e } else { format!("wp2-sim: {e}\n") });
            return 64;
        }
    };
    if cfg.fresh {
        let _ = fs::remove_file(cfg.store.join("data.dat"));
        let _ = fs::remove_file(cfg.store.join("data.new"));
    }
    if let Some(d) = &cfg.frames {
        let _ = fs::create_dir_all(d);
    }
    let listener = match cfg.tcp.map(|p| TcpListener::bind(("127.0.0.1", p))) {
        Some(Err(e)) => {
            eprintln!("wp2-sim: --tcp: {e}");
            return 69;
        }
        l => l.map(Result::unwrap),
    };
    let (headless, stdin) = (cfg.headless, cfg.stdin);
    let s = SIM.get_or_init(|| Sim::new(cfg));
    {
        let mut st = s.lock();
        let mode = if headless { if st.cfg.virtual_clock { "virtual" } else { "realtime" } } else { "window" };
        st.log("start", &[("app", V::S(&app)), ("mode", V::S(mode))]);
    }
    if stdin {
        thread::spawn(stdin_messages);
    }
    if let Some(l) = listener {
        thread::spawn(move || tcp_channel(l));
    }
    thread::Builder::new()
        .name("app".into())
        .spawn(move || {
            let code = start(&API);
            call(|s| s.finish(End::Exit(code)));
        })
        .expect("app thread");
    let end = if headless { s.wait_end() } else { window::run(s) };
    let mut st = s.lock();
    st.finish(end);
    st.shot("last");
    let end = st.end.unwrap_or(end);
    let code = if let End::Exit(c) = end { format!(" {c}") } else { String::new() };
    eprintln!("wp2-sim: {app}: {}{code} at {} ms, {} shows", end.name(), st.now(), st.shows);
    std::process::exit(end.status())
}

#[cfg(test)]
mod tests;
