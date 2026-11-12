//! Folder watching, so a tab shows what is on disk without F5.
//!
//! One background thread waits on a Win32 change notification per watched folder, plus a wake
//! event that `set_dirs` and `Drop` signal. Notifications carry no file names, which suits the
//! app: any change just re-lists the folder.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Storage::FileSystem::{
    FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification,
    FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
};
use windows::Win32::System::Threading::{
    CreateEventW, ResetEvent, SetEvent, WaitForMultipleObjects, INFINITE,
};

/// How long a folder must stay quiet before it is reported, so a big copy re-lists a few times
/// rather than once per file.
const QUIET: Duration = Duration::from_millis(250);

/// WaitForMultipleObjects takes at most this many handles; one is the wake event.
const MAXIMUM_WAIT_OBJECTS: usize = 64;

/// Raw handles are not `Send`; the wake event is only ever used through Win32 calls, which are
/// thread-safe, and is closed after the thread has been joined.
#[derive(Clone, Copy)]
struct Wake(isize);

impl Wake {
    fn handle(self) -> HANDLE {
        HANDLE(self.0 as *mut _)
    }
}

struct Shared {
    dirs: Mutex<Vec<PathBuf>>,
    ready: Mutex<Vec<PathBuf>>,
    stop: AtomicBool,
    quiet: Duration,
}

pub struct Watcher {
    shared: Arc<Shared>,
    wake: Wake,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    pub fn new(ctx: egui::Context) -> Self {
        Self::with_quiet(ctx, QUIET)
    }

    /// For a folder of small settings files, where a change should show at once (a slider being
    /// dragged in another app) and there is no flood of files to wait out.
    pub fn quick(ctx: egui::Context) -> Self {
        Self::with_quiet(ctx, Duration::from_millis(40))
    }

    fn with_quiet(ctx: egui::Context, quiet: Duration) -> Self {
        let shared = Arc::new(Shared {
            dirs: Mutex::new(Vec::new()),
            ready: Mutex::new(Vec::new()),
            stop: AtomicBool::new(false),
            quiet,
        });
        // Manual reset: it stays signaled until the thread has seen it, so a `set_dirs` that lands
        // while the thread is busy is not lost.
        let wake =
            Wake(unsafe { CreateEventW(None, true, false, None) }.map_or(0, |h| h.0 as isize));
        let thread = (wake.0 != 0).then(|| {
            let shared = shared.clone();
            std::thread::spawn(move || run(&shared, wake, &ctx))
        });
        Self {
            shared,
            wake,
            thread,
        }
    }

    /// Replace the set of watched folders.
    pub fn set_dirs(&self, dirs: Vec<PathBuf>) {
        *self.shared.dirs.lock().unwrap() = dirs;
        if self.wake.0 != 0 {
            unsafe {
                let _ = SetEvent(self.wake.handle());
            }
        }
    }

    /// Folders that changed since the last poll and have been quiet for a moment.
    pub fn poll(&self) -> Vec<PathBuf> {
        std::mem::take(&mut *self.shared.ready.lock().unwrap())
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            unsafe {
                let _ = SetEvent(self.wake.handle());
            }
            let _ = t.join();
        }
        if self.wake.0 != 0 {
            unsafe {
                let _ = CloseHandle(self.wake.handle());
            }
        }
    }
}

/// Start notifications for each folder, skipping those that cannot be watched (gone, no access).
fn open_all(dirs: &[PathBuf]) -> Vec<(PathBuf, HANDLE)> {
    let filter = FILE_NOTIFY_CHANGE_FILE_NAME
        | FILE_NOTIFY_CHANGE_DIR_NAME
        | FILE_NOTIFY_CHANGE_SIZE
        | FILE_NOTIFY_CHANGE_LAST_WRITE
        | FILE_NOTIFY_CHANGE_ATTRIBUTES;
    let mut out = Vec::new();
    for dir in dirs {
        if out.len() >= MAXIMUM_WAIT_OBJECTS - 1 {
            break;
        }
        let wide: Vec<u16> = dir.as_os_str().encode_wide_nul();
        if let Ok(h) = unsafe { FindFirstChangeNotificationW(PCWSTR(wide.as_ptr()), false, filter) }
        {
            out.push((dir.clone(), h));
        }
    }
    out
}

