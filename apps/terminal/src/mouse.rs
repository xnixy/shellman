//! Mouse reports to programs that ask for them (vim, htop, tmux, Midnight Commander): which
//! events a tracking mode wants, and the bytes xterm sends for them in each encoding. Pure, so
//! every mode and encoding is tested.

use crate::keys::Mods;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Button {
    Left,
    Middle,
    Right,
    /// Motion with no button held (any-event tracking).
    None,
    WheelUp,
    WheelDown,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Press,
    Release,
    Motion,
}

/// Which tracking the program turned on (the strongest wins) and how reports are written.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Modes {
    /// DECSET 9: presses only, no modifiers.
    pub x10: bool,
    /// 1000: presses and releases.
    pub click: bool,
    /// 1002: and motion while a button is held.
    pub drag: bool,
    /// 1003: and all motion.
    pub motion: bool,
    /// 1006: `CSI < b ; x ; y M/m`.
    pub sgr: bool,
    /// 1005: coordinates as UTF-8 characters.
    pub utf8: bool,
}

impl Modes {
    /// Whether any tracking is on: then the mouse belongs to the program (Shift takes it back).
    pub fn tracking(&self) -> bool {
        self.x10 || self.click || self.drag || self.motion
    }

    /// Whether this event is reported. `held` is whether a button is down during a motion.
    pub fn wants(&self, button: Button, action: Action, held: bool) -> bool {
        match action {
            Action::Press => self.tracking(),
            // X10 reports presses only; wheels have no release.
            Action::Release => {
                (self.click || self.drag || self.motion) && !matches!(button, Button::WheelUp | Button::WheelDown)
            }
            Action::Motion => self.motion || (self.drag && held),
        }
    }
}

