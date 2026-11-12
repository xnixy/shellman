//! Appearing on request, and only one launcher at a time.
//!
//! The process stays resident with its window out of sight (cloaked; see `hide`). Running
//! `shellman-launcher.exe` again finds the first one through a named mutex, signals it through a
//! named event, and exits at once; the resident process then shows its window (or hides it, if it
//! is already showing).
//! Showing is a few Win32 calls on a window that already exists, so there is no start-up cost
//! at the moment of summoning.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use windows::core::HSTRING;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, HMONITOR, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject, INFINITE,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use shellman_ui::glass;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, GetCursorPos, GetForegroundWindow, GetWindowRect, IsIconic,
    IsWindowVisible, GetWindowLongPtrW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, ASFW_ANY, GWL_EXSTYLE, GWL_STYLE, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE, WS_CAPTION, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
};

/// The panel's size in points, where its centre sits in the work area (a little above the middle),
/// and the margin it keeps from the work area's edges.
pub const WIDTH: f32 = 760.0;
pub const HEIGHT: f32 = 560.0;
const CENTRE_FRACTION: f64 = 0.46;
const MARGIN: f32 = 24.0;
/// The panel's corner radius in points.
pub const RADIUS: f32 = 24.0;

/// Whether this process is the resident launcher; if not, the resident one has been signalled
/// and this process has nothing more to do.
pub enum Claim {
    Resident(Instance),
    Signalled,
}

/// The resident launcher's end of the signal.
pub struct Instance {
    event: HANDLE,
    /// Keeps the mutex alive for as long as the process runs.
    _mutex: HANDLE,
}

/// What the window and the signal thread tell each other.
#[derive(Default)]
pub struct Shared {
    /// The signal thread asks the window to hide itself (it owns the state to reset).
    hide_requested: AtomicBool,
    /// Counts showings, so the window notices a new one.
    shown: AtomicU32,
    /// Whether the panel is up, as this launcher last set it.
    showing: AtomicBool,
}

impl Shared {
    /// The window hid itself.
    pub fn mark_hidden(&self) {
        self.showing.store(false, Ordering::Release);
    }

    pub fn is_showing(&self) -> bool {
        self.showing.load(Ordering::Acquire)
    }

    pub fn take_hide_request(&self) -> bool {
        self.hide_requested.swap(false, Ordering::AcqRel)
    }

    pub fn shown_count(&self) -> u32 {
        self.shown.load(Ordering::Acquire)
    }
}

/// A kernel object's name. `SHELLMAN_INSTANCE` is added to it, so a test build can run next to the
/// real launcher without the two finding each other.
fn name(what: &str) -> HSTRING {
    let instance = std::env::var("SHELLMAN_INSTANCE").unwrap_or_default();
    HSTRING::from(format!("Local\\shellman-launcher-{what}{instance}"))
}

/// Become the resident launcher, or signal the one that is. Both sides create the event before
/// the mutex, so whichever order two starts interleave in, the one that loses the mutex finds
/// the event.
pub fn claim(background: bool) -> Claim {
    unsafe {
        let Ok(event) = CreateEventW(None, false, false, &name("summon")) else {
            // Cannot coordinate; run on our own rather than not at all.
            return Claim::Resident(Instance {
                event: HANDLE::default(),
                _mutex: HANDLE::default(),
            });
        };
        let mutex = CreateMutexW(None, false, &name("resident"));
        let taken = mutex.is_ok() && GetLastError() == ERROR_ALREADY_EXISTS;
        if taken {
            // The resident process may take the foreground for the window it is about to show.
            // A `--background` start only asks for a resident to exist: it is there, so no
            // signal (which would pop the panel open).
            if !background {
                let _ = AllowSetForegroundWindow(ASFW_ANY);
                let _ = SetEvent(event);
            }
            return Claim::Signalled;
        }
        Claim::Resident(Instance {
            event,
            _mutex: mutex.unwrap_or_default(),
        })
    }
}

impl Instance {
    /// Show the window now, as if the launcher had been run again.
    pub fn summon(&self) {
        if !self.event.is_invalid() {
            unsafe {
                let _ = SetEvent(self.event);
            }
        }
    }

