//! Lets DWM blur without Microsoft's shell: the session publishes the transparency state DWM
//! reads (`shellman_core::system::transparency`) with the user's setting, before it starts any
//! app, so every glass window is made with blur available.
//!
//! Then a thread of its own follows, without polling: the setting's registry key, the end of
//! Windows' own shell (Explorer or the modern-app host: while one runs it owns the state and is
//! left alone; when it ends it may have left another value), and a poke from the session window
//! (`WM_SETTINGCHANGE` "ImmersiveColorSet", or a glass refresh broadcast, which the desktop sends
//! once the host is up). A publish that changed the value asks every glass panel to make its
//! backdrop again, since a backdrop made while DWM said no stays flat.
//!
//! Only the real session does this; a test session (`SHELLMAN_SESSION_APPS`) only with
//! `SHELLMAN_TRANSPARENCY=on`, and `off` stops it everywhere.

use crate::log;
use shellman_core::system::transparency::{self as t, Act};
use std::sync::OnceLock;
use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{CloseHandle, HANDLE, LPARAM, WAIT_OBJECT_0, WPARAM};
use windows::Win32::System::Registry::{
    RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, KEY_QUERY_VALUE,
    REG_NOTIFY_CHANGE_LAST_SET,
};
use windows::Win32::System::Threading::{
    CreateEventW, OpenProcess, ResetEvent, SetEvent, WaitForMultipleObjects, INFINITE, PROCESS_SYNCHRONIZE,
};
use windows::Win32::UI::WindowsAndMessaging::{RegisterWindowMessageW, SendNotifyMessageW, HWND_BROADCAST, WM_SETTINGCHANGE};

pub const ENV: &str = "SHELLMAN_TRANSPARENCY";

/// The poke event, set from the session window's thread.
static POKE: OnceLock<isize> = OnceLock::new();

/// Whether this session keeps the state, from `SHELLMAN_TRANSPARENCY` and the test mode.
pub fn enabled(env: Option<&str>, test: bool) -> bool {
    match env.map(str::trim) {
        Some("off") => false,
        Some("on") => true,
        _ => !test,
    }
}

/// Publish now if needed, then follow on a thread. Called before any app starts.
pub fn start(test: bool) {
    if !enabled(std::env::var(ENV).ok().as_deref(), test) {
        log::line("transparency: not kept by this session");
        return;
    }
    let mut keeper = Keeper { last: String::new() };
    keeper.check("start");
    let Ok(poke) = (unsafe { CreateEventW(None, false, false, None) }) else { return };
    let raw = poke.0 as isize;
    let _ = POKE.set(raw);
    let _ = std::thread::Builder::new().name("transparency".into()).spawn(move || keeper.follow(HANDLE(raw as *mut _)));
}

/// The session window's messages that may mean the state changed under shellman.
pub fn on_message(msg: u32, lparam: LPARAM) {
    let colours = msg == WM_SETTINGCHANGE && is_immersive_colors(lparam);
    if colours || msg == refresh_message() {
        if let Some(&h) = POKE.get() {
            unsafe {
                let _ = SetEvent(HANDLE(h as *mut _));
            }
        }
    }
}

fn is_immersive_colors(lparam: LPARAM) -> bool {
    lparam.0 != 0 && unsafe { windows::core::PCWSTR(lparam.0 as *const u16).to_string() }.is_ok_and(|t| t == "ImmersiveColorSet")
}

fn refresh_message() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("shellman-glass-refresh")) })
}

struct Keeper {
    /// The last line logged for a state left alone, so the same reason is logged once.
    last: String,
}

