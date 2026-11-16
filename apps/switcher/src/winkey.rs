//! A lone Win key opens shellman's launcher instead of Windows' Start menu.
//!
//! Windows opens Start when Win is pressed and released with no other key in between (with the
//! modern-app host running, its Start menu is there to open). Win's press is never held back
//! here, so every Win shortcut (Win+E, Win+R, Win+Shift+S, Win+arrows...) reaches Windows
//! exactly as typed (shellman's own Win shortcuts, Win+N and Win+1 ... 9, are taken by their
//! modules after this one has seen them); holding back the press would mean replaying it, in
//! order, before whatever key follows. Only the release of a lone tap is changed: it is swallowed, and in its place the
//! hook sends one press of an unused key (0xE8) and then the release. Windows saw "another key"
//! while Win was down, so no Start menu; the launcher is summoned instead.
//!
//! Only while Explorer is not the shell: next to Explorer, Win stays Windows'.
//!
//! The hook calls `on_key` for every key; it decides from a little state and returns at once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_LWIN, VK_RWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{KBDLLHOOKSTRUCT, LLKHF_EXTENDED, LLKHF_UP};

/// Marks the keys shellman sends, so hooks let them by (the switcher's `keys` uses the same).
pub const OURS: usize = 0x5348_4D4E; // "SHMN"
const VK_MASK: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);

/// The Win key's state between events.
#[derive(Default, Debug)]
pub struct WinTap {
    /// The Win key that is down, if one is.
    held: Option<VIRTUAL_KEY>,
    /// Nothing else happened since it went down, and the tap is shellman's to take.
    alone: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Pass,
    /// A lone tap's release: swallow it, send the mask key and the release, summon the launcher.
    Tap,
}

impl WinTap {
    /// One key event. `repeat`: the key was already down (auto-repeat). `take`: whether a tap
    /// would be shellman's, asked only when Win goes down.
    pub fn key(&mut self, vk: VIRTUAL_KEY, down: bool, repeat: bool, take: impl FnOnce() -> bool) -> Verdict {
        let is_win = vk == VK_LWIN || vk == VK_RWIN;
        if !is_win {
            // Any other key, pressed or released, while Win is down: a shortcut, Windows'.
            if self.held.is_some() {
                self.alone = false;
            }
            return Verdict::Pass;
        }
        if down {
            match self.held {
                Some(h) if h == vk && repeat => {}
                // The other Win key too: not a lone tap.
                Some(h) if h != vk => self.alone = false,
                // A fresh press (also after a release the hook never saw, as with Win+L).
                _ => {
                    self.held = Some(vk);
                    self.alone = take();
                }
            }
            return Verdict::Pass;
        }
        let tap = self.held == Some(vk) && self.alone;
        if self.held == Some(vk) {
            self.held = None;
        }
        self.alone = false;
        if tap {
            Verdict::Tap
        } else {
            Verdict::Pass
        }
    }
}

static STATE: Mutex<WinTap> = Mutex::new(WinTap { held: None, alone: false });

/// For the hook, on every key event that is not shellman's own. True: swallow it. `take` says
/// whether a tap is shellman's (`shellman_is_shell`); `summon` runs on a thread of its own after
/// one.
pub fn on_key(info: &KBDLLHOOKSTRUCT, take: fn() -> bool, summon: fn()) -> bool {
    let vk = VIRTUAL_KEY(info.vkCode as u16);
    let down = info.flags.0 & LLKHF_UP.0 == 0;
    // During the hook Windows has not yet recorded this event, so "already down" is a repeat.
    let repeat = down && (vk == VK_LWIN || vk == VK_RWIN) && unsafe { GetAsyncKeyState(vk.0 as i32) } < 0;
    let verdict = STATE.lock().map_or(Verdict::Pass, |mut s| s.key(vk, down, repeat, take));
    if verdict == Verdict::Pass {
        return false;
    }
    release_masked(vk, info.scanCode as u16, info.flags.0 & LLKHF_EXTENDED.0 != 0);
    std::thread::spawn(summon);
    true
}

/// The mask key, then Win's release, as one batch: Windows reads them after the swallowed
/// release, in this order, so the release can never be lost or come first.
fn release_masked(vk: VIRTUAL_KEY, scan: u16, extended: bool) {
    let key = |vk: VIRTUAL_KEY, scan: u16, flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, dwExtraInfo: OURS, ..Default::default() } },
    };
    let ext = if extended { KEYEVENTF_EXTENDEDKEY } else { Default::default() };
    let inputs = [key(VK_MASK, 0, Default::default()), key(VK_MASK, 0, KEYEVENTF_KEYUP), key(vk, scan, KEYEVENTF_KEYUP | ext)];
    unsafe {
        SendInput(&inputs, size_of::<INPUT>() as i32);
    }
}

