//! Ctrl+Shift+Esc opens shellman's Task Manager instead of Windows' own.
//!
//! Windows keeps Ctrl+Shift+Esc for itself: `RegisterHotKey` for it fails (the key is already
//! registered, error 1409), and the system starts taskmgr.exe when it sees the key. A low-level
//! keyboard hook sees keys before that, so the switcher's hook (`keys`) asks here on each key:
//! Esc pressed with Ctrl and Shift (and no Alt or Win) is swallowed, its release too, and
//! `shellman-tasks.exe` is started (or the open one raised). Only while shellman is the shell and
//! `shellman-tasks.exe` is next to the switcher; otherwise the key stays Windows'.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use windows::Win32::UI::Input::KeyboardAndMouse::{VIRTUAL_KEY, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};

/// The modifiers held when a key came.
#[derive(Clone, Copy, Debug, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
}

/// What to do with a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    /// Swallow it and open Task Manager.
    Open,
    /// Swallow it (the release of an Esc that opened Task Manager).
    Swallow,
}

/// The whole rule, kept pure. `held`: an Esc press was taken and its release has not come yet.
pub fn decide(vk: VIRTUAL_KEY, down: bool, mods: Mods, held: bool) -> Verdict {
    if vk != VK_ESCAPE {
        return Verdict::Pass;
    }
    if !down {
        return if held { Verdict::Swallow } else { Verdict::Pass };
    }
    if mods.ctrl && mods.shift && !mods.alt && !mods.win {
        // Holding the keys repeats the press: open once, swallow the repeats.
        return if held { Verdict::Swallow } else { Verdict::Open };
    }
    Verdict::Pass
}

static HELD: AtomicBool = AtomicBool::new(false);

/// Whether Task Manager is there to open, and shellman is the shell.
fn ours() -> bool {
    static EXE: OnceLock<bool> = OnceLock::new();
    let exe = *EXE.get_or_init(|| {
        std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("shellman-tasks.exe").exists())).unwrap_or(false)
    });
    exe && crate::winkey::shellman_is_shell()
}

/// Called by the hook for every key (Esc is the only one looked at). True: swallow it.
pub fn on_key(vk: VIRTUAL_KEY, down: bool, mods: impl FnOnce() -> Mods) -> bool {
    if vk != VK_ESCAPE {
        return false;
    }
    let held = HELD.load(Ordering::Acquire);
    // Modifiers are read only for Esc, and only when it goes down.
    let mods = if down { mods() } else { Mods::default() };
    match decide(vk, down, mods, held) {
        Verdict::Pass => false,
        Verdict::Swallow => {
            if !down {
                HELD.store(false, Ordering::Release);
            }
            true
        }
        Verdict::Open => {
            if !ours() {
                return false;
            }
            HELD.store(true, Ordering::Release);
            // This process had the last input, so it may hand the foreground to Task Manager.
            let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
            // Never start a process from inside the hook's callback.
            std::thread::spawn(|| shellman_core::sibling::start("tasks"));
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_A, VK_TAB};

    const CS: Mods = Mods { ctrl: true, shift: true, alt: false, win: false };

    #[test]
    fn ctrl_shift_esc_opens_and_its_release_is_kept_too() {
        assert_eq!(decide(VK_ESCAPE, true, CS, false), Verdict::Open);
        assert_eq!(decide(VK_ESCAPE, true, CS, true), Verdict::Swallow, "auto-repeat");
        assert_eq!(decide(VK_ESCAPE, false, Mods::default(), true), Verdict::Swallow);
    }

    #[test]
    fn everything_else_passes() {
        assert_eq!(decide(VK_ESCAPE, true, Mods::default(), false), Verdict::Pass);
        assert_eq!(decide(VK_ESCAPE, true, Mods { ctrl: true, ..Default::default() }, false), Verdict::Pass, "Ctrl+Esc");
        assert_eq!(decide(VK_ESCAPE, true, Mods { alt: true, ..CS }, false), Verdict::Pass);
        assert_eq!(decide(VK_ESCAPE, true, Mods { win: true, ..CS }, false), Verdict::Pass);
        assert_eq!(decide(VK_ESCAPE, false, CS, false), Verdict::Pass, "a release nobody took");
        assert_eq!(decide(VK_A, true, CS, false), Verdict::Pass);
        assert_eq!(decide(VK_TAB, true, CS, true), Verdict::Pass);
    }
}