impl Keeper {
    /// Look, decide, act; one log line for what was done.
    fn check(&mut self, why: &str) {
        let before = t::read();
        let wanted = t::setting();
        let shell = t::real_shell();
        let state = before.ok().and_then(|r| r.allowed);
        let stamp = |r: Result<t::Reading, i32>| match r {
            Ok(r) => format!("stamp {} value {}", r.stamp, show(r.allowed)),
            Err(s) => format!("unreadable {:#010x}", s as u32),
        };
        match t::decide(state, wanted, shell.is_some()) {
            Act::Publish(value) => {
                let line = match t::publish(value) {
                    Ok(()) => format!(
                        "transparency ({why}): published {} ({} -> {})",
                        u8::from(value),
                        stamp(before),
                        stamp(t::read())
                    ),
                    Err(s) => format!("transparency ({why}): publishing {} failed, NTSTATUS {:#010x} ({})", u8::from(value), s as u32, stamp(before)),
                };
                log::line(&line);
                self.last.clear();
                if state != Some(value) && line.contains("published") {
                    refresh_glass();
                }
            }
            act => {
                let line = match act {
                    Act::RealShell => format!("transparency: Windows' shell (pid {}) owns it; left alone ({})", shell.unwrap_or(0), stamp(before)),
                    _ => format!("transparency: already {} ({})", u8::from(wanted), stamp(before)),
                };
                // Stamps move with every publish, so compare without them.
                let key = line.split(" (").next().unwrap_or_default().to_string();
                if key != self.last {
                    log::line(&format!("{line} [{why}]"));
                    self.last = key;
                }
            }
        }
    }

    /// Wait on the setting's key, the real shell's end and the poke, forever; check on each.
    fn follow(mut self, poke: HANDLE) {
        let key = open_key();
        let changed = unsafe { CreateEventW(None, true, false, None) }.ok();
        let arm = |key: HKEY, ev: HANDLE| unsafe {
            let _ = ResetEvent(ev);
            RegNotifyChangeKeyValue(key, false, REG_NOTIFY_CHANGE_LAST_SET, Some(ev), true).is_ok()
        };
        let watching = matches!((key, changed), (Some(k), Some(e)) if arm(k, e));
        loop {
            let shell = t::real_shell().and_then(|pid| unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.ok());
            let mut handles = vec![poke];
            if let (true, Some(e)) = (watching, changed) {
                handles.push(e);
            }
            if let Some(s) = shell {
                handles.push(s);
            }
            let got = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
            let i = got.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
            if let Some(s) = shell {
                unsafe {
                    let _ = CloseHandle(s);
                }
            }
            let why = match handles.get(i) {
                Some(h) if *h == poke => "asked",
                Some(h) if Some(*h) == changed => {
                    if let (Some(k), Some(e)) = (key, changed) {
                        arm(k, e);
                    }
                    "setting changed"
                }
                Some(_) => "Windows' shell ended",
                None => {
                    // The wait itself failed; never spin on that.
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    "wait failed"
                }
            };
            // A writer may set several values in a row; let it finish.
            std::thread::sleep(std::time::Duration::from_millis(100));
            self.check(why);
        }
    }
}

fn open_key() -> Option<HKEY> {
    let mut key = HKEY::default();
    let path = HSTRING::from(t::SETTING_KEY);
    unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &path, Some(0), KEY_NOTIFY | KEY_QUERY_VALUE, &mut key) }.is_ok().then_some(key)
}

fn show(v: Option<bool>) -> &'static str {
    match v {
        None => "none",
        Some(true) => "1",
        Some(false) => "0",
    }
}

/// Ask every shellman glass panel to make its backdrop again (`shellman_ui::glass`'s
/// `REFRESH_MESSAGE`).
fn refresh_glass() {
    unsafe {
        let _ = SendNotifyMessageW(HWND_BROADCAST, refresh_message(), WPARAM(0), LPARAM(0));
    }
}

#[cfg(test)]
mod tests {
    use super::enabled;

    #[test]
    fn only_the_real_session_keeps_it_unless_told() {
        assert!(enabled(None, false));
        assert!(!enabled(None, true));
        assert!(enabled(Some("on"), true));
        assert!(!enabled(Some("off"), false));
        assert!(enabled(Some(" "), false));
    }
}
