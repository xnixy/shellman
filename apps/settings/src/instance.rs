//! Only one settings window: running the app again brings the first one to the front.
//!
//! The first process holds a named mutex and waits on a named event from a thread; a second
//! process finds the mutex taken, sets the event, and exits. The event wakes the waiting thread,
//! so nothing polls.

use windows::core::HSTRING;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND};
use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject, INFINITE};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, IsIconic, SetForegroundWindow, ShowWindow, ASFW_ANY, SW_RESTORE,
};

/// Whether this process is the settings window; if not, the first one has been signalled and
/// this process has nothing more to do.
pub enum Claim {
    First(Instance),
    Signalled,
}

/// The first process's end of the signal.
pub struct Instance {
    event: HANDLE,
    /// Keeps the mutex alive for as long as the process runs.
    _mutex: HANDLE,
}

/// A kernel object's name. `SHELLMAN_INSTANCE` is added to it, so a test build can run next to the
/// real window without the two finding each other (the launcher does the same).
fn name(what: &str) -> HSTRING {
    let instance = std::env::var("SHELLMAN_INSTANCE").unwrap_or_default();
    HSTRING::from(format!("Local\\shellman-settings-{what}{instance}"))
}

/// Become the first settings window, or signal the one that is. Both sides create the event
/// before the mutex, so whichever order two starts interleave in, the loser finds the event.
pub fn claim() -> Claim {
    unsafe {
        let Ok(event) = CreateEventW(None, false, false, &name("raise")) else {
            // Cannot coordinate; run on our own rather than not at all.
            return Claim::First(Instance { event: HANDLE::default(), _mutex: HANDLE::default() });
        };
        let mutex = CreateMutexW(None, false, &name("resident"));
        if mutex.is_ok() && GetLastError() == ERROR_ALREADY_EXISTS {
            // The first process may take the foreground for the window it is about to raise.
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            let _ = SetEvent(event);
            return Claim::Signalled;
        }
        Claim::First(Instance { event, _mutex: mutex.unwrap_or_default() })
    }
}

impl Instance {
    /// Raise the window `hwnd` whenever another run signals, from a thread of its own.
    pub fn serve(&self, hwnd: isize, ctx: egui::Context) {
        if self.event.is_invalid() || hwnd == 0 {
            return;
        }
        let event = self.event.0 as isize;
        std::thread::spawn(move || {
            let event = HANDLE(event as *mut _);
            let hwnd = HWND(hwnd as *mut _);
            while unsafe { WaitForSingleObject(event, INFINITE) }.0 == 0 {
                unsafe {
                    if IsIconic(hwnd).as_bool() {
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                    }
                    let _ = SetForegroundWindow(hwnd);
                }
                ctx.request_repaint();
            }
        });
    }
}
