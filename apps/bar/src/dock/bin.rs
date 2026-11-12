//! The dock's Recycle Bin icon: open it in Files, and empty it after a native confirmation.
//!
//! Files shows the bin itself, so nothing here hands off to `shell:RecycleBinFolder` (which would
//! start Explorer's folder window). Emptying asks with a plain Windows message box: the dock has
//! no window of its own tall enough for an in-window dialog.

use shellman_core::recycle_bin::{self, BinState};
use std::sync::mpsc::channel;
use std::time::Duration;
use windows::core::HSTRING;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO,
};

/// Start Files on the bin. False when Files is not next to the bar.
pub fn open() -> bool {
    shellman_core::sibling::start_with("files", &["--recycle-bin"])
}

/// Whether the bin has anything to empty. The shell is asked on another thread and given a
/// moment: a slow answer counts as "yes", so the menu never stalls and the confirmation (which
/// asks again) has the last word.
pub fn can_empty() -> bool {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(recycle_bin::state());
    });
    match rx.recv_timeout(Duration::from_millis(300)) {
        Ok(Some(state)) => !state.is_empty(),
        Ok(None) | Err(_) => true,
    }
}

/// Ask, then empty the bin. Runs on its own thread: the message box and the shell call both
/// block, and neither belongs on a dock window's thread.
pub fn confirm_and_empty() {
    std::thread::spawn(|| {
        let Some(state) = recycle_bin::state() else { return };
        if state.is_empty() {
            return;
        }
        if ask(state) {
            let _ = recycle_bin::empty(0);
        }
    });
}

fn ask(state: BinState) -> bool {
    let text = format!("Permanently delete everything in the Recycle Bin?\n\n{}. This can't be undone.", recycle_bin::summary(state));
    let answer = unsafe {
        MessageBoxW(
            Some(HWND::default()),
            &HSTRING::from(text),
            &HSTRING::from("Empty Recycle Bin"),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2 | MB_TOPMOST | MB_SETFOREGROUND,
        )
    };
    answer == IDYES
}
