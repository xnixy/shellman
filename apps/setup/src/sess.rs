//! Talking to the running session: "restart app X" (see `apps/session/src/restart.rs`, which
//! documents the protocol and why it is not a crash). The window is found by class and title;
//! `SHELLMAN_INSTANCE` picks a test session, so a request never reaches the real one by mistake.
//! The send has a timeout and gives up on a hung session. No input is synthesised.

use windows::core::HSTRING;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SendMessageTimeoutW, SMTO_ABORTIFHUNG, WM_COPYDATA};

/// Must match `restart::MAGIC` in the session.
const MAGIC: usize = 0x5348_4D52;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Answer {
    Unknown,
    Accepted,
    Refused,
}

pub fn answer_of(code: usize) -> Answer {
    match code {
        1 => Answer::Accepted,
        2 => Answer::Refused,
        _ => Answer::Unknown,
    }
}

/// The text the session expects (kept next to the session's own `encode`; tested against it by
/// value here).
pub fn request(name: &str) -> Vec<u8> {
    format!("restart {name}").into_bytes()
}

/// The app name for an exe file name: `shellman-bar.exe` -> `bar`.
pub fn app_name(rel: &str) -> Option<String> {
    let n = rel.to_ascii_lowercase();
    n.strip_prefix("shellman-")?.strip_suffix(".exe").filter(|s| !s.is_empty() && !s.contains(['\\', '/', '.'])).map(str::to_string)
}

pub fn window_title(instance: &str) -> String {
    if instance.is_empty() {
        "shellman session".into()
    } else {
        format!("shellman session {instance}")
    }
}

pub fn find(instance: &str) -> Option<HWND> {
    unsafe { FindWindowW(&HSTRING::from("ShellmanSession"), &HSTRING::from(window_title(instance))).ok() }
}

/// Ask the session to restart `name`. `Err`: no session window, or it did not answer in time.
pub fn ask_restart(instance: &str, name: &str) -> Result<Answer, String> {
    let hwnd = find(instance).ok_or("no shellman session is running")?;
    let mut data = request(name);
    let cds = COPYDATASTRUCT { dwData: MAGIC, cbData: data.len() as u32, lpData: data.as_mut_ptr() as *mut _ };
    let mut result = 0usize;
    let r = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&cds as *const COPYDATASTRUCT as isize),
            SMTO_ABORTIFHUNG,
            3000,
            Some(&mut result),
        )
    };
    if r.0 == 0 {
        return Err("the session did not answer in 3 s".into());
    }
    Ok(answer_of(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_names_become_app_names() {
        assert_eq!(app_name("shellman-bar.exe").as_deref(), Some("bar"));
        assert_eq!(app_name("Shellman-Shelf.EXE").as_deref(), Some("shelf"));
        for no in ["shellman-.exe", "conpty.dll", "sub\\shellman-bar.exe", "shellman-bar.old1.exe", "bar.exe"] {
            assert_eq!(app_name(no), None, "{no}");
        }
    }

    #[test]
    fn the_wire_format_is_the_sessions() {
        assert_eq!(request("bar"), b"restart bar");
        assert_eq!(answer_of(1), Answer::Accepted);
        assert_eq!(answer_of(2), Answer::Refused);
        assert_eq!(answer_of(0), Answer::Unknown);
        assert_eq!(window_title(""), "shellman session");
        assert_eq!(window_title("t1"), "shellman session t1");
    }

    #[test]
    fn no_session_means_an_error_not_a_hang() {
        assert!(ask_restart("no-such-test-instance-zz", "bar").is_err());
    }
}
