//! Win+N opens shellman's notifications (the bar's bell panel) instead of nothing.
//!
//! With shellman as the shell, Windows' own Win+N opens nothing (no notification centre runs).
//! The switcher's hook (`keys`) asks here on each key: N pressed while a Win key is held (and no
//! Ctrl, Alt or Shift) is swallowed, its release too, and the bar is told through its named
//! event (`Local\shellman-notifications-summon<instance>`; the bar's `notify::summon` waits on
//! it). Windows saw Win go down; so that its release is not taken for a lone tap (which would
//! open Start), one press of the unused mask key goes in place of the N. Only while shellman is
//! the shell; next to Explorer, Win+N stays Windows'.

use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::HSTRING;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{OpenEventW, SetEvent, EVENT_MODIFY_STATE};
use windows::Win32::UI::Input::KeyboardAndMouse::{VIRTUAL_KEY, VK_N};

/// What to do with a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    /// Swallow it and open the notifications.
    Open,
    /// Swallow it (a repeat, or the release of an N that opened them).
    Swallow,
}

/// The whole rule, kept pure. `held`: an N press was taken and its release has not come yet.
pub fn decide(vk: VIRTUAL_KEY, down: bool, mods: crate::taskkey::Mods, held: bool) -> Verdict {
    if vk != VK_N {
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

/// Called by the hook for every key (N is the only one looked at). True: swallow it.
pub fn on_key(vk: VIRTUAL_KEY, down: bool, mods: impl FnOnce() -> crate::taskkey::Mods) -> bool {
    if vk != VK_N {
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
            if !crate::winkey::shellman_is_shell() {
                return false;
            }
            HELD.store(true, Ordering::Release);
            // Windows must see a key while Win is down, or Win's release opens Start.
            crate::keys::send_mask();
            std::thread::spawn(summon);
            true
        }
    }
}

/// Tell the bar (its named event); nothing happens when no bar waits on it.
fn summon() {
    let instance = std::env::var("SHELLMAN_INSTANCE").unwrap_or_default();
    let name = HSTRING::from(format!("Local\\shellman-notifications-summon{instance}"));
    if let Ok(event) = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, &name) } {
        unsafe {
            let _ = SetEvent(event);
            let _ = CloseHandle(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskkey::Mods;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_E, VK_N};

    const WIN: Mods = Mods { ctrl: false, shift: false, alt: false, win: true };

    #[test]
    fn win_n_opens_and_its_release_is_kept_too() {
        assert_eq!(decide(VK_N, true, WIN, false), Verdict::Open);
        assert_eq!(decide(VK_N, true, WIN, true), Verdict::Swallow, "auto-repeat");
        assert_eq!(decide(VK_N, false, Mods::default(), true), Verdict::Swallow);
    }

    #[test]
    fn everything_else_passes() {
        assert_eq!(decide(VK_N, true, Mods::default(), false), Verdict::Pass, "plain N");
        assert_eq!(decide(VK_N, true, Mods { shift: true, ..WIN }, false), Verdict::Pass, "Win+Shift+N");
        assert_eq!(decide(VK_N, true, Mods { ctrl: true, ..WIN }, false), Verdict::Pass);
        assert_eq!(decide(VK_N, false, WIN, false), Verdict::Pass, "a release nobody took");
        assert_eq!(decide(VK_E, true, WIN, false), Verdict::Pass, "Win+E stays Windows'");
    }
}
