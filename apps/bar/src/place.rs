//! Where the dock sits: centred at the bottom of the primary monitor's work area, which is the
//! screen minus Explorer's taskbar while it still runs.

use windows::Win32::Foundation::RECT;
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

/// Gap between the dock and the bottom of the work area, in points.
const GAP: f32 = 8.0;

/// The work area in physical pixels.
fn work_area() -> RECT {
    let mut r = RECT::default();
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&raw mut r).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    r
}

/// Top-left corner, in points, for a dock of `size` points at `ppp` pixels per point.
pub fn dock_origin(size: egui::Vec2, ppp: f32) -> egui::Pos2 {
    let wa = work_area();
    let (left, right, bottom) = (wa.left as f32 / ppp, wa.right as f32 / ppp, wa.bottom as f32 / ppp);
    egui::pos2(
        ((left + right) / 2.0 - size.x / 2.0).round(),
        (bottom - GAP - size.y).round(),
    )
}