/// Whether a lone Win is shellman's: Explorer is not the shell, and there is a launcher to open.
pub fn shellman_is_shell() -> bool {
    static LAUNCHER: OnceLock<bool> = OnceLock::new();
    let launcher = *LAUNCHER.get_or_init(|| {
        std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("shellman-launcher.exe").exists())).unwrap_or(false)
    });
    launcher && !shellman_core::app_host::explorer_is_shell()
}

/// Show (or hide) the launcher: signal the resident one through its named event (see the
/// launcher's `summon`), or start it.
pub fn summon() {
    static BUSY: AtomicBool = AtomicBool::new(false);
    if BUSY.swap(true, Ordering::AcqRel) {
        return;
    }
    // The hook just sent input, so this process may hand the foreground on (done there).
    shellman_core::summon::launcher();
    BUSY.store(false, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_D, VK_E, VK_LEFT, VK_LSHIFT, VK_S};

    /// Feed (key, down) pairs; return the verdict of each.
    fn run(take: bool, keys: &[(VIRTUAL_KEY, bool)]) -> Vec<Verdict> {
        let mut t = WinTap::default();
        let mut down_keys: Vec<VIRTUAL_KEY> = Vec::new();
        keys.iter()
            .map(|&(vk, down)| {
                let repeat = down && down_keys.contains(&vk);
                if down {
                    down_keys.push(vk);
                } else {
                    down_keys.retain(|&k| k != vk);
                }
                t.key(vk, down, repeat, || take)
            })
            .collect()
    }

    use Verdict::{Pass, Tap};

    #[test]
    fn a_lone_tap_is_taken_on_release_only() {
        assert_eq!(run(true, &[(VK_LWIN, true), (VK_LWIN, false)]), [Pass, Tap]);
        assert_eq!(run(true, &[(VK_RWIN, true), (VK_RWIN, false)]), [Pass, Tap]);
    }

    #[test]
    fn held_win_repeats_are_still_a_tap() {
        assert_eq!(run(true, &[(VK_LWIN, true), (VK_LWIN, true), (VK_LWIN, true), (VK_LWIN, false)]), [Pass, Pass, Pass, Tap]);
    }

    #[test]
    fn shortcuts_pass_untouched() {
        assert_eq!(run(true, &[(VK_LWIN, true), (VK_E, true), (VK_E, false), (VK_LWIN, false)]), [Pass; 4]);
        // Win+Shift+S, Win+Left, Win+D: every event through, Win's release too.
        let shift_s = [(VK_LWIN, true), (VK_LSHIFT, true), (VK_S, true), (VK_S, false), (VK_LSHIFT, false), (VK_LWIN, false)];
        assert_eq!(run(true, &shift_s), [Pass; 6]);
        assert_eq!(run(true, &[(VK_LWIN, true), (VK_LEFT, true), (VK_LEFT, false), (VK_LWIN, false)]), [Pass; 4]);
        // Released in another order: Win first, then the key.
        assert_eq!(run(true, &[(VK_LWIN, true), (VK_D, true), (VK_LWIN, false), (VK_D, false)]), [Pass; 4]);
    }

    #[test]
    fn a_key_held_before_win_spoils_the_tap() {
        // Shift down, Win tap while Shift repeats, then released.
        let keys = [(VK_LSHIFT, true), (VK_LWIN, true), (VK_LSHIFT, true), (VK_LWIN, false), (VK_LSHIFT, false)];
        assert_eq!(run(true, &keys), [Pass; 5]);
    }

    #[test]
    fn both_win_keys_are_not_a_tap() {
        let keys = [(VK_LWIN, true), (VK_RWIN, true), (VK_RWIN, false), (VK_LWIN, false)];
        assert_eq!(run(true, &keys), [Pass; 4]);
    }

    #[test]
    fn next_to_explorer_win_stays_windows() {
        assert_eq!(run(false, &[(VK_LWIN, true), (VK_LWIN, false)]), [Pass, Pass]);
    }

    #[test]
    fn a_missed_release_does_not_stick() {
        // Win+L: the lock screen takes the keyboard, the releases are never seen.
        let mut t = WinTap::default();
        t.key(VK_LWIN, true, false, || true);
        t.key(VIRTUAL_KEY(b'L' as u16), true, false, || true);
        // Back at the desktop, a fresh tap works.
        assert_eq!(t.key(VK_LWIN, true, false, || true), Pass);
        assert_eq!(t.key(VK_LWIN, false, false, || true), Tap);
        // And a stray release with nothing down is just passed on.
        assert_eq!(t.key(VK_LWIN, false, false, || true), Pass);
    }
}
