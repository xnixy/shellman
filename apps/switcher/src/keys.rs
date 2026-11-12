//! Alt+Tab, taken from Windows: a low-level keyboard hook on its own thread.
//!
//! Windows waits for every low-level hook before any key reaches any app, and drops a hook that
//! keeps it waiting too long, so the callback only decides (from the key and a flag) whether to
//! swallow the key, and hands what it saw to the UI. It never waits on the UI. Only Alt+Tab and
//! Alt+Shift+Tab are taken; while the switcher is open, also Esc, Enter and the arrow keys. Every
//! other key passes untouched. The hook belongs to this process: if it ends, Windows' own Alt+Tab
//! is back.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_LMENU, VK_LWIN, VK_MENU, VK_RETURN,
    VK_RIGHT, VK_RMENU, VK_RWIN, VK_SHIFT, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, PostThreadMessageW, SetWindowsHookExW, KBDLLHOOKSTRUCT,
    LLKHF_ALTDOWN, LLKHF_UP, MSG, WH_KEYBOARD_LL, WM_APP,
};

/// What the switcher is asked to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    /// Tab: open, or the next window.
    Next,
    /// Shift+Tab: open on the oldest window, or the previous one.
    Prev,
    Arrow(crate::select::Dir),
    /// Alt released, or Enter: switch to the selection.
    Commit,
    /// Esc: close without switching.
    Cancel,
}

/// One key event, as the hook sees it.
#[derive(Clone, Copy, Debug)]
pub struct Press {
    pub vk: VIRTUAL_KEY,
    pub down: bool,
    pub alt: bool,
    pub shift: bool,
    /// Ctrl or a Windows key is held: those Alt+Tab variants are Windows' own.
    pub other_mods: bool,
}

/// What to do with one key event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Verdict {
    /// Keep the key from every app.
    pub swallow: bool,
    pub key: Option<Key>,
    /// Whether the switcher is open after this key.
    pub open: bool,
}

/// The whole of the hook's logic, kept pure so it can be tested.
pub fn decide(open: bool, p: Press) -> Verdict {
    let pass = Verdict { swallow: false, key: None, open };
    let take = |key: Option<Key>, open: bool| Verdict { swallow: true, key, open };
    let is_alt = matches!(p.vk, VK_MENU | VK_LMENU | VK_RMENU);
    if open && !p.alt && !is_alt {
        // Alt was let go without the hook seeing it (the secure desktop had the keyboard):
        // close the switcher and leave the key alone.
        return Verdict { swallow: false, key: Some(Key::Cancel), open: false };
    }
    if p.vk == VK_TAB {
        return match (p.down, p.alt && !p.other_mods) {
            (true, true) => take(Some(if p.shift { Key::Prev } else { Key::Next }), true),
            // Tab's release goes with its press.
            (false, _) if open || p.alt => take(None, open),
            _ => pass,
        };
    }
    if !open {
        return pass;
    }
    let arrow = |dir| take(p.down.then_some(Key::Arrow(dir)), true);
    use crate::select::Dir;
    match p.vk {
        // Alt's release reaches the apps too, so Windows never thinks Alt is stuck.
        _ if is_alt && !p.down => Verdict { swallow: false, key: Some(Key::Commit), open: false },
        VK_ESCAPE if p.down => take(Some(Key::Cancel), false),
        VK_RETURN if p.down => take(Some(Key::Commit), false),
        VK_LEFT => arrow(Dir::Left),
        VK_RIGHT => arrow(Dir::Right),
        VK_UP => arrow(Dir::Up),
        VK_DOWN => arrow(Dir::Down),
        _ => pass,
    }
}

/// Whether the switcher is open, as far as keys go. Set by the hook; the UI clears it when the
/// switcher closes some other way (a click).
static OPEN: AtomicBool = AtomicBool::new(false);
static SINK: OnceLock<Box<dyn Fn(Key) + Send + Sync>> = OnceLock::new();
static THREAD: AtomicU32 = AtomicU32::new(0);
/// Marks the keys this process sends, so the hook lets them by.
const OURS: usize = 0x5348_4D4E; // "SHMN"
/// Asks the hook thread to send the mask key.
const WM_MASK: u32 = WM_APP + 1;
/// A key no keyboard has and no app uses.
const VK_MASK: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);

/// The switcher closed without a key (a click on a tile).
pub fn close() {
    OPEN.store(false, Ordering::Release);
}

/// Install the hook on its own thread. `sink` is called on that thread with each request; it
/// must be quick and never block. False if Windows refused the hook.
pub fn start(sink: impl Fn(Key) + Send + Sync + 'static) -> bool {
    if SINK.set(Box::new(sink)).is_err() {
        return false;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("keyboard hook".into())
        .spawn(move || run(tx))
        .is_ok()
        && rx.recv().unwrap_or(false)
}

/// The hook thread: low-level hooks are called through the installing thread's message loop,
/// which only otherwise sends the mask key. It lives as long as the process.
fn run(ready: std::sync::mpsc::Sender<bool>) {
    let module = unsafe { GetModuleHandleW(None) }.ok();
    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), module.map(Into::into), 0) };
    THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Release);
    let _ = ready.send(hook.is_ok());
    if hook.is_err() {
        return;
    }
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        if msg.message == WM_MASK {
            send_mask();
        }
    }
}

