//! Reserving screen space for a bar, so maximized windows stop at its edge instead of sliding
//! under it.
//!
//! While Explorer runs, it keeps the list of such "app bars" and Windows asks it; this goes
//! through that (`SHAppBarMessage`). Once shellman is the shell nobody answers that message, and
//! the session sets the work area itself; that comes with the takeover.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::WM_APP;

/// A registered app bar; unregistered when dropped, which gives the space back.
pub struct AppBar {
    data: APPBARDATA,
}

impl AppBar {
    /// Reserve `height` physical pixels along the top of the rectangle `screen`.
    pub fn top(hwnd: isize, screen: RECT, height: i32) -> Option<Self> {
        let mut data = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: HWND(hwnd as *mut _),
            // Explorer sends its notices (fullscreen apps, position changes) with this message.
            // Fullscreen is detected separately, so nothing reads them yet.
            uCallbackMessage: WM_APP + 1,
            uEdge: ABE_TOP,
            ..Default::default()
        };
        if unsafe { SHAppBarMessage(ABM_NEW, &mut data) } == 0 {
            return None;
        }
        data.rc = RECT {
            left: screen.left,
            top: screen.top,
            right: screen.right,
            bottom: screen.top + height,
        };
        unsafe {
            // Ask where the bar may go (another bar may already hold the edge), then claim it.
            SHAppBarMessage(ABM_QUERYPOS, &mut data);
            data.rc.bottom = data.rc.top + height;
            SHAppBarMessage(ABM_SETPOS, &mut data);
        }
        Some(Self { data })
    }
}

impl Drop for AppBar {
    fn drop(&mut self) {
        unsafe {
            SHAppBarMessage(ABM_REMOVE, &mut self.data);
        }
    }
}