fn close_all(watches: &mut Vec<(PathBuf, HANDLE)>) {
    for (_, h) in watches.drain(..) {
        unsafe {
            let _ = FindCloseChangeNotification(h);
        }
    }
}

trait EncodeWideNul {
    fn encode_wide_nul(&self) -> Vec<u16>;
}

impl EncodeWideNul for std::ffi::OsStr {
    fn encode_wide_nul(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(Some(0)).collect()
    }
}

fn run(shared: &Shared, wake: Wake, ctx: &egui::Context) {
    let mut watches: Vec<(PathBuf, HANDLE)> = Vec::new();
    // Folder -> when it last changed.
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();
    let mut rebuild = true;
    while !shared.stop.load(Ordering::SeqCst) {
        if rebuild {
            rebuild = false;
            // Reset before reading the set, so a `set_dirs` after the read signals again.
            unsafe {
                let _ = ResetEvent(wake.handle());
            }
            close_all(&mut watches);
            let dirs = shared.dirs.lock().unwrap().clone();
            watches = open_all(&dirs);
            pending.retain(|d, _| watches.iter().any(|(w, _)| w == d));
        }
        let timeout = pending
            .values()
            .map(|t| (*t + shared.quiet).saturating_duration_since(Instant::now()))
            .min()
            .map_or(INFINITE, |d| d.as_millis() as u32 + 1);
        let mut handles = vec![wake.handle()];
        handles.extend(watches.iter().map(|(_, h)| *h));
        let r = unsafe { WaitForMultipleObjects(&handles, false, timeout) };
        if r == WAIT_TIMEOUT {
            // Fall through to the flush below.
        } else if r == WAIT_OBJECT_0 {
            rebuild = true;
        } else if r.0 > WAIT_OBJECT_0.0 && ((r.0 - WAIT_OBJECT_0.0) as usize) < handles.len() {
            let i = (r.0 - WAIT_OBJECT_0.0) as usize - 1;
            if unsafe { FindNextChangeNotification(watches[i].1) }.is_ok() {
                pending.insert(watches[i].0.clone(), Instant::now());
            } else {
                // The folder went away; drop it rather than spin on a dead handle.
                let (dir, h) = watches.remove(i);
                unsafe {
                    let _ = FindCloseChangeNotification(h);
                }
                pending.insert(dir, Instant::now());
            }
        } else {
            // WAIT_FAILED or abandoned: start over, without spinning.
            std::thread::sleep(QUIET);
            rebuild = true;
        }
        let now = Instant::now();
        let due: Vec<PathBuf> = pending
            .iter()
            .filter(|(_, t)| now.duration_since(**t) >= shared.quiet)
            .map(|(d, _)| d.clone())
            .collect();
        if !due.is_empty() {
            for d in &due {
                pending.remove(d);
            }
            shared.ready.lock().unwrap().extend(due);
            // The root by name: from this thread a plain `request_repaint` reaches whichever
            // viewport was last current, and an app with several (the bars) polls in its root.
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    }
    close_all(&mut watches);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for(w: &Watcher, dir: &PathBuf, secs: u64) -> bool {
        let end = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < end {
            if w.poll().contains(dir) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn reports_a_created_file_and_stops_on_drop() {
        let dir = std::env::temp_dir().join(format!("shellman-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let w = Watcher::new(egui::Context::default());
        w.set_dirs(vec![dir.clone(), dir.join("missing")]);
        std::thread::sleep(Duration::from_millis(300));
        std::fs::write(dir.join("a.txt"), b"hi").unwrap();
        assert!(wait_for(&w, &dir, 2), "change not reported");
        // Quiet now: nothing more to report.
        assert!(!wait_for(&w, &dir, 1));
        drop(w);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