/// Apps open their menu bar when Alt is pressed and released with no key in between, and the
/// Tab in between was swallowed. A press of an unused key in between keeps them out of it.
fn send_mask() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_MASK,
                dwFlags: flags,
                dwExtraInfo: OURS,
                ..Default::default()
            },
        },
    };
    let inputs = [key(Default::default()), key(KEYEVENTF_KEYUP)];
    unsafe {
        SendInput(&inputs, size_of::<INPUT>() as i32);
    }
}

fn held(vk: VIRTUAL_KEY) -> bool {
    (unsafe { GetAsyncKeyState(vk.0 as i32) }) < 0
}

unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    const HC_ACTION: i32 = 0;
    if code != HC_ACTION {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
    let vk = VIRTUAL_KEY(info.vkCode as u16);
    let watched = matches!(
        vk,
        VK_TAB | VK_MENU | VK_LMENU | VK_RMENU | VK_ESCAPE | VK_RETURN | VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN
    );
    if !watched || info.dwExtraInfo == OURS {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let was_open = OPEN.load(Ordering::Acquire);
    let press = Press {
        vk,
        down: info.flags.0 & LLKHF_UP.0 == 0,
        alt: info.flags.0 & LLKHF_ALTDOWN.0 != 0,
        // Read only for Tab, where they matter.
        shift: vk == VK_TAB && held(VK_SHIFT),
        other_mods: vk == VK_TAB && (held(VK_CONTROL) || held(VK_LWIN) || held(VK_RWIN)),
    };
    let verdict = decide(was_open, press);
    OPEN.store(verdict.open, Ordering::Release);
    if verdict.open && !was_open {
        let _ = PostThreadMessageW(THREAD.load(Ordering::Acquire), WM_MASK, WPARAM(0), LPARAM(0));
    }
    if let (Some(key), Some(sink)) = (verdict.key, SINK.get()) {
        sink(key);
    }
    if verdict.swallow {
        LRESULT(1)
    } else {
        CallNextHookEx(None, code, wparam, lparam)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::select::Dir;

    fn press(vk: VIRTUAL_KEY, down: bool, alt: bool) -> Press {
        Press { vk, down, alt, shift: false, other_mods: false }
    }

    #[test]
    fn alt_tab_opens_and_is_swallowed() {
        let v = decide(false, press(VK_TAB, true, true));
        assert_eq!(v, Verdict { swallow: true, key: Some(Key::Next), open: true });
        let back = decide(false, Press { shift: true, ..press(VK_TAB, true, true) });
        assert_eq!(back.key, Some(Key::Prev));
        assert!(decide(true, press(VK_TAB, false, true)).swallow, "Tab's release too");
    }

    #[test]
    fn other_keys_pass_untouched() {
        assert_eq!(decide(false, press(VK_TAB, true, false)), Verdict { swallow: false, key: None, open: false });
        let ctrl = Press { other_mods: true, ..press(VK_TAB, true, true) };
        assert!(!decide(false, ctrl).swallow, "Ctrl+Alt+Tab stays Windows'");
        assert!(!decide(false, press(VK_ESCAPE, true, true)).swallow, "Alt+Esc when closed");
        assert!(!decide(false, press(VK_LEFT, true, false)).swallow);
        assert!(!decide(false, press(VK_LMENU, false, false)).swallow);
        assert!(!decide(true, press(VIRTUAL_KEY(b'A' as u16), true, true)).swallow);
    }

    #[test]
    fn releasing_alt_switches_and_passes() {
        let v = decide(true, press(VK_LMENU, false, false));
        assert_eq!(v, Verdict { swallow: false, key: Some(Key::Commit), open: false });
        assert_eq!(decide(true, press(VK_RMENU, false, false)).key, Some(Key::Commit));
        assert_eq!(decide(true, press(VK_LMENU, true, true)).key, None, "Alt's repeat is nothing");
    }

    #[test]
    fn esc_enter_and_arrows_while_open() {
        assert_eq!(decide(true, press(VK_ESCAPE, true, true)), Verdict { swallow: true, key: Some(Key::Cancel), open: false });
        assert_eq!(decide(true, press(VK_RETURN, true, true)).key, Some(Key::Commit));
        let left = decide(true, press(VK_LEFT, true, true));
        assert_eq!(left, Verdict { swallow: true, key: Some(Key::Arrow(Dir::Left)), open: true });
        let up = decide(true, press(VK_DOWN, false, true));
        assert_eq!(up, Verdict { swallow: true, key: None, open: true }, "arrow releases are kept too");
    }

    #[test]
    fn a_missed_alt_release_closes_on_the_next_key() {
        let v = decide(true, press(VK_LEFT, true, false));
        assert_eq!(v, Verdict { swallow: false, key: Some(Key::Cancel), open: false });
    }
}
