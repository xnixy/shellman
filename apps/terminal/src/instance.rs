//! A second launch handing its tab to the window already open (`new_instance: "tab"`).
//!
//! The first window of a sign-in session listens on a named pipe,
//! `\\.\pipe\shellman-terminal-<session id>`; a later launch with `new_instance: "tab"` writes its
//! [`Launch`] there as JSON and exits. Only that: a folder, a profile name, a command line and
//! `hold`, re-checked on arrival ([`Launch::check`], unknown fields refused).
//!
//! Pipe names are global, so anyone could make one with this name first. Hence:
//! - the window makes the pipe as its first instance (`FILE_FLAG_FIRST_PIPE_INSTANCE`, also when
//!   it makes it again), with a DACL that lets only this user open it, and takes no remote
//!   clients; if the name is taken it does not listen;
//! - a launch hands over only to a server that runs as this user in this session (the server's
//!   process, from `GetNamedPipeServerProcessId`, its token's user SID), checked before anything
//!   is written or the server is allowed to come to the front;
//! - the window reads a message with a deadline (`READ_FOR`) and a size cap (`MAX_MESSAGE`), so a
//!   client that connects and never writes (or never stops) holds it up for a second at most.

use crate::cli::Launch;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, LocalFree, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_WRITE, HANDLE, HLOCAL,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::Security::{EqualSid, GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE,
    OPEN_EXISTING, PIPE_ACCESS_INBOUND,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId, WaitNamedPipeW, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken, WaitForSingleObject, INFINITE,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

/// The most a launch message may be.
const MAX_MESSAGE: usize = 128 * 1024;
/// How long a client has, from connecting, to write its whole message.
const READ_FOR: Duration = Duration::from_secs(1);
/// How long a launch waits for the window while it is busy with another client.
const BUSY_WAIT_MS: u32 = 2000;

fn pipe_name() -> HSTRING {
    let mut session = 0u32;
    unsafe {
        let _ = ProcessIdToSessionId(GetCurrentProcessId(), &mut session);
    }
    HSTRING::from(format!(r"\\.\pipe\shellman-terminal-{session}"))
}

/// Launches other processes handed over, for the window to open as tabs.
pub type Inbox = Arc<Mutex<Vec<Launch>>>;

/// Hand `launch` to the window already open. False when there is none (or it would not take it,
/// or the pipe is not this user's own window's), so this process opens its own.
pub fn hand_over(launch: &Launch) -> bool {
    hand_over_to(&pipe_name(), launch)
}

fn hand_over_to(name: &HSTRING, launch: &Launch) -> bool {
    let Ok(json) = serde_json::to_vec(launch) else { return false };
    if json.len() > MAX_MESSAGE {
        return false;
    }
    let Some(pipe) = open_client(name) else { return false };
    let ok = unsafe {
        let mut pid = 0u32;
        // Nothing goes to a server that is not this user's, in this session.
        if GetNamedPipeServerProcessId(pipe, &mut pid).is_ok() && trusted_server(pid) {
            // The window comes to the front for the new tab: let it.
            let _ = AllowSetForegroundWindow(pid);
            let mut written = 0u32;
            WriteFile(pipe, Some(&json), Some(&mut written), None).is_ok() && written as usize == json.len()
        } else {
            false
        }
    };
    unsafe {
        let _ = CloseHandle(pipe);
    }
    ok
}

/// Connect to the pipe, waiting a moment if the window is busy with another launch.
fn open_client(name: &HSTRING) -> Option<HANDLE> {
    for attempt in 0..2 {
        let opened = unsafe { CreateFileW(name, GENERIC_WRITE.0, FILE_SHARE_NONE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) };
        match opened {
            Ok(h) => return Some(h),
            Err(e) if attempt == 0 && e.code() == ERROR_PIPE_BUSY.to_hresult() => {
                if !unsafe { WaitNamedPipeW(name, BUSY_WAIT_MS) }.as_bool() {
                    return None;
                }
            }
            Err(_) => return None,
        }
    }
    None
}

/// A process's token's user (a `TOKEN_USER` and the SID after it), or None when it cannot be
/// read (another user's process usually cannot).
fn token_user(process: HANDLE) -> Option<Vec<u64>> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let mut len = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        // u64s, so the buffer is aligned for TOKEN_USER.
        let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
        let got = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr().cast()), (buf.len() * 8) as u32, &mut len);
        let _ = CloseHandle(token);
        got.ok()?;
        Some(buf)
    }
}

fn sid_of(user: &[u64]) -> windows::Win32::Security::PSID {
    unsafe { (*(user.as_ptr() as *const TOKEN_USER)).User.Sid }
}

