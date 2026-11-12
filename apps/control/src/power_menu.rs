//! The power button's inline choice: Restart, Shut down or Sign out, each asking for a second
//! click before it acts. Pure state, so the confirmation rules are tested without ever ending
//! the session.

use shellman_core::system::power::Action;
use std::time::{Duration, Instant};

/// A choice that is not confirmed within this long goes back to unarmed.
pub const CONFIRM_WITHIN: Duration = Duration::from_secs(4);

/// The three choices, in the order they are shown.
pub const CHOICES: [Action; 3] = [Action::Restart, Action::ShutDown, Action::SignOut];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PowerMenu {
    #[default]
    Closed,
    /// The choices show; none is armed.
    Open,
    /// One was clicked once, at `at`; a second click carries it out.
    Armed { action: Action, at: Instant },
}

impl PowerMenu {
    /// The power button: open the choices, or close them.
    pub fn toggle(&mut self) {
        *self = match self {
            PowerMenu::Closed => PowerMenu::Open,
            _ => PowerMenu::Closed,
        };
    }

    /// A choice was clicked. Returns the action to carry out when this click confirms it.
    pub fn press(&mut self, action: Action, now: Instant) -> Option<Action> {
        self.expire(now);
        match *self {
            PowerMenu::Closed => None,
            PowerMenu::Armed { action: armed, .. } if armed == action => {
                *self = PowerMenu::Closed;
                Some(action)
            }
            _ => {
                *self = PowerMenu::Armed { action, at: now };
                None
            }
        }
    }

    /// Disarm a choice left unconfirmed too long.
    pub fn expire(&mut self, now: Instant) {
        if let PowerMenu::Armed { at, .. } = *self {
            if now.duration_since(at) >= CONFIRM_WITHIN {
                *self = PowerMenu::Open;
            }
        }
    }

    pub fn is_open(&self) -> bool {
        *self != PowerMenu::Closed
    }

    pub fn armed(&self) -> Option<Action> {
        match *self {
            PowerMenu::Armed { action, .. } => Some(action),
            _ => None,
        }
    }

    /// When the armed choice runs out, for one wake-up to redraw it.
    pub fn deadline(&self) -> Option<Instant> {
        match *self {
            PowerMenu::Armed { at, .. } => Some(at + CONFIRM_WITHIN),
            _ => None,
        }
    }
}

/// The label of a choice, and what it says while armed.
pub fn label(action: Action, armed: bool) -> &'static str {
    match (action, armed) {
        (Action::Restart, false) => "Restart",
        (Action::Restart, true) => "Restart now",
        (Action::ShutDown, false) => "Shut down",
        (Action::ShutDown, true) => "Shut down now",
        (Action::SignOut, false) => "Sign out",
        (Action::SignOut, true) => "Sign out now",
        (Action::Lock, _) => "Lock",
        (Action::Sleep, _) => "Sleep",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_click_only_arms() {
        let t = Instant::now();
        let mut m = PowerMenu::default();
        assert_eq!(m.press(Action::Restart, t), None, "closed: nothing happens");
        m.toggle();
        assert!(m.is_open());
        assert_eq!(m.press(Action::Restart, t), None);
        assert_eq!(m.armed(), Some(Action::Restart));
    }

    #[test]
    fn the_second_click_on_the_same_choice_confirms() {
        let t = Instant::now();
        let mut m = PowerMenu::Open;
        m.press(Action::ShutDown, t);
        assert_eq!(m.press(Action::ShutDown, t + Duration::from_secs(1)), Some(Action::ShutDown));
        assert_eq!(m, PowerMenu::Closed);
    }

    #[test]
    fn another_choice_rearms_instead() {
        let t = Instant::now();
        let mut m = PowerMenu::Open;
        m.press(Action::ShutDown, t);
        assert_eq!(m.press(Action::SignOut, t), None);
        assert_eq!(m.armed(), Some(Action::SignOut));
    }

    #[test]
    fn a_late_second_click_only_arms_again() {
        let t = Instant::now();
        let mut m = PowerMenu::Open;
        m.press(Action::Restart, t);
        assert_eq!(m.press(Action::Restart, t + CONFIRM_WITHIN), None);
        assert_eq!(m.armed(), Some(Action::Restart));
        m.expire(t + CONFIRM_WITHIN * 3);
        assert_eq!(m, PowerMenu::Open);
    }

    #[test]
    fn the_power_button_closes_the_choices() {
        let mut m = PowerMenu::Armed { action: Action::Restart, at: Instant::now() };
        m.toggle();
        assert_eq!(m, PowerMenu::Closed);
        assert_eq!(m.deadline(), None);
    }
}
