//! Mouse and keyboard settings: pointer speed and precision, the primary button, the wheel,
//! the double-click time, and the keyboard's repeat delay and rate. Reads and writes are
//! `SystemParametersInfoW`, written with `SPIF_UPDATEINIFILE | SPIF_SENDCHANGE` so they persist
//! and every window hears `WM_SETTINGCHANGE`. A write can wait on a hung window (the broadcast),
//! so callers make it from a worker; `apply_async` does that.
//!
//! The conversions (what a step of the repeat rate is in characters per second, the wheel's
//! "one screen" value) are plain functions with tests.

use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SystemParametersInfoW, SM_SWAPBUTTON, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE,
    SPI_GETKEYBOARDDELAY, SPI_GETKEYBOARDSPEED, SPI_GETMOUSE, SPI_GETMOUSESPEED,
    SPI_GETWHEELSCROLLCHARS, SPI_GETWHEELSCROLLLINES, SPI_SETDOUBLECLICKTIME, SPI_SETKEYBOARDDELAY,
    SPI_SETKEYBOARDSPEED, SPI_SETMOUSE, SPI_SETMOUSEBUTTONSWAP, SPI_SETMOUSESPEED,
    SPI_SETWHEELSCROLLCHARS, SPI_SETWHEELSCROLLLINES, SYSTEM_PARAMETERS_INFO_ACTION,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

/// `WHEEL_PAGESCROLL`: the wheel scrolls one screen per notch.
pub const WHEEL_PAGE: u32 = u32::MAX;
pub const SPEED_RANGE: (u32, u32) = (1, 20);
pub const WHEEL_LINES_RANGE: (u32, u32) = (1, 100);
pub const WHEEL_CHARS_RANGE: (u32, u32) = (1, 100);
pub const DOUBLE_CLICK_RANGE: (u32, u32) = (200, 900);
/// Repeat delay steps: 0 is 250 ms, 3 is 1 s.
pub const DELAY_STEPS: u32 = 4;
/// Repeat rate steps: 0 is about 2.5 characters a second, 31 about 30.
pub const RATE_STEPS: u32 = 32;

/// Everything this page edits, as Windows holds it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pointer {
    /// 1 to 20; 10 is Windows' default.
    pub speed: u32,
    /// "Enhance pointer precision" (mouse acceleration).
    pub precision: bool,
    /// The right button is the primary one.
    pub swapped: bool,
    /// Lines per wheel notch, or [`WHEEL_PAGE`].
    pub wheel_lines: u32,
    /// Characters per notch of a tilt wheel.
    pub wheel_chars: u32,
    pub double_click_ms: u32,
    /// 0 to 3.
    pub repeat_delay: u32,
    /// 0 to 31.
    pub repeat_rate: u32,
}

/// One change; applying it is one `SystemParametersInfoW` call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    Speed(u32),
    Precision(bool),
    Swapped(bool),
    WheelLines(u32),
    WheelChars(u32),
    DoubleClick(u32),
    RepeatDelay(u32),
    RepeatRate(u32),
}

/// The three values `SPI_SETMOUSE` takes: two thresholds and the acceleration level. Windows'
/// own "enhance pointer precision" is (6, 10, 1); off is zeros.
pub fn mouse_triple(precision: bool) -> [i32; 3] {
    if precision {
        [6, 10, 1]
    } else {
        [0, 0, 0]
    }
}

pub fn clamp(value: u32, (lo, hi): (u32, u32)) -> u32 {
    value.clamp(lo, hi)
}

/// The repeat delay of a step, in milliseconds.
pub fn delay_ms(step: u32) -> u32 {
    250 * (step.min(DELAY_STEPS - 1) + 1)
}

/// The repeat rate of a step, in characters a second (Windows documents 2.5 to 30 over 0..=31).
pub fn rate_per_second(step: u32) -> f32 {
    2.5 + step.min(RATE_STEPS - 1) as f32 * (30.0 - 2.5) / (RATE_STEPS - 1) as f32
}

