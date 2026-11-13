//! Which bar's clock the notifications hang from. Pure, so the rules are tested.
//!
//! - The list (with the calendar) opens under the clock that was clicked (or, for Win+N and the
//!   "now" pill, the bar it was asked from), on that monitor.
//! - Toasts pop up under the clock of the display with the window in front (the default), or of
//!   the main display, as Settings › Top bar says. Once some are up they stay on that display
//!   until they have all gone, so a burst never jumps between screens.

use shellman_core::settings::bar::ToastScreen;

/// One bar's clock island, in signed screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    /// The island's centre and lower edge: what the window hangs from.
    pub anchor: (i32, i32),
    /// The island itself (left, top, right, bottom), which counts as inside the open list (a
    /// click there toggles it, and must not also close it from outside).
    pub rect: (i32, i32, i32, i32),
    /// The "now" pill beside it, which can open the list too.
    pub also: Option<(i32, i32, i32, i32)>,
}

/// The display toasts go to: the focused window's (if it has a bar), else the primary's, else
/// any bar's. `showing` is where toasts already are, if any are up: they stay there.
pub fn toast_device<'a>(
    setting: ToastScreen,
    showing: Option<&'a str>,
    focused: Option<&'a str>,
    primary: Option<&'a str>,
    bars: &[&'a str],
) -> Option<&'a str> {
    let has = |d: &&str| bars.contains(d);
    if let Some(d) = showing.filter(has) {
        return Some(d);
    }
    let first = match setting {
        ToastScreen::Focused => focused.filter(has),
        ToastScreen::Primary => None,
    };
    first.or(primary.filter(has)).or(bars.first().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BARS: [&str; 3] = ["\\\\.\\DISPLAY1", "\\\\.\\DISPLAY2", "\\\\.\\DISPLAY3"];

    #[test]
    fn toasts_follow_the_window_in_front_by_default() {
        let d = toast_device(ToastScreen::Focused, None, Some(BARS[1]), Some(BARS[0]), &BARS);
        assert_eq!(d, Some(BARS[1]));
        // The main display when asked.
        let d = toast_device(ToastScreen::Primary, None, Some(BARS[1]), Some(BARS[0]), &BARS);
        assert_eq!(d, Some(BARS[0]));
    }

    #[test]
    fn a_display_without_a_bar_falls_back() {
        // Nothing in front (the desktop), or in front on a display without a bar.
        assert_eq!(toast_device(ToastScreen::Focused, None, None, Some(BARS[0]), &BARS), Some(BARS[0]));
        assert_eq!(toast_device(ToastScreen::Focused, None, Some("\\\\.\\DISPLAY9"), Some(BARS[0]), &BARS), Some(BARS[0]));
        // No primary bar either (a test bar on one monitor): the first bar.
        assert_eq!(toast_device(ToastScreen::Focused, None, None, None, &BARS[1..]), Some(BARS[1]));
        assert_eq!(toast_device(ToastScreen::Focused, None, None, None, &[]), None);
    }

    #[test]
    fn toasts_already_up_stay_where_they_are() {
        let d = toast_device(ToastScreen::Focused, Some(BARS[2]), Some(BARS[1]), Some(BARS[0]), &BARS);
        assert_eq!(d, Some(BARS[2]));
        // Unless that bar went (a monitor unplugged).
        let d = toast_device(ToastScreen::Focused, Some(BARS[2]), Some(BARS[1]), Some(BARS[0]), &BARS[..2]);
        assert_eq!(d, Some(BARS[1]));
    }
}
