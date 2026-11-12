//! Reserving screen space for a bar, so maximized windows stop at its edge instead of sliding
//! under it.
//!
//! While Explorer runs, it keeps the list of such "app bars" and Windows asks it; this goes
//! through that (`SHAppBarMessage`). Once shellman is the shell nobody answers that message, and
//! the work area is set directly instead (`workarea.rs`).
//!
//! Each monitor's top bar and dock register separately, along the top and bottom edges of their
//! own monitor; Explorer keeps them apart from its own taskbars.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABE_BOTTOM, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::WM_APP;

/// Explorer sends its notices (another bar moved, a fullscreen app) with this message. Only
/// "positions changed" is acted on, by asking for the place again.
pub const CALLBACK: u32 = WM_APP + 2;
/// ABN_POSCHANGED, the notice's wParam.
pub const POS_CHANGED: usize = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    Top,
    Bottom,
}

/// A registered app bar; unregistered when dropped, which gives the space back.
///
/// Only the window handle (as a number) and the rectangle are kept, so it can live in state
/// shared between the bar's windows.
pub struct AppBar {
    hwnd: isize,
    edge: Edge,
    thickness: i32,
    /// The proposed rectangle: the full strip along the monitor's edge.
    want: RECT,
    /// Where Explorer let it go.
    pub granted: RECT,
}

impl AppBar {
    /// Reserve `want` (a strip along `edge` of a monitor, in physical pixels) for the window
    /// `hwnd`. None when nobody keeps app bars (Explorer is not running).
    pub fn new(hwnd: isize, edge: Edge, want: RECT) -> Option<Self> {
        let mut data = Self::data(hwnd, edge);
        if unsafe { SHAppBarMessage(ABM_NEW, &mut data) } == 0 {
            return None;
        }
        let mut bar = Self {
            hwnd,
            edge,
            thickness: want.bottom - want.top,
            want,
            granted: want,
        };
        bar.granted = bar.claim();
        Some(bar)
    }

    fn data(hwnd: isize, edge: Edge) -> APPBARDATA {
        APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: HWND(hwnd as *mut _),
            uCallbackMessage: CALLBACK,
            uEdge: match edge {
                Edge::Top => ABE_TOP,
                Edge::Bottom => ABE_BOTTOM,
            },
            ..Default::default()
        }
    }

    /// Ask where the bar may go (another bar may already hold the edge), then claim it.
    fn claim(&self) -> RECT {
        let mut data = Self::data(self.hwnd, self.edge);
        data.rc = self.want;
        unsafe {
            SHAppBarMessage(ABM_QUERYPOS, &mut data);
            match self.edge {
                Edge::Top => data.rc.bottom = data.rc.top + self.thickness,
                Edge::Bottom => data.rc.top = data.rc.bottom - self.thickness,
            }
            SHAppBarMessage(ABM_SETPOS, &mut data);
        }
        data.rc
    }

    /// Explorer said positions changed: ask again. Returns true if the bar's place moved. The
    /// place is claimed again only when it differs, so bars answering each other's notices
    /// settle instead of echoing forever.
    pub fn requery(&mut self) -> bool {
        let mut data = Self::data(self.hwnd, self.edge);
        data.rc = self.want;
        unsafe {
            SHAppBarMessage(ABM_QUERYPOS, &mut data);
        }
        match self.edge {
            Edge::Top => data.rc.bottom = data.rc.top + self.thickness,
            Edge::Bottom => data.rc.top = data.rc.bottom - self.thickness,
        }
        if data.rc == self.granted {
            return false;
        }
        self.granted = self.claim();
        true
    }
}

impl Drop for AppBar {
    fn drop(&mut self) {
        let mut data = Self::data(self.hwnd, self.edge);
        unsafe {
            SHAppBarMessage(ABM_REMOVE, &mut data);
        }
    }
}