/// Whether process `pid` runs in this session as this user.
fn trusted_server(pid: u32) -> bool {
    let mut theirs = u32::MAX;
    let mut ours = 0u32;
    unsafe {
        if ProcessIdToSessionId(pid, &mut theirs).is_err() || ProcessIdToSessionId(GetCurrentProcessId(), &mut ours).is_err() || theirs != ours {
            return false;
        }
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let user = token_user(process);
        let _ = CloseHandle(process);
        let (Some(user), Some(me)) = (user, token_user(GetCurrentProcess())) else { return false };
        EqualSid(sid_of(&user), sid_of(&me)).is_ok()
    }
}

/// This user's SID as text (`S-1-5-21-...`).
fn my_sid() -> Option<String> {
    let me = token_user(unsafe { GetCurrentProcess() })?;
    unsafe {
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(sid_of(&me), &mut text).ok()?;
        let s = text.to_string().ok();
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        s
    }
}

/// A security descriptor that lets only this user open the pipe (no Everyone read, no
/// administrators or SYSTEM), kept for the life of the listener.
struct OnlyMe(PSECURITY_DESCRIPTOR);

impl OnlyMe {
    fn new() -> Option<Self> {
        let sddl = HSTRING::from(format!("D:P(A;;GA;;;{})", my_sid()?));
        let mut sd = PSECURITY_DESCRIPTOR::default();
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut sd, None) }.ok()?;
        Some(Self(sd))
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: self.0 .0, bInheritHandle: false.into() }
    }
}

impl Drop for OnlyMe {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0 .0)));
        }
    }
}

/// Listen for launches handed over, if no other window of this session does; each one is put in
/// the inbox and `wake` called. Runs on a thread of its own for the life of the process.
pub fn listen(wake: Arc<dyn Fn() + Send + Sync>) -> Inbox {
    let inbox: Inbox = Arc::new(Mutex::new(Vec::new()));
    let out = inbox.clone();
    let _ = std::thread::Builder::new().name("instance".into()).spawn(move || serve(&pipe_name(), &inbox, wake.as_ref()));
    out
}

/// The pipe's one instance, made only if nobody has the name (also when made again after an
/// error: a name someone else took in between is left to them, and nothing listens).
fn create(name: &HSTRING, security: &OnlyMe) -> Option<HANDLE> {
    let sa = security.attributes();
    let h = unsafe {
        CreateNamedPipeW(
            name,
            PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            0,
            MAX_MESSAGE as u32,
            0,
            Some(&sa),
        )
    };
    (h != INVALID_HANDLE_VALUE && !h.is_invalid()).then_some(h)
}

