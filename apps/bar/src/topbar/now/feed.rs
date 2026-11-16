//! What the app windows tell the taskbar (`dock::tasks`), as the "now" pill's inputs (`pick::Task`).
//!
//! Only windows the dock lists count (as Explorer's taskbar has it). Progress on Windows' own
//! file operation dialog (`OperationStatusWindow`, e.g. a copy started in shellman Files) is
//! named by the work the dialog's process said it has under way (`fsops::busy`, read from all
//! its top-level windows, listed or not): "Copying", "Moving", "Deleting"; anything else by its
//! app's name.

use super::pick::Task;
use crate::dock::tasks;
use shellman_core::fsops::busy;
use shellman_core::system::app_name;
use shellman_core::taskbar_list::Progress;
use shellman_core::winlist::AppWindow;
use std::time::Instant;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowThreadProcessId};

fn pid_of(hwnd: isize) -> u32 {
    let mut pid = 0u32;
    // SAFETY: a stale handle only leaves the pid at 0.
    unsafe { GetWindowThreadProcessId(HWND(hwnd as *mut _), Some(&mut pid)) };
    pid
}

fn is_file_dialog(hwnd: isize) -> bool {
    let mut buf = [0u16; 64];
    // SAFETY: the buffer's length is passed.
    let n = unsafe { GetClassNameW(HWND(hwnd as *mut _), &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n.min(buf.len())]) == "OperationStatusWindow"
}

/// The listed windows that show progress or ask for attention.
pub fn tasks(windows: &[AppWindow]) -> Vec<Task> {
    let known = tasks::snapshot();
    let now = Instant::now();
    windows
        .iter()
        .filter_map(|w| {
            let k = known.get(&w.hwnd)?;
            if k.progress == Progress::None && k.attention.is_none() {
                return None;
            }
            let verb = (k.progress != Progress::None && is_file_dialog(w.hwnd))
                .then(|| busy::read_process(pid_of(w.hwnd)).verb())
                .flatten();
            Some(Task {
                hwnd: w.hwnd,
                name: app_name::display_name(&w.app_id, w.exe.as_deref()),
                verb,
                progress: k.progress,
                done: k.done,
                progress_for: k.progress_since.map_or_else(Default::default, |t| now.saturating_duration_since(t)),
                attention_for: k.attention.map(|t| now.saturating_duration_since(t)),
            })
        })
        .collect()
}
