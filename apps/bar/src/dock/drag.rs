//! Picking an icon up and dropping it: a small state machine over the pointer, so the rules
//! (threshold, drop, cancel) are tested without windows. It knows nothing of icons beyond an id;
//! `Dock` decides where a drop lands (see `items::drop_slot`).
//!
//! Press on an icon, move past `START` and the icon is picked up. Letting go drops it; Esc cancels.
//! Letting go before the threshold is an ordinary click, which egui reports itself.

use egui::Pos2;

/// How far, in points, the pointer must travel from where it was pressed before an icon is
/// picked up. Points scale with the monitor's DPI (6 pt is 9 px at 150%), so the feel is the same
/// everywhere; it equals egui's own limit for a click, so a press is either a click or a drag.
pub const START: f32 = 6.0;

#[derive(Default, Debug, PartialEq)]
enum State {
    #[default]
    Idle,
    /// Button down on an icon, not yet moved far enough.
    Pressed { id: String, origin: Pos2, grab: f32 },
    /// Picked up. `grab` is how far the pointer is from the icon's left edge, so the icon does not
    /// jump to centre on the pointer. `last` is the latest place the pointer was seen.
    Dragging { id: String, grab: f32, last: Pos2 },
}

/// What a frame of pointer input did.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Nothing is pressed.
    Idle,
    /// Pressed, below the threshold.
    Pending,
    /// Released below the threshold: a click.
    Click,
    /// Picked up and moving; the pointer is `None` while it is out of the window.
    Dragging(Option<Pos2>),
    /// Button released: the icon, its grab offset, and where; `None` when the pointer was out of
    /// the window.
    Drop { id: String, grab: f32, pos: Option<Pos2> },
    Cancel,
}

#[derive(Default)]
pub struct Machine {
    state: State,
    /// The dock window (monitor) the drag belongs to.
    salt: String,
}

impl Machine {
    /// Start tracking a press. Ignored while another press or drag is on.
    pub fn press(&mut self, salt: &str, id: &str, origin: Pos2, grab: f32) {
        if self.state == State::Idle {
            self.state = State::Pressed { id: id.to_owned(), origin, grab };
            self.salt = salt.to_owned();
        }
    }

    /// Whether a press or drag is on in the dock window `salt`.
    pub fn is_for(&self, salt: &str) -> bool {
        self.state != State::Idle && self.salt == salt
    }

    /// The id and grab offset of the icon in hand, once picked up.
    pub fn held(&self) -> Option<(&str, f32)> {
        match &self.state {
            State::Dragging { id, grab, .. } => Some((id, *grab)),
            _ => None,
        }
    }

    /// The pointer's last known place while dragging.
    pub fn last(&self) -> Option<Pos2> {
        match &self.state {
            State::Dragging { last, .. } => Some(*last),
            _ => None,
        }
    }

    pub fn cancel(&mut self) {
        self.state = State::Idle;
    }

    /// One frame: where the pointer is (`None` out of the window), whether the primary button is
    /// down, whether Esc is down.
    pub fn update(&mut self, pos: Option<Pos2>, down: bool, esc: bool) -> Step {
        match std::mem::take(&mut self.state) {
            State::Idle => Step::Idle,
            State::Pressed { id, origin, grab } => {
                if !down {
                    return Step::Click;
                }
                match pos {
                    Some(p) if p.distance(origin) >= START => {
                        self.state = State::Dragging { id, grab, last: p };
                        Step::Dragging(Some(p))
                    }
                    _ => {
                        self.state = State::Pressed { id, origin, grab };
                        Step::Pending
                    }
                }
            }
            State::Dragging { id, grab, last } => {
                if esc {
                    return Step::Cancel;
                }
                if !down {
                    return Step::Drop { id, grab, pos };
                }
                let last = pos.unwrap_or(last);
                self.state = State::Dragging { id, grab, last };
                Step::Dragging(pos)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f32, y: f32) -> Option<Pos2> {
        Some(egui::pos2(x, y))
    }

    fn pressed() -> Machine {
        let mut m = Machine::default();
        m.press("a", "app", egui::pos2(100.0, 20.0), 10.0);
        m
    }

    #[test]
    fn idle_does_nothing() {
        let mut m = Machine::default();
        assert_eq!(m.update(p(1.0, 1.0), true, false), Step::Idle);
        assert!(!m.is_for("a"));
    }

    #[test]
    fn a_short_press_is_a_click() {
        let mut m = pressed();
        assert_eq!(m.update(p(103.0, 21.0), true, false), Step::Pending);
        assert_eq!(m.update(p(103.0, 21.0), false, false), Step::Click);
        assert_eq!(m.update(p(103.0, 21.0), false, false), Step::Idle);
    }

    #[test]
    fn the_threshold_picks_the_icon_up() {
        let mut m = pressed();
        assert_eq!(m.update(p(105.0, 20.0), true, false), Step::Pending);
        assert!(m.held().is_none());
        assert_eq!(m.update(p(106.0, 20.0), true, false), Step::Dragging(p(106.0, 20.0)));
        assert_eq!(m.held(), Some(("app", 10.0)));
        // Coming back near the start does not put it down again.
        assert_eq!(m.update(p(101.0, 20.0), true, false), Step::Dragging(p(101.0, 20.0)));
    }

    #[test]
    fn release_drops_where_the_pointer_is() {
        let mut m = pressed();
        m.update(p(150.0, 20.0), true, false);
        assert_eq!(m.update(p(180.0, 20.0), false, false), Step::Drop { id: "app".into(), grab: 10.0, pos: p(180.0, 20.0) });
        assert!(!m.is_for("a"));
    }

    #[test]
    fn escape_cancels_and_a_later_release_does_nothing() {
        let mut m = pressed();
        m.update(p(150.0, 20.0), true, false);
        assert_eq!(m.update(p(160.0, 20.0), true, true), Step::Cancel);
        assert_eq!(m.update(p(160.0, 20.0), false, false), Step::Idle);
    }

    #[test]
    fn a_pointer_out_of_the_window_is_remembered_and_drops_nowhere() {
        let mut m = pressed();
        m.update(p(150.0, 20.0), true, false);
        assert_eq!(m.update(None, true, false), Step::Dragging(None));
        assert_eq!(m.last(), p(150.0, 20.0));
        assert_eq!(m.update(None, false, false), Step::Drop { id: "app".into(), grab: 10.0, pos: None });
    }

    #[test]
    fn one_press_at_a_time_and_per_window() {
        let mut m = pressed();
        m.press("b", "other", egui::pos2(0.0, 0.0), 0.0);
        assert!(m.is_for("a") && !m.is_for("b"));
        m.update(p(150.0, 20.0), true, false);
        assert_eq!(m.held(), Some(("app", 10.0)));
        m.cancel();
        assert!(!m.is_for("a"));
    }
}
