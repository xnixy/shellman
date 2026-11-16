//! Win+1 ... Win+9 open the dock's pinned apps, as on Windows' taskbar.
//!
//! With shellman as the shell, Windows' own Win+digit does nothing (no taskbar runs). The
//! switcher's hook (`keys`) asks here on each key: a digit pressed while a Win key is held (and no
//! Ctrl, Alt or Shift) is swallowed, its repeats and release too, and the bar is told through the
//! digit's named event (`Local\shellman-dock-key<instance>-<n>`; the bar's `dock::numkey` waits
//! on them). The bar starts the Nth pinned app, or brings it forward, minimizes it or steps
//! through its windows, as a click on its icon does. Windows saw Win go down; so that its release
//! is not taken for a lone tap, one press of the unused mask key goes in place of the digit, which
//! also lets this process hand the foreground to the bar. Only while shellman is the shell; next
//! to Explorer, Win+digit stays Windows'.

use std::sync::atomic::{AtomicU16, Ordering};
use windows::core::HSTRING;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{OpenEventW, SetEvent, EVENT_MODIFY_STATE};
use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};

/// What to do with a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    /// Swallow it and open the dock's pinned app with this number (1 to 9).
    Open(u8),
    /// Swallow it (a repeat, or the release of a digit that opened an app).
    Swallow,
}

/// The number on a digit key of the main row: 1 to 9. The number pad is not Windows' either.
pub fn digit(vk: VIRTUAL_KEY) -> Option<u8> {
    (0x31..=0x39).contains(&vk.0).then(|| (vk.0 - 0x30) as u8)
}

/// The whole rule, kept pure. `held`: the digit key whose press was taken and whose release has
/// not come yet, if any.
pub fn decide(vk: VIRTUAL_KEY, down: bool, mods: crate::taskkey::Mods, held: Option<VIRTUAL_KEY>) -> Verdict {
    let Some(n) = digit(vk) else { return Verdict::Pass };
    if held == Some(vk) {
        // Its repeats while held, and its release, whatever Win does meanwhile.
        return Verdict::Swallow;
    }
    if down && mods.win && !mods.ctrl && !mods.alt && !mods.shift {
        return Verdict::Open(n);
    }
    Verdict::Pass
}

/// The digit key taken (its code), 0 for none.
static HELD: AtomicU16 = AtomicU16::new(0);

/// Called by the hook for every key (digits are the only ones looked at). True: swallow it.
pub fn on_key(vk: VIRTUAL_KEY, down: bool, mods: impl FnOnce() -> crate::taskkey::Mods) -> bool {
    if digit(vk).is_none() {
        return false;
    }
    let held = Some(VIRTUAL_KEY(HELD.load(Ordering::Acquire))).filter(|k| k.0 != 0);
    let mods = if down { mods() } else { crate::taskkey::Mods::default() };
    match decide(vk, down, mods, held) {
        Verdict::Pass => false,
        Verdict::Swallow => {
            if !down {
                HELD.store(0, Ordering::Release);
            }
            true
        }
        Verdict::Open(n) => {
            if !crate::winkey::shellman_is_shell() {
                return false;
            }
            HELD.store(vk.0, Ordering::Release);
            // Windows must see a key while Win is down, or Win's release opens Start.
            crate::keys::send_mask();
            std::thread::spawn(move || summon(n));
            true
        }
    }
}

/// The event the bar waits on for number `n`; the bar names it the same (`dock::numkey`).
pub fn event_name(instance: &str, n: u8) -> String {
    format!("Local\\shellman-dock-key{instance}-{n}")
}

/// Tell the bar; nothing happens when no bar waits.
fn summon(n: u8) {
    // The hook just sent input, so this process may hand the foreground on: the bar brings the
    // app's window forward.
    let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
    let instance = std::env::var("SHELLMAN_INSTANCE").unwrap_or_default();
    let name = HSTRING::from(event_name(&instance, n));
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
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_0, VK_1, VK_5, VK_9, VK_E, VK_NUMPAD1};

    const WIN: Mods = Mods { ctrl: false, shift: false, alt: false, win: true };

    #[test]
    fn digits_are_one_to_nine() {
        assert_eq!(digit(VK_1), Some(1));
        assert_eq!(digit(VK_9), Some(9));
        assert_eq!(digit(VK_0), None, "Win+0 is not asked for");
        assert_eq!(digit(VK_NUMPAD1), None);
        assert_eq!(digit(VK_E), None);
    }

    #[test]
    fn win_digit_opens_and_its_repeats_and_release_are_kept_too() {
        assert_eq!(decide(VK_5, true, WIN, None), Verdict::Open(5));
        assert_eq!(decide(VK_5, true, WIN, Some(VK_5)), Verdict::Swallow, "auto-repeat");
        // Win let go first, then the digit: its release is still ours.
        assert_eq!(decide(VK_5, false, Mods::default(), Some(VK_5)), Verdict::Swallow);
        // Another digit while one is held: a new press.
        assert_eq!(decide(VK_1, true, WIN, Some(VK_5)), Verdict::Open(1));
    }

    #[test]
    fn everything_else_passes() {
        assert_eq!(decide(VK_1, true, Mods::default(), None), Verdict::Pass, "plain 1");
        assert_eq!(decide(VK_1, true, Mods { shift: true, ..WIN }, None), Verdict::Pass, "Win+Shift+1");
        assert_eq!(decide(VK_1, true, Mods { ctrl: true, ..WIN }, None), Verdict::Pass, "Win+Ctrl+1");
        assert_eq!(decide(VK_1, true, Mods { alt: true, ..WIN }, None), Verdict::Pass, "Win+Alt+1");
        assert_eq!(decide(VK_1, false, WIN, None), Verdict::Pass, "a release nobody took");
        assert_eq!(decide(VK_1, false, WIN, Some(VK_5)), Verdict::Pass, "another digit's release");
        assert_eq!(decide(VK_E, true, WIN, None), Verdict::Pass, "Win+E stays Windows'");
    }

    #[test]
    fn the_bar_and_the_switcher_name_the_same_events() {
        assert_eq!(event_name("", 3), "Local\\shellman-dock-key-3");
        assert_eq!(event_name("test", 9), "Local\\shellman-dock-keytest-9");
    }
}