    /// Serve summons for the window `hwnd` from a thread of its own, until the process ends.
    pub fn serve(&self, hwnd: isize, ctx: egui::Context, shared: Arc<Shared>) {
        if self.event.is_invalid() {
            return;
        }
        let event = self.event.0 as isize;
        std::thread::spawn(move || {
            let event = HANDLE(event as *mut _);
            let hwnd = HWND(hwnd as *mut _);
            // The event wakes this thread, so nothing polls while the launcher is hidden.
            while unsafe { WaitForSingleObject(event, INFINITE) }.0 == 0 {
                // Toggle on what this launcher last did, not on `IsWindowVisible`: after a
                // `--background` start the window library shows the window once by itself.
                if shared.showing.load(Ordering::Acquire) {
                    shared.hide_requested.store(true, Ordering::Release);
                } else {
                    // Count the showing first: a frame run by the show itself must already know.
                    shared.showing.store(true, Ordering::Release);
                    shared.shown.fetch_add(1, Ordering::AcqRel);
                    show(hwnd);
                }
                ctx.request_repaint();
            }
        });
    }
}

/// Whether the window is on screen right now (shown and not cloaked).
pub fn is_visible(hwnd: isize) -> bool {
    unsafe { IsWindowVisible(HWND(hwnd as *mut _)) }.as_bool() && !glass::is_cloaked(hwnd)
}

/// Set the window up as resident and out of sight: cloaked, and a tool window, so it is never
/// listed as a running app. Called once, before the window library first shows it.
pub fn prepare(hwnd: isize) {
    glass::cloak(hwnd, true);
    tool_window(HWND(hwnd as *mut _));
}

/// Put the window out of sight. Called by the window itself.
///
/// Out of sight means cloaked, not hidden: a hidden window gets no paint messages, so a frame
/// egui had already asked for (the caret's next blink, say) never comes, and the window library
/// waits for it on a whole core until the next showing. Hiding and showing again without
/// activation hands the keyboard back to the window behind.
pub fn hide(hwnd: isize) {
    glass::cloak(hwnd, true);
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
}

/// Place the panel on the monitor of the window in front (else the one with the cursor), and
/// show it with the keyboard focus.
fn show(hwnd: HWND) {
    unsafe {
        let monitor = front_monitor();
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let (mut dpi_x, mut dpi_y) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let scale = dpi_x as f32 / 96.0;
        let (x, y, w, h) = panel_rect(info.rcWork, scale);
        // Placed while still cloaked, so it is never seen where it was or at a passing size.
        glass::cloak(hwnd.0 as isize, true);
        strip_frame(hwnd);
        tool_window(hwnd);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            x,
            y,
            w,
            h,
            SWP_SHOWWINDOW | SWP_FRAMECHANGED,
        );
        // Moving onto a monitor of another scale makes the window library resize the window to
        // keep its old size in points (and move it to match), so the place is set once more.
        let mut now = RECT::default();
        let wanted = RECT { left: x, top: y, right: x + w, bottom: y + h };
        if GetWindowRect(hwnd, &mut now).is_ok() && now != wanted {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
        }
        glass::cloak(hwnd.0 as isize, false);
        take_foreground(hwnd);
    }
}

/// A tool window, never an app window: the dock and the window list leave it out. (The window
/// library's "no taskbar button" only removes the button.)
fn tool_window(hwnd: HWND) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let tool = (ex & !WS_EX_APPWINDOW.0) | WS_EX_TOOLWINDOW.0;
        if tool != ex {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, tool as isize);
        }
    }
}

/// Give `hwnd` the foreground and the keyboard. Windows refuses that to a process that did not
/// get the last input, and the summoner here is a click on a bar that never activates. So when
/// the first request is refused, send one press of a key no keyboard has (which makes this
/// process the last input's sender; the switcher does the same) and ask again.
fn take_foreground(hwnd: HWND) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    unsafe {
        let _ = SetForegroundWindow(hwnd);
        if GetForegroundWindow() == hwnd {
            return;
        }
        let key = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0xE8), dwFlags: flags, ..Default::default() },
            },
        };
        SendInput(&[key(Default::default()), key(KEYEVENTF_KEYUP)], size_of::<INPUT>() as i32);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// No caption, system menu or sizing frame: over glass, Windows would otherwise draw its