/// The report for one event at cell `col`, `row` (from 0, top-left of the screen), or None when
/// the encoding cannot say where (the plain encoding stops at column 223).
pub fn report(button: Button, action: Action, mods: Mods, col: usize, row: usize, modes: &Modes) -> Option<Vec<u8>> {
    let base: u32 = match button {
        Button::Left => 0,
        Button::Middle => 1,
        Button::Right => 2,
        Button::None => 3,
        Button::WheelUp => 64,
        Button::WheelDown => 65,
    };
    let mut code = base;
    let only_x10 = modes.x10 && !(modes.click || modes.drag || modes.motion);
    if !only_x10 {
        code += 4 * mods.shift as u32 + 8 * mods.alt as u32 + 16 * mods.ctrl as u32;
    }
    if action == Action::Motion {
        code += 32;
    }
    let (x, y) = (col as u32 + 1, row as u32 + 1);
    if modes.sgr {
        let end = if action == Action::Release { 'm' } else { 'M' };
        return Some(format!("\x1b[<{code};{x};{y}{end}").into_bytes());
    }
    // The older encodings cannot say which button went up.
    if action == Action::Release {
        code = (code & !3) | 3;
    }
    let mut out = b"\x1b[M".to_vec();
    if modes.utf8 {
        for v in [32 + code, 32 + x, 32 + y] {
            let c = char::from_u32(v).filter(|_| v < 2048)?;
            let mut buf = [0; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    } else {
        for v in [32 + code, 32 + x, 32 + y] {
            if v > 255 {
                return None;
            }
            out.push(v as u8);
        }
    }
    Some(out)
}

/// Focus in and out (DECSET 1004).
pub fn focus(focused: bool) -> &'static [u8] {
    if focused {
        b"\x1b[I"
    } else {
        b"\x1b[O"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Mods = Mods::NONE;
    const NORMAL: Modes = Modes { x10: false, click: true, drag: false, motion: false, sgr: false, utf8: false };

    fn sgr(m: Modes) -> Modes {
        Modes { sgr: true, ..m }
    }

    #[test]
    fn normal_encoding_offsets_by_32_and_counts_from_one() {
        assert_eq!(report(Button::Left, Action::Press, NONE, 0, 0, &NORMAL).unwrap(), b"\x1b[M !!");
        assert_eq!(report(Button::Right, Action::Press, NONE, 9, 4, &NORMAL).unwrap(), b"\x1b[M\"*%");
        // A release says button 3, whichever went up.
        assert_eq!(report(Button::Right, Action::Release, NONE, 0, 0, &NORMAL).unwrap(), b"\x1b[M#!!");
        // Past column 223 it cannot be said.
        assert_eq!(report(Button::Left, Action::Press, NONE, 223, 0, &NORMAL), None);
        assert!(report(Button::Left, Action::Press, NONE, 222, 0, &NORMAL).is_some());
    }

    #[test]
    fn modifiers_and_motion_add_to_the_button() {
        let ctrl_shift = Mods { shift: true, ctrl: true, alt: false };
        // 0 + 4 + 16 = 20, +32 = 52 = '4'.
        assert_eq!(report(Button::Left, Action::Press, ctrl_shift, 0, 0, &NORMAL).unwrap(), b"\x1b[M4!!");
        let alt = Mods { alt: true, ..NONE };
        assert_eq!(report(Button::Left, Action::Motion, alt, 0, 0, &sgr(NORMAL)).unwrap(), b"\x1b[<40;1;1M");
    }

    #[test]
    fn sgr_says_which_button_went_up_and_has_no_limit() {
        let m = sgr(NORMAL);
        assert_eq!(report(Button::Left, Action::Press, NONE, 299, 99, &m).unwrap(), b"\x1b[<0;300;100M");
        assert_eq!(report(Button::Middle, Action::Release, NONE, 0, 0, &m).unwrap(), b"\x1b[<1;1;1m");
        assert_eq!(report(Button::WheelUp, Action::Press, NONE, 4, 2, &m).unwrap(), b"\x1b[<64;5;3M");
        assert_eq!(report(Button::WheelDown, Action::Press, NONE, 4, 2, &m).unwrap(), b"\x1b[<65;5;3M");
        assert_eq!(report(Button::None, Action::Motion, NONE, 1, 1, &m).unwrap(), b"\x1b[<35;2;2M");
    }

    #[test]
    fn utf8_encoding_goes_past_223() {
        let m = Modes { utf8: true, ..NORMAL };
        let r = report(Button::Left, Action::Press, NONE, 299, 0, &m).unwrap();
        // Button 32, column 32 + 300 (two bytes of UTF-8), row 33.
        assert_eq!(String::from_utf8(r).unwrap(), "\x1b[M \u{14C}!");
    }

    #[test]
    fn x10_reports_presses_only_and_no_modifiers() {
        let m = Modes { x10: true, ..Modes::default() };
        assert!(m.wants(Button::Left, Action::Press, false));
        assert!(!m.wants(Button::Left, Action::Release, false));
        assert!(!m.wants(Button::Left, Action::Motion, true));
        let ctrl = Mods { ctrl: true, ..NONE };
        assert_eq!(report(Button::Left, Action::Press, ctrl, 0, 0, &m).unwrap(), b"\x1b[M !!");
    }

    #[test]
    fn each_tracking_mode_wants_its_events() {
        let click = NORMAL;
        assert!(click.wants(Button::Left, Action::Release, false));
        assert!(!click.wants(Button::Left, Action::Motion, true));
        assert!(!click.wants(Button::WheelUp, Action::Release, false));
        let drag = Modes { drag: true, ..Modes::default() };
        assert!(drag.wants(Button::Left, Action::Motion, true));
        assert!(!drag.wants(Button::None, Action::Motion, false));
        let any = Modes { motion: true, ..Modes::default() };
        assert!(any.wants(Button::None, Action::Motion, false));
        assert!(!Modes::default().tracking());
        assert!(!Modes::default().wants(Button::Left, Action::Press, false));
    }

    #[test]
    fn focus_reports() {
        assert_eq!(focus(true), b"\x1b[I");
        assert_eq!(focus(false), b"\x1b[O");
    }
}
