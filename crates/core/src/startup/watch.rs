//! Noticing that startup items changed: the three Run keys, the StartupApproved keys (what Task
//! Manager and Settings write) and the two Startup folders. One thread sleeps on Windows' own
//! change notifications, so nothing runs while nothing changes, and calls `on_change` once the
//! changes have stopped for a moment.

use super::win::startup_dirs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Storage::FileSystem::{
    FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_NOTIFY,
    REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME,
};
use windows::Win32::System::Threading::{CreateEventW, ResetEvent, SetEvent, WaitForMultipleObjects, INFINITE};

/// How long things must stay quiet before the change is reported.
const QUIET: Duration = Duration::from_millis(300);

const CV: &str = "Software\\Microsoft\\Windows\\CurrentVersion";

/// (hive, key, whole subtree).
fn registry_targets() -> Vec<(HKEY, String, bool)> {
    let approved = format!("{CV}\\Explorer\\StartupApproved");
    vec![
        (HKEY_CURRENT_USER, format!("{CV}\\Run"), false),
        (HKEY_LOCAL_MACHINE, format!("{CV}\\Run"), false),
        (HKEY_LOCAL_MACHINE, "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run".into(), false),
        (HKEY_CURRENT_USER, approved.clone(), true),
        (HKEY_LOCAL_MACHINE, approved, true),
    ]
}

/// Raw handles are not `Send`; the stop event is only used through thread-safe Win32 calls and is
/// closed after the thread has ended.
pub struct RunWatch {
    stop: isize,
    quit: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl RunWatch {
    pub fn start(on_change: impl Fn() + Send + 'static) -> Self {
        let stop = unsafe { CreateEventW(None, true, false, None) }.map_or(0, |h| h.0 as isize);
        let quit = Arc::new(AtomicBool::new(false));
        let thread = (stop != 0).then(|| {
            let quit = quit.clone();
            std::thread::Builder::new().name("startup watch".into()).spawn(move || run(stop, &quit, &on_change)).ok()
        });
        Self { stop, quit, thread: thread.flatten() }
    }
}

impl Drop for RunWatch {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::SeqCst);
        let h = HANDLE(self.stop as *mut _);
        if let Some(t) = self.thread.take() {
            unsafe {
                let _ = SetEvent(h);
            }
            let _ = t.join();
        }
        if self.stop != 0 {
            unsafe {
                let _ = CloseHandle(h);
            }
        }
    }
}

/// One thing waited on: a registry key with its event, or a folder's change handle.
enum Source {
    Key(HKEY, HANDLE, bool),
    Folder(HANDLE),
}

impl Source {
    fn handle(&self) -> HANDLE {
        match self {
            Source::Key(_, h, _) | Source::Folder(h) => *h,
        }
    }

    /// Ask for the next change.
    fn arm(&self) {
        unsafe {
            match self {
                Source::Key(key, ev, subtree) => {
                    let _ = ResetEvent(*ev);
                    let _ = RegNotifyChangeKeyValue(*key, *subtree, REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET, Some(*ev), true);
                }
                Source::Folder(h) => {
                    let _ = FindNextChangeNotification(*h);
                }
            }
        }
    }

    fn close(self) {
        unsafe {
            match self {
                Source::Key(key, ev, _) => {
                    let _ = RegCloseKey(key);
                    let _ = CloseHandle(ev);
                }
                Source::Folder(h) => {
                    let _ = FindCloseChangeNotification(h);
                }
            }
        }
    }
}

fn sources() -> Vec<Source> {
    let mut out = Vec::new();
    for (root, path, subtree) in registry_targets() {
        let mut key = HKEY::default();
        let opened = unsafe { RegOpenKeyExW(root, &HSTRING::from(path), None, KEY_NOTIFY, &mut key) }.is_ok();
        if !opened {
            continue;
        }
        match unsafe { CreateEventW(None, true, false, None) } {
            Ok(ev) => {
                let s = Source::Key(key, ev, subtree);
                s.arm();
                out.push(s);
            }
            Err(_) => unsafe {
                let _ = RegCloseKey(key);
            },
        }
    }
    for (_, dir) in startup_dirs() {
        let h = unsafe {
            FindFirstChangeNotificationW(&HSTRING::from(dir.as_os_str()), false, FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE)
        };
        if let Ok(h) = h {
            out.push(Source::Folder(h));
        }
    }
    out
}

fn run(stop: isize, quit: &AtomicBool, on_change: &dyn Fn()) {
    let stop = HANDLE(stop as *mut _);
    let sources = sources();
    let mut handles = vec![stop];
    handles.extend(sources.iter().map(Source::handle));
    let mut timeout = INFINITE;
    while !quit.load(Ordering::SeqCst) {
        let r = unsafe { WaitForMultipleObjects(&handles, false, timeout) };
        if r == WAIT_TIMEOUT {
            // Quiet for a moment: report once.
            timeout = INFINITE;
            on_change();
        } else if r == WAIT_OBJECT_0 {
            break;
        } else if r.0 > WAIT_OBJECT_0.0 && ((r.0 - WAIT_OBJECT_0.0) as usize) < handles.len() {
            sources[(r.0 - WAIT_OBJECT_0.0) as usize - 1].arm();
            timeout = QUIET.as_millis() as u32;
        } else {
            // The wait itself failed: do not spin.
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    for s in sources {
        s.close();
    }
}
