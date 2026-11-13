//! A second launch handing its tab to the window already open (`new_instance: "tab"`).
//!
//! The first window of a sign-in session listens on a named pipe,
//! `\\.\pipe\shellman-terminal-<session id>`; a later launch with `new_instance: "tab"` writes its
//! [`Launch`] there as JSON and exits. Only that: a folder, a profile name, a command line and
//! `hold`, re-checked on arrival ([`Launch::check`], unknown fields refused). The pipe takes no
//! remote clients, and its default security lets only this user (and SYSTEM and
//! administrators) open it for writing.

use crate::cli::Launch;
use std::sync::{Arc, Mutex};
use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_SHARE_NONE, OPEN_EXISTING,
    PIPE_ACCESS_INBOUND,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

/// The most a launch message may be.
const MAX_MESSAGE: usize = 128 * 1024;

fn pipe_name() -> HSTRING {
    let mut session = 0u32;
    unsafe {
        let _ = ProcessIdToSessionId(GetCurrentProcessId(), &mut session);
    }
    HSTRING::from(format!(r"\\.\pipe\shellman-terminal-{session}"))
}

/// Launches other processes handed over, for the window to open as tabs.
pub type Inbox = Arc<Mutex<Vec<Launch>>>;

/// Hand `launch` to the window already open. False when there is none (or it would not take it),
/// so this process opens its own.
pub fn hand_over(launch: &Launch) -> bool {
    let Ok(json) = serde_json::to_vec(launch) else { return false };
    unsafe {
        let Ok(pipe) = CreateFileW(&pipe_name(), GENERIC_WRITE.0, FILE_SHARE_NONE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None)
        else {
            return false;
        };
        // The window comes to the front for the new tab: let it.
        let mut pid = 0u32;
        if GetNamedPipeServerProcessId(pipe, &mut pid).is_ok() {
            let _ = AllowSetForegroundWindow(pid);
        }
        let mut written = 0u32;
        let ok = WriteFile(pipe, Some(&json), Some(&mut written), None).is_ok() && written as usize == json.len();
        let _ = CloseHandle(pipe);
        ok
    }
}

/// Listen for launches handed over, if no other window of this session does; each one is put in
/// the inbox and `wake` called. Runs on a thread of its own for the life of the process.
pub fn listen(wake: Arc<dyn Fn() + Send + Sync>) -> Inbox {
    let inbox: Inbox = Arc::new(Mutex::new(Vec::new()));
    let out = inbox.clone();
    let _ = std::thread::Builder::new().name("instance".into()).spawn(move || serve(&inbox, wake.as_ref()));
    out
}

fn create(first: bool) -> Option<HANDLE> {
    let flags = if first { PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE } else { PIPE_ACCESS_INBOUND };
    let h = unsafe {
        CreateNamedPipeW(
            &pipe_name(),
            flags,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            0,
            MAX_MESSAGE as u32,
            0,
            None,
        )
    };
    (h != INVALID_HANDLE_VALUE && !h.is_invalid()).then_some(h)
}

fn serve(inbox: &Inbox, wake: &(dyn Fn() + Send + Sync)) {
    // Another window of this session already listens: leave it to that one.
    let Some(mut pipe) = create(true) else { return };
    loop {
        let connected = unsafe { ConnectNamedPipe(pipe, None) };
        // ERROR_PIPE_CONNECTED (a client got in first) is a success too.
        let ok = match connected {
            Ok(()) => true,
            Err(e) => e.code() == windows::Win32::Foundation::ERROR_PIPE_CONNECTED.to_hresult(),
        };
        if ok {
            if let Some(launch) = read_launch(pipe) {
                inbox.lock().unwrap().push(launch);
                wake();
            }
        }
        unsafe {
            let _ = DisconnectNamedPipe(pipe);
        }
        if !ok {
            // Something is wrong with this instance: make a new one.
            unsafe {
                let _ = CloseHandle(pipe);
            }
            match create(false) {
                Some(p) => pipe = p,
                None => return,
            }
        }
    }
}

/// Read one message to its end, and accept it only if it is a valid launch.
fn read_launch(pipe: HANDLE) -> Option<Launch> {
    let mut data = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let mut n = 0u32;
        if unsafe { ReadFile(pipe, Some(&mut buf), Some(&mut n), None) }.is_err() || n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n as usize]);
        if data.len() > MAX_MESSAGE {
            return None;
        }
    }
    decode(&data)
}

/// A message as a launch, or None when it is not one.
pub fn decode(data: &[u8]) -> Option<Launch> {
    let launch: Launch = serde_json::from_slice(data).ok()?;
    launch.check().ok()?;
    Some(launch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_valid_launches_are_taken() {
        let l = Launch { dir: Some(r"C:\x".into()), profile: Some("Debian".into()), command: None, hold: false };
        assert_eq!(decode(&serde_json::to_vec(&l).unwrap()), Some(l));
        assert_eq!(decode(b"{}"), Some(Launch::default()));
        assert_eq!(decode(b"not json"), None);
        assert_eq!(decode(br#"{"program":"x.exe"}"#), None);
        assert_eq!(decode(br#"{"hold":true}"#), None);
        assert_eq!(decode(br#"{"command":""}"#), None);
    }
}