/// Text for the repeat rate.
pub fn rate_label(step: u32) -> String {
    let r = rate_per_second(step);
    if (r - r.round()).abs() < 0.05 {
        format!("{} per second", r.round() as u32)
    } else {
        format!("{r:.1} per second")
    }
}

pub fn wheel_label(lines: u32) -> String {
    match lines {
        WHEEL_PAGE => "One screen".to_owned(),
        1 => "1 line".to_owned(),
        n => format!("{n} lines"),
    }
}

/// A value's place from 0 to 1 over `range`.
pub fn fraction(value: u32, (lo, hi): (u32, u32)) -> f32 {
    if hi <= lo {
        return 0.0;
    }
    (value.clamp(lo, hi) - lo) as f32 / (hi - lo) as f32
}

/// The whole value at place `f` (0 to 1) over `range`.
pub fn value_at(f: f32, (lo, hi): (u32, u32)) -> u32 {
    lo + (f.clamp(0.0, 1.0) * (hi - lo) as f32).round() as u32
}

fn flags() -> SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS {
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(SPIF_UPDATEINIFILE.0 | SPIF_SENDCHANGE.0)
}

fn get_u32(action: SYSTEM_PARAMETERS_INFO_ACTION, fallback: u32) -> u32 {
    let mut v: u32 = 0;
    // SAFETY: `v` outlives the call and is the size these actions write.
    let ok = unsafe {
        SystemParametersInfoW(action, 0, Some(&mut v as *mut u32 as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0))
    };
    if ok.is_ok() {
        v
    } else {
        fallback
    }
}

/// Read every value from Windows now. A value Windows does not report keeps its default.
pub fn read() -> Pointer {
    let mut triple = [0i32; 3];
    // SAFETY: the array is three ints, as SPI_GETMOUSE writes.
    let have = unsafe {
        SystemParametersInfoW(SPI_GETMOUSE, 0, Some(triple.as_mut_ptr() as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0))
    }
    .is_ok();
    Pointer {
        speed: clamp(get_u32(SPI_GETMOUSESPEED, 10), SPEED_RANGE),
        precision: have && triple[2] != 0,
        // SAFETY: plain queries.
        swapped: unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0,
        wheel_lines: get_u32(SPI_GETWHEELSCROLLLINES, 3),
        wheel_chars: get_u32(SPI_GETWHEELSCROLLCHARS, 3),
        double_click_ms: unsafe { GetDoubleClickTime() },
        repeat_delay: get_u32(SPI_GETKEYBOARDDELAY, 1).min(DELAY_STEPS - 1),
        repeat_rate: get_u32(SPI_GETKEYBOARDSPEED, 31).min(RATE_STEPS - 1),
    }
}

/// Write one change. True when Windows took it. Blocks while windows answer the broadcast.
pub fn apply(change: Change) -> bool {
    let set = |action, ui: u32, pv: Option<*mut core::ffi::c_void>| {
        // SAFETY: `pv` is null or points at memory the caller keeps alive for the call.
        unsafe { SystemParametersInfoW(action, ui, pv, flags()) }.is_ok()
    };
    match change {
        // The speed travels in the pointer argument itself.
        Change::Speed(v) => set(SPI_SETMOUSESPEED, 0, Some(clamp(v, SPEED_RANGE) as usize as *mut _)),
        Change::Precision(on) => {
            let mut t = mouse_triple(on);
            set(SPI_SETMOUSE, 0, Some(t.as_mut_ptr() as *mut _))
        }
        Change::Swapped(on) => set(SPI_SETMOUSEBUTTONSWAP, on as u32, None),
        Change::WheelLines(v) => set(SPI_SETWHEELSCROLLLINES, if v == WHEEL_PAGE { v } else { clamp(v, WHEEL_LINES_RANGE) }, None),
        Change::WheelChars(v) => set(SPI_SETWHEELSCROLLCHARS, clamp(v, WHEEL_CHARS_RANGE), None),
        Change::DoubleClick(v) => set(SPI_SETDOUBLECLICKTIME, clamp(v, DOUBLE_CLICK_RANGE), None),
        Change::RepeatDelay(v) => set(SPI_SETKEYBOARDDELAY, v.min(DELAY_STEPS - 1), None),
        Change::RepeatRate(v) => set(SPI_SETKEYBOARDSPEED, v.min(RATE_STEPS - 1), None),
    }
}

