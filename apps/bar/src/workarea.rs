//! Screen space for the bars when Explorer is not running.
//!
//! Without Explorer nobody keeps the list of app bars, so the work area (where windows maximize)
//! is set directly. It is put back to the whole screen when the bar exits.
//!
//! Explorer also hides minimized windows (they live on the taskbar); without a shell, Windows
//! draws them as small title bars at the bottom-left of the desktop. The same setting Explorer
//! uses hides them while the bar runs; it takes effect once the tray window is registered as the
//! shell's task window, and for windows minimized after that (at sign-in, that is all of them).

use windows::Win32::Foundation::RECT;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SystemParametersInfoW, MINIMIZEDMETRICS, SM_CXSCREEN, SM_CYSCREEN,
    SPI_GETMINIMIZEDMETRICS, SPI_SETMINIMIZEDMETRICS, SPI_SETWORKAREA,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

/// ARW_HIDE: minimized windows are not drawn on the desktop.
const ARW_HIDE: i32 = 0x0008;

/// SPIF_SENDCHANGE: tell running apps, so maximized windows resize at once.
const SEND_CHANGE: SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS = SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(2);

pub struct WorkArea {
    full: RECT,
    minimized: MINIMIZEDMETRICS,
}

impl WorkArea {
    /// Keep `top` and `bottom` physical pixels of the primary screen free of maximized windows.
    pub fn reserve(top: i32, bottom: i32) -> Self {
        let full = unsafe {
            RECT {
                left: 0,
                top: 0,
                right: GetSystemMetrics(SM_CXSCREEN),
                bottom: GetSystemMetrics(SM_CYSCREEN),
            }
        };
        set(RECT {
            top: full.top + top,
            bottom: full.bottom - bottom,
            ..full
        });
        let minimized = hide_minimized();
        Self { full, minimized }
    }
}

impl Drop for WorkArea {
    fn drop(&mut self) {
        set(self.full);
        let mut m = self.minimized;
        unsafe {
            let _ = SystemParametersInfoW(SPI_SETMINIMIZEDMETRICS, m.cbSize, Some((&raw mut m).cast()), SEND_CHANGE);
        }
    }
}

/// Hide minimized windows; returns the previous setting, to put back later.
fn hide_minimized() -> MINIMIZEDMETRICS {
    let mut m = MINIMIZEDMETRICS {
        cbSize: size_of::<MINIMIZEDMETRICS>() as u32,
        ..Default::default()
    };
    unsafe {
        let _ = SystemParametersInfoW(SPI_GETMINIMIZEDMETRICS, m.cbSize, Some((&raw mut m).cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        let before = m;
        m.iArrange.0 |= ARW_HIDE;
        let _ = SystemParametersInfoW(SPI_SETMINIMIZEDMETRICS, m.cbSize, Some((&raw mut m).cast()), SEND_CHANGE);
        before
    }
}

fn set(mut rect: RECT) {
    unsafe {
        let _ = SystemParametersInfoW(SPI_SETWORKAREA, 0, Some((&raw mut rect).cast()), SEND_CHANGE);
    }
}
