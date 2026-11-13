//! Mouse & keyboard: pointer speed and precision, the primary button, the wheel, the double-click
//! time, and the keyboard's repeat delay and rate. The values are read from Windows when the page
//! is shown and a change is written at once (a slider when the knob is let go), so a change made
//! elsewhere shows the next time the page is opened. The writes are
//! `shellman_core::system::pointer`.

use super::slider_row;
use crate::page;
use crate::state::State;
use egui::Id;
use shellman_core::system::pointer::{self as p, Change, Pointer};
use shellman_ui::widgets::switch;

/// The most lines the wheel slider offers; "One screen" is its own switch.
const LINES_SHOWN: (u32, u32) = (1, 20);

/// The page's live state: what is shown, and what Windows was last told.
#[derive(Default)]
pub struct MouseView {
    shown: Option<Pointer>,
    applied: Option<Pointer>,
}

impl MouseView {
    /// Forget the reading, so the page reads again when it is next shown.
    pub fn stop(&mut self) {
        *self = Self::default();
    }
}

/// The calls that bring Windows from `from` to `to`.
fn changes(from: &Pointer, to: &Pointer) -> Vec<Change> {
    let mut out = Vec::new();
    if from.speed != to.speed {
        out.push(Change::Speed(to.speed));
    }
    if from.precision != to.precision {
        out.push(Change::Precision(to.precision));
    }
    if from.swapped != to.swapped {
        out.push(Change::Swapped(to.swapped));
    }
    if from.wheel_lines != to.wheel_lines {
        out.push(Change::WheelLines(to.wheel_lines));
    }
    if from.wheel_chars != to.wheel_chars {
        out.push(Change::WheelChars(to.wheel_chars));
    }
    if from.double_click_ms != to.double_click_ms {
        out.push(Change::DoubleClick(to.double_click_ms));
    }
    if from.repeat_delay != to.repeat_delay {
        out.push(Change::RepeatDelay(to.repeat_delay));
    }
    if from.repeat_rate != to.repeat_rate {
        out.push(Change::RepeatRate(to.repeat_rate));
    }
    out
}

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Mouse & keyboard", "How the pointer, the wheel and the keys behave. Changes apply at once.");
    let v = &mut st.mouse;
    if v.shown.is_none() {
        let now = p::read();
        v.shown = Some(now);
        v.applied = Some(now);
    }
    let (Some(mut s), Some(applied)) = (v.shown, v.applied) else { return };
    let mut held = false;
    page::group(ui, "Mouse", |ui| {
        let m = slider_row::show(ui, Id::new("mouse-speed"), "Pointer speed", p::fraction(s.speed, p::SPEED_RANGE), &format!("{} of 20", s.speed));
        if let Some(f) = m.value {
            s.speed = p::value_at(f, p::SPEED_RANGE);
        }
        held |= m.held;
        ui.add_space(10.0);
        switch(ui, &mut s.precision, "Enhance pointer precision");
        ui.add_space(10.0);
        page::label(ui, "Primary button");
        let mut right = s.swapped;
        if page::segmented(ui, &mut right, &[(false, "Left"), (true, "Right")]) {
            s.swapped = right;
        }
        ui.add_space(14.0);
        let page_scroll = s.wheel_lines == p::WHEEL_PAGE;
        let at = if page_scroll { 0.15 } else { p::fraction(s.wheel_lines, LINES_SHOWN) };
        let m = slider_row::show(ui, Id::new("mouse-lines"), "Lines to scroll at a time", at, &p::wheel_label(s.wheel_lines));
        if let Some(f) = m.value {
            s.wheel_lines = p::value_at(f, LINES_SHOWN);
        }
        held |= m.held;
        ui.add_space(6.0);
        let mut screen = page_scroll;
        if switch(ui, &mut screen, "Scroll one screen at a time").changed() {
            s.wheel_lines = if screen { p::WHEEL_PAGE } else { 3 };
        }
        ui.add_space(10.0);
        let at = p::fraction(s.wheel_chars, LINES_SHOWN);
        let m = slider_row::show(ui, Id::new("mouse-chars"), "Characters to scroll sideways", at, &format!("{} characters", s.wheel_chars));
        if let Some(f) = m.value {
            s.wheel_chars = p::value_at(f, LINES_SHOWN);
        }
        held |= m.held;
        ui.add_space(10.0);
        let at = p::fraction(s.double_click_ms, p::DOUBLE_CLICK_RANGE);
        let m = slider_row::show(ui, Id::new("mouse-dbl"), "Double-click speed", at, &format!("{} ms between clicks", s.double_click_ms));
        if let Some(f) = m.value {
            // Whole tens of milliseconds are plenty to tell apart.
            s.double_click_ms = (p::value_at(f, p::DOUBLE_CLICK_RANGE) + 5) / 10 * 10;
        }
        held |= m.held;
    });
    page::group(ui, "Keyboard", |ui| {
        let at = p::fraction(s.repeat_delay, (0, p::DELAY_STEPS - 1));
        let m = slider_row::show(ui, Id::new("kb-delay"), "Repeat delay", at, &format!("{} ms", p::delay_ms(s.repeat_delay)));
        if let Some(f) = m.value {
            s.repeat_delay = p::value_at(f, (0, p::DELAY_STEPS - 1));
        }
        held |= m.held;
        ui.add_space(10.0);
        let at = p::fraction(s.repeat_rate, (0, p::RATE_STEPS - 1));
        let m = slider_row::show(ui, Id::new("kb-rate"), "Repeat rate", at, &p::rate_label(s.repeat_rate));
        if let Some(f) = m.value {
            s.repeat_rate = p::value_at(f, (0, p::RATE_STEPS - 1));
        }
        held |= m.held;
    });
    // Windows is told once the knob is let go, one call for each value that moved.
    if !held && s != applied {
        for change in changes(&applied, &s) {
            p::apply_async(change);
        }
        v.applied = Some(s);
    }
    v.shown = Some(s);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Pointer {
        Pointer { speed: 10, precision: true, swapped: false, wheel_lines: 3, wheel_chars: 3, double_click_ms: 500, repeat_delay: 1, repeat_rate: 31 }
    }

    #[test]
    fn nothing_moved_means_no_calls() {
        assert!(changes(&base(), &base()).is_empty());
    }

    #[test]
    fn only_what_moved_is_written() {
        let to = Pointer { speed: 12, swapped: true, ..base() };
        assert_eq!(changes(&base(), &to), vec![Change::Speed(12), Change::Swapped(true)]);
    }
}
