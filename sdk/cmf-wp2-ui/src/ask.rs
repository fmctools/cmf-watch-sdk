//! Asking the phone until it answers. An app's first message wakes the phone app (iOS: CMFX, woken over Bluetooth);
//! a phone app that iOS has to relaunch loses that message, so the app asks again every few seconds until anything
//! comes back, a bounded number of times.

pub struct Ask {
    at: Option<u32>,
    left: u8,
}

impl Ask {
    pub const EVERY: u32 = 3000;

    /// Asked at `now`; at most `tries` more times.
    pub fn new(now: u32, tries: u8) -> Self {
        Self { at: (tries > 0).then(|| now.wrapping_add(Self::EVERY)), left: tries }
    }

    /// ms to the next ask (None: answered, or given up): the longest an event wait may be.
    pub fn wait(&self, now: u32) -> Option<u32> {
        self.at.map(|t| t.wrapping_sub(now).min(Self::EVERY))
    }

    /// true: time to ask again (call after each event or timeout).
    pub fn due(&mut self, now: u32) -> bool {
        match self.at {
            Some(t) if now.wrapping_sub(t) as i32 >= 0 => {
                self.left -= 1;
                self.at = (self.left > 0).then(|| now.wrapping_add(Self::EVERY));
                true
            }
            _ => false,
        }
    }

    /// The phone answered: no more asking.
    pub fn answered(&mut self) {
        self.at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::Ask;

    #[test]
    fn asks_until_answered_or_out_of_tries() {
        let mut a = Ask::new(0, 2);
        assert!(!a.due(2999));
        assert_eq!(a.wait(1000), Some(2000));
        assert!(a.due(3000));
        assert!(a.due(6000));
        assert_eq!(a.wait(6000), None);
        assert!(!a.due(9000));
        let mut b = Ask::new(0, 5);
        b.answered();
        assert!(!b.due(3000));
    }
}
