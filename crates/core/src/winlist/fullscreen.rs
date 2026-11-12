//! Is the window in front a fullscreen app (a game, a video, an F11 browser)? Then bars on that
//! monitor should step aside.
//!
//! "Fullscreen" means the window covers the whole monitor, not just the work area, so a normal
//! maximized window (which leaves the bars' reserved space free) is not fullscreen.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, IsZoomed,
    GWL_STYLE, WS_CAPTION,
};

/// Windows that cover the screen without being an app in fullscreen: the desktop and Explorer's
/// own taskbars.
const SHELL_CLASSES: [&str; 4] = ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"];

/// Does `window` reach every edge of `monitor`? Overhang does not matter: borderless windows
/// and maximized frames both stick out a few pixels.
pub fn covers(window: RECT, monitor: RECT) -> bool {
    window.left <= monitor.left
        && window.top <= monitor.top
        && window.right >= monitor.right
        && window.bottom >= monitor.bottom
        // An empty monitor rect (a failed query) must not count as covered by everything.
        && monitor.right > monitor.left
        && monitor.bottom > monitor.top
}

/// The decision, from facts already gathered: the front window's rectangle and class, whether it
/// belongs to the bar's own process, whether it is a maximized window that still has its title
/// bar, and the monitor's rectangle.
///
/// A maximized window with a title bar overhangs the monitor on every side, so by rectangle alone
/// it would look fullscreen whenever no space is reserved. It never is: F11 browsers and
/// borderless games drop the title bar.
pub fn is_fullscreen(window: RECT, class: &str, own: bool, maximized_framed: bool, monitor: RECT) -> bool {
    !own && !maximized_framed && !SHELL_CLASSES.contains(&class) && covers(window, monitor)
}

/// True when the foreground window is fullscreen on the monitor `bar` (a window handle) is on.
/// Called when the front window changed or moved, never on a timer.
pub fn foreground_covers_monitor_of(bar: isize) -> bool {
    unsafe {
        let front = GetForegroundWindow();
        if front.is_invalid() || !IsWindowVisible(front).as_bool() {
            return false;
        }
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let monitor = MonitorFromWindow(HWND(bar as *mut _), MONITOR_DEFAULTTOPRIMARY);
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return false;
        }
        let mut rect = RECT::default();
        if GetWindowRect(front, &mut rect).is_err() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(front, Some(&mut pid));
        let style = GetWindowLongPtrW(front, GWL_STYLE) as u32;
        let maximized_framed = IsZoomed(front).as_bool() && style & WS_CAPTION.0 == WS_CAPTION.0;
        is_fullscreen(rect, &super::class_name(front), pid == std::process::id(), maximized_framed, info.rcMonitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT { left, top, right, bottom }
    }
    const MONITOR: RECT = rect(0, 0, 1920, 1080);

    #[test]
    fn a_window_on_the_whole_monitor_is_fullscreen() {
        assert!(is_fullscreen(MONITOR, "UnityWndClass", false, false, MONITOR));
        // Borderless windows often overhang by a pixel or a frame.
        assert!(is_fullscreen(rect(-1, -1, 1921, 1081), "Chrome_WidgetWin_1", false, false, MONITOR));
    }

    #[test]
    fn a_maximized_window_is_not() {
        // It stops at the work area: below the top bar, above the dock's space.
        assert!(!is_fullscreen(rect(-8, 24, 1928, 1000), "Notepad", false, false, MONITOR));
        // Even one that leaves only the top bar's strip free.
        assert!(!is_fullscreen(rect(0, 32, 1920, 1080), "Notepad", false, false, MONITOR));
    }

    #[test]
    fn a_maximized_window_with_a_title_bar_never_is() {
        // With no space reserved its frame overhangs the monitor on every side.
        assert!(!is_fullscreen(rect(-8, -8, 1928, 1088), "Notepad", false, true, MONITOR));
        // The same rectangle without a title bar (an F11 browser, a borderless game) is.
        assert!(is_fullscreen(rect(-8, -8, 1928, 1088), "Chrome_WidgetWin_1", false, false, MONITOR));
    }

    #[test]
    fn the_desktop_the_shell_and_the_bars_are_not() {
        for class in SHELL_CLASSES {
            assert!(!is_fullscreen(MONITOR, class, false, false, MONITOR), "{class}");
        }
        assert!(!is_fullscreen(MONITOR, "Anything", true, false, MONITOR), "own window");
    }

    #[test]
    fn another_monitor_or_a_failed_query_is_not() {
        let second = rect(1920, 0, 3840, 1080);
        assert!(!is_fullscreen(MONITOR, "Game", false, false, second));
        assert!(!is_fullscreen(MONITOR, "Game", false, false, RECT::default()));
    }
}
