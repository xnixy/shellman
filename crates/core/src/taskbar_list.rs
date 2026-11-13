//! What apps tell the taskbar about their buttons, as a shell that is not Explorer receives it.
//!
//! `ITaskbarList3` (implemented in explorerframe.dll, inside the calling app) does not talk to
//! Explorer by COM: it sends window messages to the shell's task window, the one registered with
//! `SetTaskmanWindow` (shellman's tray window, see the bar's `tray::host`). The numbers are not
//! documented; these are the ones the open ManagedShell (Cairo, RetroBar) handles in its
//! `TasksService`, with the app window in `wParam`:
//!
//! - `WM_USER + 64` SetProgressValue: `lParam` is the value scaled to 0..=65534;
//! - `WM_USER + 65` SetProgressState: `lParam` is the `TBPFLAG`;
//! - `WM_USER + 79` SetOverlayIcon: `lParam` is the `HICON` (0 to remove), valid while the call
//!   lasts, so it is copied at once;
//! - `WM_USER + 85` the overlay's description (a shared-memory string; not used here);
//! - `WM_USER + 60` MarkFullscreenWindow.
//!
//! Apps call these only after their button exists: Explorer tells each new window with the
//! registered message "TaskbarButtonCreated", and so must a shell that wants to hear them.
//! Flashing (`FlashWindowEx`) comes another way: as `HSHELL_FLASH` to every window registered
//! with `RegisterShellHookWindow`, and that works for any process.

pub const WM_USER: u32 = 0x0400;
pub const SET_PROGRESS_VALUE: u32 = WM_USER + 64;
pub const SET_PROGRESS_STATE: u32 = WM_USER + 65;
pub const SET_OVERLAY_ICON: u32 = WM_USER + 79;
pub const SET_OVERLAY_DESCRIPTION: u32 = WM_USER + 85;
/// The top of the progress scale explorerframe sends.
pub const PROGRESS_MAX: f32 = 65534.0;
/// `HSHELL_FLASH`: `HSHELL_REDRAW` with the high bit, the window in `lParam`.
pub const HSHELL_FLASH: usize = 0x8006;
pub const HSHELL_WINDOWACTIVATED: usize = 4;
pub const HSHELL_RUDEAPPACTIVATED: usize = 0x8004;
pub const HSHELL_WINDOWDESTROYED: usize = 2;

/// `TBPFLAG`, as a state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Progress {
    #[default]
    None,
    Indeterminate,
    Normal,
    Error,
    Paused,
}

impl Progress {
    /// From `TBPFLAG` (0, 1, 2, 4, 8). An unknown value counts as normal progress.
    pub fn from_flag(flag: usize) -> Self {
        match flag {
            0 => Progress::None,
            1 => Progress::Indeterminate,
            4 => Progress::Error,
            8 => Progress::Paused,
            _ => Progress::Normal,
        }
    }
}

/// One message about an app window's button.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Message {
    /// The fraction done, 0..=1.
    Value { hwnd: isize, done: f32 },
    State { hwnd: isize, state: Progress },
    /// The overlay icon's handle, 0 for none.
    Overlay { hwnd: isize, icon: isize },
}

/// Read a message sent to the task window, if it is one of these.
pub fn decode(msg: u32, wparam: usize, lparam: isize) -> Option<Message> {
    let hwnd = wparam as isize;
    if hwnd == 0 {
        return None;
    }
    Some(match msg {
        SET_PROGRESS_VALUE => Message::Value { hwnd, done: (lparam as f32 / PROGRESS_MAX).clamp(0.0, 1.0) },
        SET_PROGRESS_STATE => Message::State { hwnd, state: Progress::from_flag(lparam as usize) },
        SET_OVERLAY_ICON => Message::Overlay { hwnd, icon: lparam },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_decode() {
        assert_eq!(decode(WM_USER + 64, 0x1234, 32767), Some(Message::Value { hwnd: 0x1234, done: 32767.0 / 65534.0 }));
        assert_eq!(decode(WM_USER + 64, 7, 70000), Some(Message::Value { hwnd: 7, done: 1.0 }));
        assert_eq!(decode(WM_USER + 64, 7, -5), Some(Message::Value { hwnd: 7, done: 0.0 }));
        assert_eq!(decode(WM_USER + 65, 7, 8), Some(Message::State { hwnd: 7, state: Progress::Paused }));
        assert_eq!(decode(WM_USER + 79, 7, 0x99), Some(Message::Overlay { hwnd: 7, icon: 0x99 }));
        assert_eq!(decode(WM_USER + 85, 7, 1), None);
        assert_eq!(decode(WM_USER + 64, 0, 1), None);
        assert_eq!(decode(0x10, 7, 1), None);
    }

    #[test]
    fn progress_flags() {
        let all: Vec<Progress> = [0, 1, 2, 4, 8].into_iter().map(Progress::from_flag).collect();
        assert_eq!(all, [Progress::None, Progress::Indeterminate, Progress::Normal, Progress::Error, Progress::Paused]);
    }
}
