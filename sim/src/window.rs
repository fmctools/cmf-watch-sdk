// The owner's window: the round screen at 1:1 (or --scale) and a status bar.
use crate::out::{on_panel, rgb};
use crate::{Action, End, Sim};
use minifb::{InputCallback, Key, KeyRepeat, MouseButton, MouseMode, Scale, Window, WindowOptions};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;
use wp2_core::font8x8::GLYPHS;
use wp2_core::gfx::{H, W};

const BAR: usize = 40;
const BEZEL: u32 = 0x161616;

struct Typed(Rc<RefCell<String>>);

impl InputCallback for Typed {
    fn add_char(&mut self, c: u32) {
        if let Some(c) = char::from_u32(c).filter(|c| !c.is_control()) {
            self.0.borrow_mut().push(c);
        }
    }
}

// status bar text: 8x8 ASCII at 1x
fn text(buf: &mut [u32], x: usize, y: usize, s: &str, c: u32) {
    for (i, ch) in s.chars().enumerate() {
        let g = GLYPHS[if (' '..='~').contains(&ch) { ch as usize - 0x20 } else { usize::from(b'?' - 0x20) }];
        for (row, bits) in g.iter().enumerate() {
            for col in 0..8 {
                let px = x + i * 8 + col;
                if bits >> col & 1 != 0 && px < W {
                    buf[(y + row) * W + px] = c;
                }
            }
        }
    }
}

fn pack([r, g, b]: [u8; 3]) -> u32 {
    u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
}

pub fn run(sim: &Sim) -> End {
    let (title, scale) = {
        let st = sim.lock();
        (format!("wp2-sim: {}", st.cfg.app), st.cfg.scale)
    };
    let scale = match scale {
        2 => Scale::X2,
        4 => Scale::X4,
        _ => Scale::X1,
    };
    let mut win = match Window::new(&title, W, H + BAR, WindowOptions { scale, ..WindowOptions::default() }) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("wp2-sim: window: {e} (use --headless)");
            return End::Closed;
        }
    };
    win.set_target_fps(60);
    let typed = Rc::new(RefCell::new(String::new()));
    win.set_input_callback(Box::new(Typed(typed.clone())));
    let mut buf = vec![0u32; W * (H + BAR)];
    let mut touch: Option<(i16, i16)> = None;
    let mut wheel = 0f32;
    let mut typing = false;
    let mut rate = (Instant::now(), 0u32, 0u32);
    while win.is_open() {
        let mut st = sim.lock();
        // a press starts on the screen; its release may be anywhere, clamped to the screen
        let pos = |mode| win.get_mouse_pos(mode).map(|(x, y)| (x as i16, (y as i16).min(H as i16 - 1)));
        match (win.get_mouse_down(MouseButton::Left), touch) {
            (true, None) => {
                if let Some(p) = pos(MouseMode::Discard).filter(|p| (p.1 as usize) < H - 1) {
                    st.act(Action::Down(p.0, p.1));
                    touch = Some(p);
                }
            }
            (false, Some(last)) => {
                let (x, y) = pos(MouseMode::Clamp).unwrap_or(last);
                st.act(Action::Up(x, y));
                touch = None;
            }
            _ => {}
        }
        if let Some((_, dy)) = win.get_scroll_wheel() {
            wheel += dy;
            let steps = (wheel / 2.0).trunc();
            if steps != 0.0 {
                st.act(Action::Crown(-(steps.clamp(-5.0, 5.0) as i16)));
                wheel -= steps * 2.0;
            }
        }
        for k in win.get_keys_pressed(KeyRepeat::Yes) {
            if typing {
                match k {
                    Key::Enter => {
                        let m = std::mem::take(&mut *typed.borrow_mut());
                        if !m.is_empty() {
                            st.act(Action::Msg(m.into_bytes()));
                        }
                        typing = false;
                    }
                    Key::Escape => typing = false,
                    Key::Backspace => {
                        typed.borrow_mut().pop();
                    }
                    _ => {}
                }
                continue;
            }
            match k {
                Key::Escape | Key::B => st.act(Action::Button),
                Key::P => st.act(Action::Pause),
                Key::M => typing = true,
                Key::Up | Key::Left => st.act(Action::Crown(-1)),
                Key::Down | Key::Right => st.act(Action::Crown(1)),
                _ => {}
            }
        }
        if !typing {
            typed.borrow_mut().clear();
        }
        st.pump();
        if let Some(e) = st.end {
            return e;
        }
        let dim = st.paused;
        let panel = st.panel();
        for (i, &c) in panel.iter().enumerate() {
            buf[i] = if !on_panel(i % W, i / W) {
                BEZEL
            } else if dim {
                pack(rgb(c).map(|v| v / 3))
            } else {
                pack(rgb(c))
            };
        }
        if rate.0.elapsed().as_secs_f32() >= 1.0 {
            rate = (Instant::now(), st.shows - rate.1, st.shows);
        }
        buf[W * H..].fill(0x202020);
        let mut line1 = format!("{:>3} fps  {:>6.1} s", rate.2, st.now() as f32 / 1000.0);
        let vib = match (st.motor, st.last_vibrate) {
            (Some((d, hz)), _) => format!("  VIBRATE {d}/1000 {hz} Hz"),
            (None, Some((d, hz, rc))) => format!("  vib {d}/1000 {hz} Hz rc {rc}"),
            _ => String::new(),
        };
        line1 += &vib;
        if st.keep_on {
            line1 += "  keep-on";
        }
        if st.paused {
            line1 += "  PAUSED (P)";
        }
        let line2 = if typing {
            format!("message> {}_", typed.borrow())
        } else {
            st.sent.back().map_or("M: message  P: pause  Esc/B: button".into(), |m| format!("sent: {m}"))
        };
        let red = if st.motor.is_some() { 0xFF5040 } else { 0xC0C0C0 };
        drop(st);
        sim.notify();
        text(&mut buf[W * H..], 8, 8, &line1, red);
        text(&mut buf[W * H..], 8, 24, &line2, 0x9090A0);
        if win.update_with_buffer(&buf, W, H + BAR).is_err() {
            break;
        }
    }
    End::Closed
}
