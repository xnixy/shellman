//! Only one settings window: running the app again brings the first one to the front.
//!
//! The first process holds a named mutex and waits on a named event from a thread; a second
//! process finds the mutex taken, sets the event, and exits. The event wakes the waiting thread,
//! so nothing polls. A run that asks for a page (`--page sound`) also sets that page's event, so
//! the window it raises opens on it.

use crate::sections::Section;
use std::sync::{Arc, Mutex};
use windows::core::HSTRING;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, SetEvent, WaitForMultipleObjects, INFINITE,
};
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
    /// One event per page a later run can ask for.
    pages: Vec<(HANDLE, Section)>,
    /// The page the latest such run asked for, until the window takes it.
    pub requested: Arc<Mutex<Option<Section>>>,
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
pub fn claim(page: Option<Section>) -> Claim {
    unsafe {
        let none = || Instance {
            event: HANDLE::default(),
            pages: Vec::new(),
            requested: Arc::default(),
            _mutex: HANDLE::default(),
        };
        let Ok(event) = CreateEventW(None, false, false, &name("raise")) else {
            // Cannot coordinate; run on our own rather than not at all.
            return Claim::First(none());
        };
        let page_event = |section: Section| {
            CreateEventW(None, false, false, &name(&format!("page-{}", section.slug()))).ok()
        };
        let pages: Vec<(HANDLE, Section)> =
            Section::ALL.iter().filter_map(|&s| Some((page_event(s)?, s))).collect();
        let mutex = CreateMutexW(None, false, &name("resident"));
        if mutex.is_ok() && GetLastError() == ERROR_ALREADY_EXISTS {
            // The first process may take the foreground for the window it is about to raise.
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            if let Some((h, _)) = pages.iter().find(|(_, s)| Some(*s) == page) {
                let _ = SetEvent(*h);
            }
            let _ = SetEvent(event);
            return Claim::Signalled;
        }
        Claim::First(Instance { event, pages, requested: Arc::default(), _mutex: mutex.unwrap_or_default() })
    }
}

impl Instance {
    /// Raise the window `hwnd` whenever another run signals, from a thread of its own.
    pub fn serve(&self, hwnd: isize, ctx: egui::Context) {
        if self.event.is_invalid() || hwnd == 0 {
            return;
        }
        let mut handles = vec![self.event.0 as isize];
        handles.extend(self.pages.iter().map(|(h, _)| h.0 as isize));
        let sections: Vec<Section> = self.pages.iter().map(|(_, s)| *s).collect();
        let requested = self.requested.clone();
        std::thread::spawn(move || {
            let handles: Vec<HANDLE> = handles.iter().map(|&h| HANDLE(h as *mut _)).collect();
            let hwnd = HWND(hwnd as *mut _);
            loop {
                let woke = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) }.0;
                let Some(i) = woke.checked_sub(WAIT_OBJECT_0.0).map(|i| i as usize).filter(|&i| i < handles.len())
                else {
                    break;
                };
                // A run that asks for a page sets two events; each wakes this loop in turn, and the
                // window shows the page by the second. One that asks for none leaves it as it is.
                if i > 0 {
                    if let Ok(mut r) = requested.lock() {
                        *r = Some(sections[i - 1]);
                    }
                }
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
