//! Win+Tab opens shellman's Task View (`apps/taskview`) instead of nothing.
//!
//! With shellman as the shell, Windows' own Task View (part of Explorer) is not there. The
//! switcher's hook (`keys`) asks here on each key: Tab pressed while a Win key is held (and no
//! Ctrl, Alt or Shift) is swallowed, its release too, and Task View is told through its named
//! event (`Local\shellman-taskview-summon<instance>`), or started. Each Win+Tab toggles it; once
//! open, Task View has the foreground and reads its own keys. Windows saw Win go down; so that
//! its release is not taken for a lone tap, one press of the unused mask key goes in place of the
//! Tab. Only while shellman is the shell and `shellman-taskview.exe` is beside the switcher.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use windows::core::HSTRING;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{OpenEventW, SetEvent, EVENT_MODIFY_STATE};
use windows::Win32::UI::Input::KeyboardAndMouse::{VIRTUAL_KEY, VK_TAB};
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};

/// What to do with a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    /// Swallow it and open (or close) Task View.
    Open,
    /// Swallow it (a repeat, or the release of a Tab that opened it).
    Swallow,
}

/// The whole rule, kept pure. `held`: a Tab press was taken and its release has not come yet.
pub fn decide(vk: VIRTUAL_KEY, down: bool, mods: crate::taskkey::Mods, held: bool) -> Verdict {
    if vk != VK_TAB {
        return Verdict::Pass;
    }
    if !down {
        return if held { Verdict::Swallow } else { Verdict::Pass };
    }
    if mods.win && !mods.ctrl && !mods.alt && !mods.shift {
        return if held { Verdict::Swallow } else { Verdict::Open };
    }
    Verdict::Pass
}

static HELD: AtomicBool = AtomicBool::new(false);

/// Whether Task View is there to open, and shellman is the shell.
fn ours() -> bool {
    static EXE: OnceLock<bool> = OnceLock::new();
    let exe = *EXE.get_or_init(|| {
        std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("shellman-taskview.exe").exists())).unwrap_or(false)
    });
    exe && crate::winkey::shellman_is_shell()
}

/// Called by the hook for every key (Tab is the only one looked at). True: swallow it.
pub fn on_key(vk: VIRTUAL_KEY, down: bool, mods: impl FnOnce() -> crate::taskkey::Mods) -> bool {
    if vk != VK_TAB {
        return false;
    }
    let held = HELD.load(Ordering::Acquire);
    let mods = if down { mods() } else { crate::taskkey::Mods::default() };
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
            // Windows must see a key while Win is down, or Win's release opens Start. It also
            // makes this process the last input's sender, so it may hand on the foreground.
            crate::keys::send_mask();
            std::thread::spawn(summon);
            true
        }
    }
}

/// Signal the resident Task View, or start it.
fn summon() {
    let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
    let instance = std::env::var("SHELLMAN_INSTANCE").unwrap_or_default();
    let name = HSTRING::from(format!("Local\\shellman-taskview-summon{instance}"));
    match unsafe { OpenEventW(EVENT_MODIFY_STATE, false, &name) } {
        Ok(event) => unsafe {
            let _ = SetEvent(event);
            let _ = CloseHandle(event);
        },
        Err(_) => {
            shellman_core::sibling::start("taskview");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskkey::Mods;
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_N;

    const WIN: Mods = Mods { ctrl: false, shift: false, alt: false, win: true };

    #[test]
    fn win_tab_opens_and_its_release_is_kept_too() {
        assert_eq!(decide(VK_TAB, true, WIN, false), Verdict::Open);
        assert_eq!(decide(VK_TAB, true, WIN, true), Verdict::Swallow, "auto-repeat");
        assert_eq!(decide(VK_TAB, false, Mods::default(), true), Verdict::Swallow, "Win let go first");
        assert_eq!(decide(VK_TAB, false, WIN, true), Verdict::Swallow);
    }

    #[test]
    fn everything_else_passes() {
        assert_eq!(decide(VK_TAB, true, Mods::default(), false), Verdict::Pass, "plain Tab");
        assert_eq!(decide(VK_TAB, true, Mods { alt: true, win: false, ..WIN }, false), Verdict::Pass, "Alt+Tab is the switcher's");
        assert_eq!(decide(VK_TAB, true, Mods { shift: true, ..WIN }, false), Verdict::Pass, "Win+Shift+Tab");
        assert_eq!(decide(VK_TAB, true, Mods { ctrl: true, ..WIN }, false), Verdict::Pass, "Win+Ctrl+Tab");
        assert_eq!(decide(VK_TAB, false, WIN, false), Verdict::Pass, "a release nobody took");
        assert_eq!(decide(VK_N, true, WIN, false), Verdict::Pass);
    }
}
