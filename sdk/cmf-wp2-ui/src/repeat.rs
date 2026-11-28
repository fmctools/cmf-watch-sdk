//! Hold to repeat (the delete keys): a tap acts once; held, it acts again after [`DELAY_MS`], then every
//! [`EVERY_MS`] until the finger lifts. The app has TOUCH_DOWN and TOUCH_UP only, so the repeats come from the
//! event timeout ([`Repeat::wait`]) and a check on each pass of the loop ([`Repeat::due`]).

pub const DELAY_MS: u32 = 450;
pub const EVERY_MS: u32 = 90;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Repeat {
    next: Option<u32>,
}

impl Repeat {
    pub const fn new() -> Self {
        Repeat { next: None }
    }

    /// The finger went down on the key at `now` (it acted once already).
    pub fn start(&mut self, now: u32) {
        self.next = Some(now.wrapping_add(DELAY_MS));
    }

    /// The finger lifted: no more, at once.
    pub fn stop(&mut self) {
        self.next = None;
    }

    pub fn held(&self) -> bool {
        self.next.is_some()
    }

    /// ms until the next repeat (0: due), for the event timeout; `None` while not held.
    pub fn wait(&self, now: u32) -> Option<u32> {
        self.next.map(|t| (t.wrapping_sub(now) as i32).max(0) as u32)
    }

    /// Whether a repeat is due at `now`; the next one is [`EVERY_MS`] later (a late loop doesn't burst).
    pub fn due(&mut self, now: u32) -> bool {
        match self.next {
            Some(t) if now.wrapping_sub(t) as i32 >= 0 => {
                self.next = Some(now.wrapping_add(EVERY_MS));
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeats_while_held_and_stops_on_lift() {
        let mut r = Repeat::new();
        assert_eq!(r.wait(0), None);
        assert!(!r.due(10_000));
        r.start(1000);
        assert_eq!(r.wait(1000), Some(DELAY_MS));
        assert!(!r.due(1000 + DELAY_MS - 1));
        assert!(r.due(1000 + DELAY_MS));
        assert!(!r.due(1000 + DELAY_MS + 1));
        assert_eq!(r.wait(1000 + DELAY_MS + EVERY_MS + 5), Some(0));
        assert!(r.due(1000 + DELAY_MS + EVERY_MS));
        r.stop();
        assert!(!r.held());
        assert!(!r.due(u32::MAX / 2));
        // across the clock's wrap
        r.start(u32::MAX - 10);
        assert!(r.due(DELAY_MS));
    }
}
