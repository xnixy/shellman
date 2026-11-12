//! Putting a bar window exactly on a rectangle of physical pixels.
//!
//! Bars are placed with Windows directly rather than through egui, whose positions are in points
//! of whichever monitor the window is on *before* the move, which is wrong as soon as monitors
//! have different scales.
//!
//! Moving a window onto a monitor with another scale makes Windows send it `WM_DPICHANGED`, and
//! the window library answers by resizing the window by the ratio of the two scales, undoing the
//! size just set. So the rectangle is checked after the move and set once more: the window is on
//! its monitor by then, and the second move does not change its scale. The window's own frames
//! then follow the new scale, so the bar draws at its monitor's size.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOOWNERZORDER,
};

/// Move and size `hwnd` to `rect`, keeping it above other windows. Does nothing if it is
/// already there.
pub fn put(hwnd: isize, rect: RECT) {
    let hwnd = HWND(hwnd as *mut _);
    for _ in 0..2 {
        let mut now = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut now) }.is_ok() && now == rect {
            return;
        }
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
    }
}
