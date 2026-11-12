//! Resident windows out of sight cost as little as possible.
//!
//! The launcher, the control centre and the switcher stay resident with their window cloaked, so
//! they show at once. The graphics driver keeps a window's back buffers at the window's size, in
//! process memory and in video memory alike (on a 4K monitor at 300 % the launcher's are about
//! 2300 x 1700 pixels each). A cloaked window one pixel in size keeps one-pixel buffers; each
//! showing sets the full size again (all three place their window on every showing anyway).
//!
//! Measured with a 1350 x 1350 pixel window: 18 MB less committed memory and 22 MB less video
//! memory once shrunk.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOZORDER,
};

/// Shrink a cloaked window to one pixel, so its buffers go. Call only while it is out of sight.
pub fn shrink(hwnd: isize) {
    unsafe {
        let _ = SetWindowPos(
            HWND(hwnd as *mut _),
            None,
            0,
            0,
            1,
            1,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE,
        );
    }
}
