//! Screen space for the bars when Explorer is not running.
//!
//! Without Explorer nobody keeps the list of app bars, so the work area (where windows maximize)
//! is set directly. It is put back to the whole screen when the bar exits.

use windows::Win32::Foundation::RECT;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SystemParametersInfoW, SM_CXSCREEN, SM_CYSCREEN, SPI_SETWORKAREA,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

/// SPIF_SENDCHANGE: tell running apps, so maximized windows resize at once.
const SEND_CHANGE: SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS = SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(2);

pub struct WorkArea {
    full: RECT,
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
        Self { full }
    }
}

impl Drop for WorkArea {
    fn drop(&mut self) {
        set(self.full);
    }
}

fn set(mut rect: RECT) {
    unsafe {
        let _ = SystemParametersInfoW(SPI_SETWORKAREA, 0, Some((&raw mut rect).cast()), SEND_CHANGE);
    }
}
