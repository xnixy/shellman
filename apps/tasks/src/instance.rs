//! Only one Task Manager: starting it again (Ctrl+Shift+Esc a second time) brings the first one
//! to the front.
//!
//! The first process holds a named mutex and waits on a named event from a thread; a second
//! process finds the mutex taken, sets the event, and exits. Nothing polls.

use windows::core::HSTRING;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject, INFINITE};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, IsIconic, SetForegroundWindow, ShowWindow, ASFW_ANY, SW_RESTORE,
};

pub enum Claim {
    First(Instance),
    Signalled,
}

/// The first process's end of the signal; keeps the mutex for as long as it lives.
pub struct Instance {
    event: HANDLE,
    _mutex: HANDLE,
}

/// A kernel object's name; `SHELLMAN_INSTANCE` is added so a test copy runs beside the real one.
fn name(what: &str) -> HSTRING {
    let instance = std::env::var("SHELLMAN_INSTANCE").unwrap_or_default();
    HSTRING::from(format!("Local\\shellman-tasks-{what}{instance}"))
}

/// Become the Task Manager, or wake the one that is. The event is made before the mutex on both
/// sides, so whichever order two starts interleave in, the second finds the event.
pub fn claim() -> Claim {
    unsafe {
        let Ok(event) = CreateEventW(None, false, false, &name("raise")) else {
            return Claim::First(Instance { event: HANDLE::default(), _mutex: HANDLE::default() });
        };
        let mutex = CreateMutexW(None, false, &name("resident"));
        if mutex.is_ok() && GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            let _ = SetEvent(event);
            return Claim::Signalled;
        }
        Claim::First(Instance { event, _mutex: mutex.unwrap_or_default() })
    }
}

impl Instance {
    /// Raise the window `hwnd` whenever another start signals, from a thread of its own.
    pub fn serve(&self, hwnd: isize, ctx: egui::Context) {
        if self.event.is_invalid() || hwnd == 0 {
            return;
        }
        let event = self.event.0 as isize;
        std::thread::spawn(move || {
            let event = HANDLE(event as *mut _);
            let hwnd = HWND(hwnd as *mut _);
            while unsafe { WaitForSingleObject(event, INFINITE) } == WAIT_OBJECT_0 {
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
