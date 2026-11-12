//! Says when the window list may have changed, without polling: one thread receives Windows'
//! window events (shown, hidden, destroyed, renamed, minimized, cloaked, brought to front) and
//! raises a flag and wakes the UI.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetAncestor, GetForegroundWindow, GetMessageW, PostThreadMessageW, TranslateMessage, GA_ROOT, MSG,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_QUIT,
};

// The event ranges listened to. Narrow on purpose: location changes fire constantly and are
// not needed.
const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
const EVENT_SYSTEM_MINIMIZESTART: u32 = 0x0016;
const EVENT_SYSTEM_MINIMIZEEND: u32 = 0x0017;
const EVENT_OBJECT_DESTROY: u32 = 0x8001;
const EVENT_OBJECT_HIDE: u32 = 0x8003;
const EVENT_OBJECT_NAMECHANGE: u32 = 0x800C;
const EVENT_OBJECT_CLOAKED: u32 = 0x8017;
const EVENT_OBJECT_UNCLOAKED: u32 = 0x8018;
const RANGES: [(u32, u32); 5] = [
    (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
    (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
    (EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE),
    (EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE),
    (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
];

/// Hook callbacks get no context pointer, so the flag and wake-up live here. One watcher per
/// process is all a shell needs.
static DIRTY: AtomicBool = AtomicBool::new(true);
/// The window in front was renamed. Kept apart from `DIRTY` because titles change often (a
/// terminal or a progress title can change many times a second) and only the front one is shown,
/// so a rename never needs the whole list rebuilt.
static FRONT_RENAMED: AtomicBool = AtomicBool::new(false);
static WAKE: OnceLock<Mutex<Option<Box<dyn Fn() + Send>>>> = OnceLock::new();

pub struct WindowWatcher {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl WindowWatcher {
    /// Start watching. `wake` is called (from the watcher thread) after each relevant change,
    /// for example to request a repaint.
    pub fn start(wake: impl Fn() + Send + 'static) -> Self {
        *WAKE.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(Box::new(wake));
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || run(tx));
        let thread_id = rx.recv().unwrap_or(0);
        Self {
            thread_id,
            thread: Some(thread),
        }
    }

    /// True once after any change since the last call (and on the first call).
    pub fn take_dirty(&self) -> bool {
        DIRTY.swap(false, Ordering::AcqRel)
    }

    /// True once after the window in front changed its title.
    pub fn take_front_renamed(&self) -> bool {
        FRONT_RENAMED.swap(false, Ordering::AcqRel)
    }
}

impl Drop for WindowWatcher {
    fn drop(&mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        if let Some(w) = WAKE.get() {
            *w.lock().unwrap() = None;
        }
    }
}

/// Hooks set out of context deliver their events through this thread's message queue, so it
/// runs a message loop until told to quit.
fn run(ready: std::sync::mpsc::Sender<u32>) {
    let hooks: Vec<HWINEVENTHOOK> = RANGES
        .iter()
        .map(|&(min, max)| unsafe {
            SetWinEventHook(min, max, None, Some(on_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS)
        })
        .collect();
    let _ = ready.send(unsafe { GetCurrentThreadId() });
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    for h in hooks {
        unsafe {
            let _ = UnhookWinEvent(h);
        }
    }
}

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    // Only whole top-level windows matter; controls inside them fire the same events.
    const OBJID_WINDOW: i32 = 0;
    const CHILDID_SELF: i32 = 0;
    if hwnd.is_invalid() || id_object != OBJID_WINDOW || id_child != CHILDID_SELF {
        return;
    }
    if event != EVENT_SYSTEM_FOREGROUND && GetAncestor(hwnd, GA_ROOT) != hwnd {
        return;
    }
    if event == EVENT_OBJECT_NAMECHANGE {
        // Background windows' titles are not shown anywhere; ignore them without waking the UI.
        let front = GetForegroundWindow();
        if front.is_invalid() || GetAncestor(front, GA_ROOT) != hwnd {
            return;
        }
        FRONT_RENAMED.store(true, Ordering::Release);
    } else {
        DIRTY.store(true, Ordering::Release);
    }
    if let Some(w) = WAKE.get() {
        if let Some(wake) = w.lock().unwrap().as_ref() {
            wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Events from this test process are skipped by design (the bar ignores its own windows),
    /// so this checks the plumbing: the first call reports a change, a second does not, and
    /// stopping joins the hook thread.
    #[test]
    fn starts_reports_once_and_stops() {
        let watcher = WindowWatcher::start(|| {});
        assert_ne!(watcher.thread_id, 0, "hook thread started");
        assert!(watcher.take_dirty(), "first call asks for a full list");
        assert!(!watcher.take_dirty(), "then only after a change");
        drop(watcher);
    }
}
