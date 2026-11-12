//! Noticing that notifications changed, without polling.
//!
//! The listener's change event is refused to desktop processes (0x80070490). What works: the
//! platform logs each toast delivered, cleared or cancelled, and a subscription to those log
//! events signals a kernel event as they are written (`events`). A thread waits on it, lets a
//! burst settle, and reads the store through the API again. At rest it is blocked in the kernel:
//! no CPU.
//!
//! A change notification on the store's folder is waited on as well, as a second signal; it is
//! not enough alone: the store is an SQLite database whose write-ahead log is kept open and
//! rewritten in place, so its folder reports a change only when the log grows. The database
//! itself is never opened, so the platform's service is never disturbed. Reading does not
//! write, so a read wakes neither signal.

use super::events::Subscription;
use super::model::Toast;
use super::source::Source;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Storage::FileSystem::{
    FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects, INFINITE};

/// How long things must be quiet before the store is read again: a toast is a few events.
const QUIET: Duration = Duration::from_millis(100);

/// The platform's store folder.
pub fn store_dir() -> Option<PathBuf> {
    Some(dirs::data_local_dir()?.join("Microsoft").join("Windows").join("Notifications"))
}

/// What the watch hands over after each read.
pub type OnRead = Box<dyn Fn(Result<Vec<Toast>, String>) + Send>;

/// A handle to a kernel event, used only through thread-safe Win32 calls.
#[derive(Clone, Copy)]
struct Event(isize);

impl Event {
    fn new(manual: bool) -> Self {
        Event(unsafe { CreateEventW(None, manual, false, None) }.map_or(0, |h| h.0 as isize))
    }

    fn handle(self) -> HANDLE {
        HANDLE(self.0 as *mut _)
    }
}

/// Reads the store now and after every change, on its own thread, until dropped.
pub struct Watch {
    stop: Event,
    thread: Option<JoinHandle<()>>,
}

impl Watch {
    pub fn start(source: Arc<dyn Source>, on_read: OnRead) -> Self {
        let stop = Event::new(true);
        let thread = std::thread::Builder::new()
            .name("notifications".into())
            .spawn(move || run(source.as_ref(), &on_read, stop))
            .ok();
        Self { stop, thread }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        if self.stop.0 == 0 {
            return;
        }
        unsafe {
            let _ = SetEvent(self.stop.handle());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        unsafe {
            let _ = CloseHandle(self.stop.handle());
        }
    }
}

/// The signals the thread waits on, released when it ends.
struct Signals {
    logged: Event,
    subscription: Option<Subscription>,
    folder: Option<HANDLE>,
}

impl Signals {
    fn open() -> Self {
        let logged = Event::new(false);
        let subscription = (logged.0 != 0).then(|| Subscription::start(logged.handle())).flatten();
        let folder = store_dir().and_then(|dir| unsafe {
            FindFirstChangeNotificationW(
                &HSTRING::from(dir.as_os_str()),
                false,
                FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE,
            )
            .ok()
        });
        Self { logged, subscription, folder }
    }

    /// Take what woke the thread (`which`, an index into `handles`), so it can signal again.
    fn rearm(&self, which: usize) {
        match which {
            1 if self.subscription.is_some() => {
                self.subscription.as_ref().map(Subscription::drain);
            }
            _ => {
                if let Some(folder) = self.folder {
                    unsafe {
                        let _ = FindNextChangeNotification(folder);
                    }
                }
            }
        }
    }
}

impl Drop for Signals {
    fn drop(&mut self) {
        self.subscription = None;
        unsafe {
            if let Some(folder) = self.folder {
                let _ = FindCloseChangeNotification(folder);
            }
            if self.logged.0 != 0 {
                let _ = CloseHandle(self.logged.handle());
            }
        }
    }
}

fn run(source: &dyn Source, on_read: &OnRead, stop: Event) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let signals = Signals::open();
    on_read(source.read());
    if stop.0 == 0 {
        return;
    }
    let mut handles = vec![stop.handle()];
    if signals.subscription.is_some() {
        handles.push(signals.logged.handle());
    }
    handles.extend(signals.folder);
    // Which signal an index is: 1 is the log when subscribed, the last is the folder.
    let index = |woke: u32| woke.wrapping_sub(WAIT_OBJECT_0.0) as usize;
    let which = |i: usize| if signals.subscription.is_some() && i == 1 { 1 } else { 2 };
    loop {
        let woke = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
        let i = index(woke.0);
        if i == 0 || i >= handles.len() {
            // Stopped, or the wait failed.
            return;
        }
        signals.rearm(which(i));
        // Wait out the rest of the burst: each further signal restarts the quiet period.
        loop {
            let woke = unsafe { WaitForMultipleObjects(&handles, false, QUIET.as_millis() as u32) };
            if woke == WAIT_TIMEOUT {
                break;
            }
            let i = index(woke.0);
            if i == 0 || i >= handles.len() {
                return;
            }
            signals.rearm(which(i));
        }
        on_read(source.read());
    }
}
