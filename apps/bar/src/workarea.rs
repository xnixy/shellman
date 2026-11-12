//! Screen space for the bars when Explorer is not running.
//!
//! Without Explorer nobody keeps the list of app bars, so the work area (where windows maximize)
//! is set directly, monitor by monitor: Windows applies a new work area to the monitor that holds
//! it. Each monitor's is put back to its whole screen when the bar exits or stops using it, but
//! only while that monitor is still there with the same size; a work area for a monitor that has
//! gone would otherwise land on whichever monitor is nearest.
//!
//! Explorer also hides minimized windows (they live on the taskbar); without a shell, Windows
//! draws them as small title bars at the bottom-left of the desktop. The same setting Explorer
//! uses hides them while the bar runs; it takes effect once the tray window is registered as the
//! shell's task window, and for windows minimized after that (at sign-in, that is all of them).

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONULL};
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, MINIMIZEDMETRICS, SPI_GETMINIMIZEDMETRICS, SPI_SETMINIMIZEDMETRICS,
    SPI_SETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

/// ARW_HIDE: minimized windows are not drawn on the desktop.
const ARW_HIDE: i32 = 0x0008;

/// SPIF_SENDCHANGE: tell running apps, so maximized windows resize at once.
const SEND_CHANGE: SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS = SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(2);

/// One monitor's work area, with space taken off its top and bottom.
pub struct WorkArea {
    monitor: RECT,
    wanted: RECT,
}

impl WorkArea {
    /// Keep `top` and `bottom` physical pixels of `monitor` free of maximized windows.
    pub fn reserve(monitor: RECT, top: i32, bottom: i32) -> Self {
        let wanted = RECT {
            top: monitor.top + top,
            bottom: monitor.bottom - bottom,
            ..monitor
        };
        set(wanted);
        Self { monitor, wanted }
    }

    /// Set it again if something else changed it. An Explorer that Winlogon starts for a moment
    /// (when the modern-app host dies, see `shellman_core::app_host`) sets every monitor's work
    /// area for its own taskbar, and nothing else would put shellman's back.
    pub fn reassert(&self) {
        if work_area(self.monitor).is_some_and(|now| now != self.wanted) {
            set(self.wanted);
        }
    }
}

/// The work area of the monitor with exactly this rectangle.
fn work_area(monitor: RECT) -> Option<RECT> {
    let handle = unsafe { MonitorFromRect(&monitor, MONITOR_DEFAULTTONULL) };
    if handle.is_invalid() {
        return None;
    }
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let ok = unsafe { GetMonitorInfoW(handle, &mut info) }.as_bool() && info.rcMonitor == monitor;
    ok.then_some(info.rcWork)
}

impl Drop for WorkArea {
    fn drop(&mut self) {
        if still_there(self.monitor) {
            set(self.monitor);
        }
    }
}

/// Is there still a monitor with exactly this rectangle?
fn still_there(monitor: RECT) -> bool {
    work_area(monitor).is_some()
}

fn set(mut rect: RECT) {
    unsafe {
        let _ = SystemParametersInfoW(SPI_SETWORKAREA, 0, Some((&raw mut rect).cast()), SEND_CHANGE);
    }
}

/// Minimized windows hidden while this lives; the previous setting comes back when dropped.
pub struct HiddenMinimized {
    before: MINIMIZEDMETRICS,
}

impl HiddenMinimized {
    pub fn start() -> Self {
        let mut m = MINIMIZEDMETRICS {
            cbSize: size_of::<MINIMIZEDMETRICS>() as u32,
            ..Default::default()
        };
        unsafe {
            let _ = SystemParametersInfoW(SPI_GETMINIMIZEDMETRICS, m.cbSize, Some((&raw mut m).cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
            let before = m;
            m.iArrange.0 |= ARW_HIDE;
            let _ = SystemParametersInfoW(SPI_SETMINIMIZEDMETRICS, m.cbSize, Some((&raw mut m).cast()), SEND_CHANGE);
            Self { before }
        }
    }
}

impl Drop for HiddenMinimized {
    fn drop(&mut self) {
        let mut m = self.before;
        unsafe {
            let _ = SystemParametersInfoW(SPI_SETMINIMIZEDMETRICS, m.cbSize, Some((&raw mut m).cast()), SEND_CHANGE);
        }
    }
}
