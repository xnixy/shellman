//! Appearing on request, and only one launcher at a time.
//!
//! The process stays resident with its window hidden. Running `shellman-launcher.exe` again
//! finds the first one through a named mutex, signals it through a named event, and exits at
//! once; the resident process then shows its window (or hides it, if it is already showing).
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
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, GetCursorPos, GetForegroundWindow, IsIconic, IsWindowVisible,
    GetWindowLongPtrW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    ASFW_ANY, GWL_STYLE, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_SHOWWINDOW, SW_HIDE, WS_CAPTION,
    WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
};

/// The panel's size in points, and how far down the screen's work area it starts.
pub const WIDTH: f32 = 760.0;
pub const HEIGHT: f32 = 560.0;
const TOP_FRACTION: f64 = 0.14;

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
}

impl Shared {
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
pub fn claim() -> Claim {
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
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            let _ = SetEvent(event);
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
                if unsafe { IsWindowVisible(hwnd) }.as_bool() {
                    shared.hide_requested.store(true, Ordering::Release);
                } else {
                    show(hwnd);
                    shared.shown.fetch_add(1, Ordering::AcqRel);
                }
                ctx.request_repaint();
            }
        });
    }
}

/// Hide the window. Called by the window itself.
pub fn hide(hwnd: isize) {
    unsafe {
        let _ = ShowWindow(HWND(hwnd as *mut _), SW_HIDE);
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
        strip_frame(hwnd);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            x,
            y,
            w,
            h,
            SWP_SHOWWINDOW | SWP_FRAMECHANGED,
        );
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

/// The panel's rectangle in pixels within a monitor's work area: centred across, in the upper
/// middle, and never taller than most of the screen.
fn panel_rect(work: RECT, scale: f32) -> (i32, i32, i32, i32) {
    let (work_w, work_h) = (work.right - work.left, work.bottom - work.top);
    let w = ((WIDTH * scale) as i32).min(work_w);
    let h = ((HEIGHT * scale) as i32).min((work_h as f64 * 0.8) as i32);
    let x = work.left + (work_w - w) / 2;
    let y = work.top + (work_h as f64 * TOP_FRACTION) as i32;
    (x, y, w, h)
}

unsafe fn front_monitor() -> HMONITOR {
    let front = GetForegroundWindow();
    if !front.is_invalid() && !IsIconic(front).as_bool() {
        return MonitorFromWindow(front, MONITOR_DEFAULTTONEAREST);
    }
    let mut at = POINT::default();
    let _ = GetCursorPos(&mut at);
    MonitorFromPoint(at, MONITOR_DEFAULTTONEAREST)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_is_centred_in_the_upper_middle() {
        let work = RECT {
            left: 1920,
            top: 0,
            right: 1920 + 2560,
            bottom: 1400,
        };
        let (x, y, w, h) = panel_rect(work, 1.0);
        assert_eq!((w, h), (760, 560));
        assert_eq!(x, 1920 + (2560 - 760) / 2);
        assert!(y > 100 && y < 300);
    }

    #[test]
    fn panel_scales_and_fits_small_screens() {
        let work = RECT {
            left: 0,
            top: 0,
            right: 1000,
            bottom: 500,
        };
        let (_, _, w, h) = panel_rect(work, 1.5);
        assert_eq!(w, 1000);
        assert_eq!(h, 400);
    }
}