/// [`apply`] on its own thread, so the page never waits for the broadcast.
pub fn apply_async(change: Change) {
    std::thread::spawn(move || {
        apply(change);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Changes every value, reads it back, and puts the original back. Touches the real system
    /// settings, so it is run by hand: `cargo test -p shellman-core pointer -- --ignored`.
    #[test]
    #[ignore]
    fn every_setter_takes_and_restores() {
        let orig = read();
        let other = Pointer {
            speed: if orig.speed == 9 { 8 } else { 9 },
            precision: !orig.precision,
            swapped: !orig.swapped,
            wheel_lines: if orig.wheel_lines == 4 { 5 } else { 4 },
            wheel_chars: if orig.wheel_chars == 4 { 5 } else { 4 },
            double_click_ms: if orig.double_click_ms == 450 { 460 } else { 450 },
            repeat_delay: if orig.repeat_delay == 2 { 1 } else { 2 },
            repeat_rate: if orig.repeat_rate == 20 { 21 } else { 20 },
        };
        let all = |t: &Pointer| {
            [
                Change::Speed(t.speed),
                Change::Precision(t.precision),
                Change::Swapped(t.swapped),
                Change::WheelLines(t.wheel_lines),
                Change::WheelChars(t.wheel_chars),
                Change::DoubleClick(t.double_click_ms),
                Change::RepeatDelay(t.repeat_delay),
                Change::RepeatRate(t.repeat_rate),
            ]
        };
        let took: Vec<bool> = all(&other).into_iter().map(apply).collect();
        let changed = read();
        let back: Vec<bool> = all(&orig).into_iter().map(apply).collect();
        let restored = read();
        println!("orig {orig:?}\nchanged {changed:?} took {took:?}\nrestored {restored:?} back {back:?}");
        assert_eq!(restored, orig);
        assert_eq!(changed, other);
    }

    #[test]
    fn precision_is_windows_triple() {
        assert_eq!(mouse_triple(true), [6, 10, 1]);
        assert_eq!(mouse_triple(false), [0, 0, 0]);
    }

    #[test]
    fn delay_steps_are_quarter_seconds() {
        assert_eq!((0..4).map(delay_ms).collect::<Vec<_>>(), [250, 500, 750, 1000]);
        assert_eq!(delay_ms(99), 1000);
    }

    #[test]
    fn rate_spans_2_5_to_30() {
        assert_eq!(rate_per_second(0), 2.5);
        assert_eq!(rate_per_second(31), 30.0);
        assert_eq!(rate_label(31), "30 per second");
        assert_eq!(rate_label(0), "2.5 per second");
        assert_eq!(rate_per_second(99), 30.0);
    }

    #[test]
    fn slider_mapping_round_trips() {
        for v in 1..=20 {
            assert_eq!(value_at(fraction(v, SPEED_RANGE), SPEED_RANGE), v);
        }
        assert_eq!(value_at(2.0, SPEED_RANGE), 20);
        assert_eq!(fraction(5000, DOUBLE_CLICK_RANGE), 1.0);
        assert_eq!(fraction(1, (1, 1)), 0.0);
    }

    #[test]
    fn wheel_labels() {
        assert_eq!(wheel_label(WHEEL_PAGE), "One screen");
        assert_eq!(wheel_label(1), "1 line");
        assert_eq!(wheel_label(3), "3 lines");
    }
}
