//! Tells Windows the session's shell is up, as Explorer does.
//!
//! The Background Tasks Infrastructure service (`bisrv.dll`) buffers a new session's background
//! work until the shell says it is ready: Explorer and ShellAppRuntime call
//! `BiNotifyNewSessionComplete` (twinapi.appcore.dll, no arguments, an RPC to the service, which
//! takes the session from the caller). Without that call no packaged app's background task or app
//! service runs: seen with Dolby Access OEM, whose `SessionConnected` and audio-device tasks never
//! started under shellman and started within two seconds of any Explorer or ShellAppRuntime
//! (10.0.26300), so Atmos speakers had no licence and stayed silent.
//!
//! In a session whose shell already made the call it answers 0x8007054F and changes nothing, so it
//! is safe beside Explorer. A failure is retried a few times, in case the service does not know the
//! session yet this early in the logon. Every answer goes to the session log.

use crate::log;
use std::time::Duration;
use windows::core::{s, w};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};

/// Waits before each attempt; the first is at once.
const TRIES: [Duration; 5] = [
    Duration::ZERO,
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
];

/// One call. `None`: this Windows has no such export.
fn notify() -> Option<i32> {
    unsafe {
        let lib = LoadLibraryExW(w!("twinapi.appcore.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        let f = GetProcAddress(lib, s!("BiNotifyNewSessionComplete"))?;
        let f: unsafe extern "system" fn() -> i32 = std::mem::transmute(f);
        Some(f())
    }
}

/// On a thread of its own (the call is an RPC): the session never waits on it.
pub fn start() {
    let _ = std::thread::Builder::new().name("ready".into()).spawn(|| {
        for (n, wait) in TRIES.iter().enumerate() {
            std::thread::sleep(*wait);
            match notify() {
                None => return log::line("ready: twinapi.appcore.dll has no BiNotifyNewSessionComplete; background tasks stay buffered"),
                Some(hr) if hr >= 0 => return log::line(&format!("ready: told Windows the session is complete (try {}), background tasks may run", n + 1)),
                Some(hr) => log::line(&format!("ready: BiNotifyNewSessionComplete answered 0x{:08X} (try {} of {})", hr as u32, n + 1, TRIES.len())),
            }
        }
    });
}
