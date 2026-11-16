//! Tells Windows the session's shell is up, as Microsoft's own shells do.
//!
//! The Background Tasks Infrastructure service (`bisrv.dll`) buffers a new session's background
//! work until the shell says the session is complete: with `HKLM\SOFTWARE\Microsoft\Windows NT\
//! CurrentVersion\BackgroundModel` `WaitForNewSessionComplete` = 1 (Windows' default) no packaged
//! app's background task or app service runs before that. Seen with Dolby Access OEM, whose
//! `SessionConnected` and audio-device tasks never started under shellman and started within two
//! seconds of any Explorer or ShellAppRuntime (10.0.26300), so Atmos speakers had no licence and
//! stayed silent.
//!
//! What the shells do, read from the binaries (10.0.26300):
//!
//! - `rdpinit.exe` (the RemoteApp shell, `RemoteAppImmersiveHost`): publishes WNF state
//!   `0x0D83063EA3BC1875`, then `0x0D83063EA3BE3835` (both with no data), then calls
//!   `BiNotifyNewSessionComplete` (twinapi.appcore.dll, no arguments, an RPC to the service, which
//!   takes the session from the caller). Nothing else. This is what shellman does.
//! - Explorer and ShellAppRuntime (`shell\lib\logontasks\logontasks.cpp`, shared): at the end of
//!   their logon tasks the same two publishes and the same call, plus things shellman leaves out
//!   on purpose: deleting `FSIAInProgress` (first-sign-in animation), message 0x5BD to
//!   `Shell_TrayWnd`, `ShellLauncherReadyEvent`, two private user32 ordinals; and earlier, in the
//!   ContentDeliveryManager logon task, `BiChangeSessionState(5)` right before registering
//!   ContentDeliveryManager's OEM preinstall background task (CustomShellHost, Shell Launcher,
//!   links the same logon-task code). rdpinit makes none of these. That none is needed is not
//!   proven: what state 5 does inside the service was not found, so if tasks still do not start
//!   at a shellman sign-in, `BiChangeSessionState(5)` before the publishes is the first suspect.
//!
//! The two states are Microsoft's shell family (`SHEL`), well-known, not persistent: the first
//! machine-scoped (sequence 3), the second session-scoped (sequence 71). Their names are not in
//! the binaries; by where they are published they say "logon complete". Subscribers in System32
//! include AudioSrvPolicyManager.dll (the first), cdpusersvc.dll and PenService.dll (the second),
//! twinui.dll and ContentDeliveryManager. Their security descriptors (`HKLM\SYSTEM\
//! CurrentControlSet\Control\Notifications\<name>`) give Authenticated Users publish rights, so
//! the session publishes them unelevated.
//!
//! The session-scoped state's change stamp says whether this session was already announced (by
//! Explorer, ShellAppRuntime or an earlier shellman session process after a restart): then the
//! publishes are skipped and only the call is made, which answers 0x8007054F (ERROR_INTERNAL_ERROR)
//! and changes nothing in a session whose shell already made it (measured beside Explorer). A
//! failed call is retried a few times, in case the service does not know the session yet this
//! early in the logon. Every answer goes to the session log.

use crate::log;
use std::time::Duration;
use windows::core::{s, w, PCSTR};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};

/// WNF `SHEL`, sequence 3, machine scope: published at the end of every shell's logon.
pub const MACHINE_LOGON: u64 = 0x0D83_063E_A3BC_1875;
/// WNF `SHEL`, sequence 71, session scope: published right after it.
pub const SESSION_LOGON: u64 = 0x0D83_063E_A3BE_3835;

/// Waits before each attempt at the call; the first is at once.
const TRIES: [Duration; 5] = [
    Duration::ZERO,
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
];

/// The service's answer when the session's shell already made the call.
const ALREADY: i32 = 0x8007_054Fu32 as i32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// Publish this WNF state with no data.
    Publish(u64),
    /// Call `BiNotifyNewSessionComplete`.
    Notify,
}