fn serve(name: &HSTRING, inbox: &Inbox, wake: &(dyn Fn() + Send + Sync)) {
    let Some(security) = OnlyMe::new() else { return };
    // Another window of this session already listens (or someone else has the name): leave it.
    let Some(mut pipe) = create(name, &security) else { return };
    let Ok(event) = (unsafe { CreateEventW(None, true, false, None) }) else { return };
    loop {
        let ok = connect(pipe, event);
        if ok {
            if let Some(launch) = read_launch(pipe, event, READ_FOR) {
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
            match create(name, &security) {
                Some(p) => pipe = p,
                None => break,
            }
        }
    }
    unsafe {
        let _ = CloseHandle(event);
    }
}

/// Wait for a client. False when the instance is broken.
fn connect(pipe: HANDLE, event: HANDLE) -> bool {
    let mut ov = OVERLAPPED { hEvent: event, ..Default::default() };
    match unsafe { ConnectNamedPipe(pipe, Some(&mut ov)) } {
        Ok(()) => true,
        // A client got in first.
        Err(e) if e.code() == ERROR_PIPE_CONNECTED.to_hresult() => true,
        Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => unsafe {
            if WaitForSingleObject(event, INFINITE) != WAIT_OBJECT_0 {
                let _ = CancelIoEx(pipe, Some(&ov));
                let mut n = 0;
                let _ = GetOverlappedResult(pipe, &ov, &mut n, true);
                return false;
            }
            let mut n = 0;
            GetOverlappedResult(pipe, &ov, &mut n, false).is_ok()
        },
        Err(_) => false,
    }
}

/// Read one message to its end (the client closing), within `within` of now and `MAX_MESSAGE`,
/// and accept it only if it is a valid launch. A client too slow or too long gets nothing taken.
fn read_launch(pipe: HANDLE, event: HANDLE, within: Duration) -> Option<Launch> {
    let deadline = Instant::now() + within;
    let mut data = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let mut ov = OVERLAPPED { hEvent: event, ..Default::default() };
        let mut n = 0u32;
        let started = unsafe { ReadFile(pipe, Some(&mut buf), None, Some(&mut ov)) };
        let done = match started {
            Ok(()) => unsafe { GetOverlappedResult(pipe, &ov, &mut n, false) },
            Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => unsafe {
                let left = deadline.saturating_duration_since(Instant::now()).as_millis() as u32;
                if WaitForSingleObject(event, left) != WAIT_OBJECT_0 {
                    // Too slow: give up on it (the read must end before `buf` goes).
                    let _ = CancelIoEx(pipe, Some(&ov));
                    let _ = GetOverlappedResult(pipe, &ov, &mut n, true);
                    return None;
                }
                GetOverlappedResult(pipe, &ov, &mut n, false)
            },
            Err(e) => Err(e),
        };
        match done {
            Ok(()) if n > 0 => {
                data.extend_from_slice(&buf[..n as usize]);
                if data.len() > MAX_MESSAGE {
                    return None;
                }
            }
            Ok(()) => break,
            // The client wrote and closed: the end of the message.
            Err(e) if e.code() == ERROR_BROKEN_PIPE.to_hresult() => break,
            Err(_) => return None,
        }
        if Instant::now() >= deadline {
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
    use std::sync::atomic::{AtomicU32, Ordering};

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

    /// A pipe name of this test's own.
    fn test_name() -> HSTRING {
        static N: AtomicU32 = AtomicU32::new(0);
        HSTRING::from(format!(r"\\.\pipe\shellman-terminal-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)))
    }

    /// A listener on `name`, and its inbox, told of each arrival.
    fn listener(name: &HSTRING) -> (Inbox, std::sync::mpsc::Receiver<()>) {
        let inbox: Inbox = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = std::sync::mpsc::channel();
        let (i, n) = (inbox.clone(), name.clone());
        let tx = Mutex::new(tx);
        std::thread::spawn(move || serve(&n, &i, &move || {
            let _ = tx.lock().unwrap().send(());
        }));
        // Until it listens.
        let start = Instant::now();
        while !unsafe { WaitNamedPipeW(name, 50) }.as_bool() && start.elapsed() < Duration::from_secs(5) {}
        (inbox, rx)
    }

    #[test]
    fn a_launch_reaches_this_user_s_own_window() {
        let name = test_name();
        let (inbox, rx) = listener(&name);
        let l = Launch { dir: Some(r"C:\x".into()), ..Default::default() };
        assert!(hand_over_to(&name, &l));
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(inbox.lock().unwrap().as_slice(), [l]);
    }

    #[test]
    fn a_client_that_never_writes_holds_the_window_up_a_second_at_most() {
        let name = test_name();
        let (inbox, rx) = listener(&name);
        // Connects, writes nothing, stays.
        let silent = unsafe { CreateFileW(&name, GENERIC_WRITE.0, FILE_SHARE_NONE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) }.unwrap();
        let l = Launch { profile: Some("Debian".into()), ..Default::default() };
        let (n, l2) = (name.clone(), l.clone());
        let later = std::thread::spawn(move || hand_over_to(&n, &l2));
        rx.recv_timeout(Duration::from_secs(5)).expect("the next launch got through");
        assert!(later.join().unwrap());
        assert_eq!(inbox.lock().unwrap().as_slice(), [l]);
        unsafe {
            let _ = CloseHandle(silent);
        }
    }

    #[test]
    fn a_taken_name_is_not_listened_on_again() {
        let name = test_name();
        let only = OnlyMe::new().unwrap();
        let first = create(&name, &only).unwrap();
        assert!(create(&name, &only).is_none());
        unsafe {
            let _ = CloseHandle(first);
        }
    }

    #[test]
    fn only_this_user_s_processes_in_this_session_are_trusted() {
        assert!(trusted_server(std::process::id()));
        // System (pid 4) and the idle process: another user, session 0.
        assert!(!trusted_server(4));
        assert!(!trusted_server(0));
    }

    #[test]
    fn the_pipe_lets_only_this_user_in() {
        use windows::Win32::Security::Authorization::{ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SE_KERNEL_OBJECT};
        use windows::Win32::Security::DACL_SECURITY_INFORMATION;
        let name = test_name();
        let only = OnlyMe::new().unwrap();
        let pipe = create(&name, &only).unwrap();
        let text = unsafe {
            let mut sd = PSECURITY_DESCRIPTOR::default();
            assert!(GetSecurityInfo(pipe, SE_KERNEL_OBJECT, DACL_SECURITY_INFORMATION, None, None, None, None, Some(&mut sd)).is_ok());
            let mut s = PWSTR::null();
            ConvertSecurityDescriptorToStringSecurityDescriptorW(sd, SDDL_REVISION_1, DACL_SECURITY_INFORMATION, &mut s, None).unwrap();
            let t = s.to_string().unwrap();
            let _ = LocalFree(Some(HLOCAL(s.0.cast())));
            let _ = LocalFree(Some(HLOCAL(sd.0)));
            let _ = CloseHandle(pipe);
            t
        };
        let me = my_sid().unwrap();
        assert_eq!(text.matches("(A;").count(), 1, "{text}");
        assert!(text.contains(&me), "{text}");
        assert!(!text.contains(";WD)") && !text.contains(";AN)") && !text.contains(";BA)") && !text.contains(";SY)"), "{text}");
    }
}