/// minimize, maximize and close buttons into the panel. Done on every showing, since the window
/// library re-applies its own styles now and then.
fn strip_frame(hwnd: HWND) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let bare = style
            & !(WS_CAPTION.0 | WS_SYSMENU.0 | WS_THICKFRAME.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0);
        SetWindowLongPtrW(hwnd, GWL_STYLE, bare as isize);
    }
}

/// The panel's rectangle in pixels within a monitor's work area: centred across, centred down with
/// a slight upward bias, always inside the work area with a margin; on a short screen the panel
/// gets shorter rather than reach the top bar or the dock.
fn panel_rect(work: RECT, scale: f32) -> (i32, i32, i32, i32) {
    let (work_w, work_h) = (work.right - work.left, work.bottom - work.top);
    let margin = (MARGIN * scale) as i32;
    let w = ((WIDTH * scale) as i32).min((work_w - 2 * margin).max(1));
    let h = ((HEIGHT * scale) as i32).min((work_h - 2 * margin).max(1));
    let x = work.left + (work_w - w) / 2;
    let centre = work.top + (work_h as f64 * CENTRE_FRACTION) as i32;
    let y = (centre - h / 2).clamp(work.top + margin.min(work_h - h), work.bottom - h - margin.min(work_h - h));
    (x, y, w, h)
}

/// The monitor under the mouse pointer: where the click that summoned the launcher happened. Only
/// when the pointer can't be read, the monitor of the window in front.
unsafe fn front_monitor() -> HMONITOR {
    let mut at = POINT::default();
    if GetCursorPos(&mut at).is_ok() {
        return MonitorFromPoint(at, MONITOR_DEFAULTTONEAREST);
    }
    let front = GetForegroundWindow();
    if !front.is_invalid() && !IsIconic(front).as_bool() {
        return MonitorFromWindow(front, MONITOR_DEFAULTTONEAREST);
    }
    MonitorFromPoint(at, MONITOR_DEFAULTTONEAREST)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(work: RECT, r: (i32, i32, i32, i32)) -> bool {
        r.0 >= work.left && r.1 >= work.top && r.0 + r.2 <= work.right && r.1 + r.3 <= work.bottom
    }

    #[test]
    fn primary_150_percent() {
        let work = RECT { left: 0, top: 48, right: 3840, bottom: 2043 };
        let r = panel_rect(work, 1.5);
        assert_eq!((r.2, r.3), (1140, 840));
        assert_eq!(r.0, (3840 - 1140) / 2);
        assert_eq!(r.1 + r.3 / 2, 48 + (1995.0 * 0.46) as i32);
        assert!(inside(work, r));
    }

    #[test]
    fn left_monitor_is_centred_in_its_own_work_area() {
        let work = RECT { left: -3840, top: 48, right: 0, bottom: 2043 };
        let r = panel_rect(work, 1.5);
        assert_eq!(r.0, -3840 + (3840 - 1140) / 2);
        assert!(inside(work, r));
    }

    #[test]
    fn monitor_300_percent_keeps_full_height_inside() {
        let work = RECT { left: 3746, top: 2256, right: 7586, bottom: 4086 };
        let r = panel_rect(work, 3.0);
        assert_eq!((r.2, r.3), (2280, 1680));
        assert_eq!(r.0, 3746 + (3840 - 2280) / 2);
        assert!(inside(work, r));
        assert!(r.1 - work.top >= 72 && work.bottom - (r.1 + r.3) >= 72);
    }

    #[test]
    fn short_screen_shrinks_the_panel_to_fit() {
        let work = RECT { left: 0, top: 0, right: 1000, bottom: 500 };
        let r = panel_rect(work, 1.5);
        assert_eq!(r.2, 1000 - 72);
        assert_eq!(r.3, 500 - 72);
        assert!(inside(work, r));
    }
}