/// What to do, in rdpinit's order. `session_stamp`: the change stamp of `SESSION_LOGON`, None
/// when it cannot be read (then publish anyway: a duplicate is what Explorer does too).
pub fn steps(session_stamp: Option<u32>) -> Vec<Step> {
    match session_stamp {
        Some(n) if n > 0 => vec![Step::Notify],
        _ => vec![Step::Publish(MACHINE_LOGON), Step::Publish(SESSION_LOGON), Step::Notify],
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Answer {
    /// The service took it.
    Done,
    /// Failed; try again after the next wait.
    Retry,
    /// This Windows has no such export: background tasks stay buffered.
    Missing,
}

/// What a call's result means. 0x8007054F is retried too: it is also what a session the service
/// does not know yet might get, and repeating the call is harmless.
pub fn classify(hr: Option<i32>) -> Answer {
    match hr {
        None => Answer::Missing,
        Some(hr) if hr >= 0 => Answer::Done,
        Some(_) => Answer::Retry,
    }
}

/// One line for the log about a call's result.
fn describe(hr: i32, try_no: usize) -> String {
    if hr >= 0 {
        format!("ready: told Windows the session is complete (try {try_no}), background tasks may run")
    } else if hr == ALREADY {
        format!("ready: BiNotifyNewSessionComplete answered 0x{:08X} (try {try_no} of {}): already told by this session's shell, or the service does not know the session yet", hr as u32, TRIES.len())
    } else {
        format!("ready: BiNotifyNewSessionComplete answered 0x{:08X} (try {try_no} of {})", hr as u32, TRIES.len())
    }
}

fn ntdll(name: PCSTR) -> Option<unsafe extern "system" fn() -> isize> {
    unsafe { GetProcAddress(GetModuleHandleW(w!("ntdll.dll")).ok()?, name) }
}

/// The change stamp of a WNF state in this session (0: never published), or the NTSTATUS.
pub fn stamp(name: u64) -> Result<u32, i32> {
    type Query = unsafe extern "system" fn(*const u64, *const u8, *const u8, *mut u32, *mut u8, *mut u32) -> i32;
    let f = ntdll(s!("NtQueryWnfStateData")).ok_or(0xC000_007Au32 as i32)?;
    let f: Query = unsafe { std::mem::transmute(f) };
    let mut stamp = 0u32;
    let mut buf = [0u8; 64];
    let mut size = buf.len() as u32;
    let status = unsafe { f(&name, std::ptr::null(), std::ptr::null(), &mut stamp, buf.as_mut_ptr(), &mut size) };
    if status < 0 { Err(status) } else { Ok(stamp) }
}

/// Publish a WNF state with no data, as the shells do (`RtlPublishWnfStateData(name, NULL, NULL,
/// 0, NULL)`). The NTSTATUS on failure.
fn publish(name: u64) -> Result<(), i32> {
    type Publish = unsafe extern "system" fn(u64, *const u8, *const u8, u32, *const u8) -> i32;
    let f = ntdll(s!("RtlPublishWnfStateData")).ok_or(0xC000_007Au32 as i32)?;
    let f: Publish = unsafe { std::mem::transmute(f) };
    let status = unsafe { f(name, std::ptr::null(), std::ptr::null(), 0, std::ptr::null()) };
    if status < 0 { Err(status) } else { Ok(()) }
}

/// One call. `None`: this Windows has no such export.
fn notify() -> Option<i32> {
    unsafe {
        let lib = LoadLibraryExW(w!("twinapi.appcore.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        let f = GetProcAddress(lib, s!("BiNotifyNewSessionComplete"))?;
        let f: unsafe extern "system" fn() -> i32 = std::mem::transmute(f);
        Some(f())
    }
}

fn run() {
    let session = stamp(SESSION_LOGON);
    match session {
        Ok(n) => log::line(&format!("ready: session logon state 0x{SESSION_LOGON:016X} has stamp {n}{}", if n > 0 { ", already announced" } else { "" })),
        Err(st) => log::line(&format!("ready: could not read 0x{SESSION_LOGON:016X} (0x{:08X})", st as u32)),
    }
    for step in steps(session.ok()) {
        match step {
            Step::Publish(name) => match publish(name) {
                Ok(()) => log::line(&format!("ready: published 0x{name:016X}")),
                Err(st) => log::line(&format!("ready: publishing 0x{name:016X} failed (0x{:08X})", st as u32)),
            },
            Step::Notify => {
                for (n, wait) in TRIES.iter().enumerate() {
                    std::thread::sleep(*wait);
                    let hr = notify();
                    match classify(hr) {
                        Answer::Missing => return log::line("ready: twinapi.appcore.dll has no BiNotifyNewSessionComplete; background tasks stay buffered"),
                        answer => {
                            log::line(&describe(hr.unwrap_or_default(), n + 1));
                            if answer == Answer::Done {
                                return;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// On a thread of its own (the call is an RPC): the session never waits on it.
pub fn start() {
    let _ = std::thread::Builder::new().name("ready".into()).spawn(run);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Well-known names are stored XORed with this constant (phnt `WNF_STATE_NAME`).
    fn plain(name: u64) -> u64 {
        name ^ 0x41C6_4E6D_A3BC_0074
    }

    #[test]
    fn the_states_are_well_known_shell_names_machine_then_session() {
        for (name, scope, seq) in [(MACHINE_LOGON, 0, 3), (SESSION_LOGON, 1, 71)] {
            let p = plain(name);
            assert_eq!(p & 0xF, 1, "version");
            assert_eq!((p >> 4) & 0x3, 0, "lifetime: well-known");
            assert_eq!((p >> 6) & 0xF, scope, "scope");
            assert_eq!((p >> 10) & 0x1, 0, "not permanent");
            assert_eq!((p >> 11) & 0x1F_FFFF, seq, "sequence");
            assert_eq!(&(p >> 32).to_le_bytes()[..4], b"SHEL");
        }
    }

    #[test]
    fn a_new_session_gets_both_publishes_then_the_call_in_rdpinits_order() {
        let all = vec![Step::Publish(MACHINE_LOGON), Step::Publish(SESSION_LOGON), Step::Notify];
        assert_eq!(steps(Some(0)), all);
        assert_eq!(steps(None), all);
    }

    #[test]
    fn an_announced_session_only_gets_the_harmless_call() {
        assert_eq!(steps(Some(1)), vec![Step::Notify]);
        assert_eq!(steps(Some(2)), vec![Step::Notify]);
    }

    #[test]
    fn success_stops_failures_retry_and_a_missing_export_gives_up() {
        assert_eq!(classify(Some(0)), Answer::Done);
        assert_eq!(classify(Some(1)), Answer::Done);
        assert_eq!(classify(Some(ALREADY)), Answer::Retry);
        assert_eq!(classify(Some(0x8007_06BAu32 as i32)), Answer::Retry);
        assert_eq!(classify(None), Answer::Missing);
    }

    #[test]
    fn retries_start_at_once_and_end_within_a_minute() {
        assert_eq!(TRIES[0], Duration::ZERO);
        assert!(TRIES.windows(2).all(|w| w[0] < w[1]));
        assert!(TRIES.iter().sum::<Duration>() < Duration::from_secs(60));
    }

    /// Live, beside whatever shell runs: reads both stamps and makes the call once (harmless in an
    /// announced session). Publishes nothing. `cargo test -p shellman-session live_probe -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_probe() {
        let session = stamp(SESSION_LOGON);
        println!("machine stamp {:?}, session stamp {:?}", stamp(MACHINE_LOGON), session);
        let plan = steps(session.ok());
        println!("steps {plan:?}");
        assert_eq!(plan.last(), Some(&Step::Notify));
        if plan.len() == 1 {
            let hr = notify();
            println!("BiNotifyNewSessionComplete -> {:?} = {:?}", hr.map(|h| format!("0x{:08X}", h as u32)), classify(hr));
        } else {
            println!("session not announced: not calling from a test");
        }
    }

    /// Live: runs `publish` and `stamp` on a temporary user-scoped WNF state of its own, which
    /// nothing subscribes to, then deletes it. `cargo test -p shellman-session live_publish -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_publish() {
        use windows::core::w;
        use windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
        use windows::Win32::Security::PSECURITY_DESCRIPTOR;
        type Create = unsafe extern "system" fn(*mut u64, u32, u32, u8, *const u8, u32, *const core::ffi::c_void) -> i32;
        type Delete = unsafe extern "system" fn(*const u64) -> i32;
        let create: Create = unsafe { std::mem::transmute(ntdll(s!("NtCreateWnfStateName")).unwrap()) };
        let delete: Delete = unsafe { std::mem::transmute(ntdll(s!("NtDeleteWnfStateName")).unwrap()) };
        let mut sd = PSECURITY_DESCRIPTOR::default();
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(w!("D:(A;;GA;;;WD)"), SDDL_REVISION_1, &mut sd, None) }.unwrap();
        let mut name = 0u64;
        // Temporary lifetime (3), user scope (2), not persistent, 4 KB.
        let st = unsafe { create(&mut name, 3, 2, 0, std::ptr::null(), 0x1000, sd.0) };
        println!("create -> 0x{:08X}, name 0x{name:016X}", st as u32);
        assert!(st >= 0);
        let before = stamp(name);
        let published = publish(name);
        let after = stamp(name);
        println!("stamp before {before:?}, publish {published:?}, stamp after {after:?}");
        let st = unsafe { delete(&name) };
        println!("delete -> 0x{:08X}", st as u32);
        assert_eq!(published, Ok(()));
        assert_eq!((before, after), (Ok(0), Ok(1)));
    }

    #[test]
    fn the_log_says_what_0x8007054f_can_mean() {
        assert!(describe(ALREADY, 1).contains("already told"));
        assert!(describe(0, 2).contains("try 2"));
        assert!(describe(0x8007_06BAu32 as i32, 3).contains("0x800706BA"));
    }
}
