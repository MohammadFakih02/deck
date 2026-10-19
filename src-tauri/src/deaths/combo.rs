//! The controller combo: hold Select (View on Xbox) + D-pad Down for a second = +1 death,
//! Select + D-pad Up = undo. Holding is required so a quick press in the game never counts;
//! one hold counts once, the buttons have to be let go before the next.

use crate::pad::input::PadState;
use std::time::{Duration, Instant};

pub const HOLD: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct Combo {
    /// the action being held and since when
    held: Option<(&'static str, Instant)>,
    /// that hold already counted
    fired: bool,
}

fn wanted(st: &PadState) -> Option<&'static str> {
    if !st.button("create") {
        return None;
    }
    match (st.button("down"), st.button("up")) {
        (true, false) => Some("death"),
        (false, true) => Some("undo"),
        _ => None,
    }
}

impl Combo {
    /// The pad's state now (None: no pad); the action to do when a hold completes.
    pub fn feed(&mut self, st: Option<&PadState>, now: Instant) -> Option<&'static str> {
        let want = st.and_then(wanted);
        match (want, self.held) {
            (None, _) => {
                self.held = None;
                self.fired = false;
                None
            }
            (Some(w), Some((h, since))) if w == h => {
                if !self.fired && now.duration_since(since) >= HOLD {
                    self.fired = true;
                    return Some(w);
                }
                None
            }
            (Some(w), _) => {
                // a new hold (or switched from Down to Up while holding Select)
                self.held = Some((w, now));
                self.fired = false;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(buttons: &[&str]) -> PadState {
        let mut st = PadState::default();
        for b in buttons {
            st.set(b, true);
        }
        st
    }

    #[test]
    fn a_full_hold_counts_once() {
        let t = Instant::now();
        let mut c = Combo::default();
        let p = pad(&["create", "down"]);
        assert_eq!(c.feed(Some(&p), t), None);
        assert_eq!(c.feed(Some(&p), t + Duration::from_millis(500)), None);
        assert_eq!(c.feed(Some(&p), t + HOLD), Some("death"));
        assert_eq!(c.feed(Some(&p), t + HOLD * 3), None, "still held: no repeat");
        assert_eq!(c.feed(Some(&pad(&[])), t + HOLD * 3), None);
        let t2 = t + HOLD * 4;
        assert_eq!(c.feed(Some(&p), t2), None);
        assert_eq!(c.feed(Some(&p), t2 + HOLD), Some("death"), "let go and held again: counts again");
    }

    #[test]
    fn quick_presses_and_other_buttons_never_count() {
        let t = Instant::now();
        let mut c = Combo::default();
        let p = pad(&["create", "down"]);
        c.feed(Some(&p), t);
        c.feed(Some(&pad(&["down"])), t + Duration::from_millis(600));
        assert_eq!(c.feed(Some(&p), t + HOLD), None, "Select let go: the hold starts over");
        assert_eq!(c.feed(Some(&pad(&["create", "down", "up"])), t + HOLD * 3), None);
        assert_eq!(c.feed(None, t + HOLD * 5), None);
        assert!(c.held.is_none());
    }

    #[test]
    fn up_is_undo() {
        let t = Instant::now();
        let mut c = Combo::default();
        let p = pad(&["create", "up"]);
        c.feed(Some(&p), t);
        assert_eq!(c.feed(Some(&p), t + HOLD), Some("undo"));
    }
}
