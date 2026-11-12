//! Says when the window list may have changed, without polling: one thread receives Windows'
//! window events (shown, hidden, destroyed, renamed, minimized, cloaked, brought to front) and
//! raises a flag and wakes the UI.

use super::filter::{
    self, Verdict, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, EVENT_OBJECT_LOCATIONCHANGE,
    EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND,
    EVENT_SYSTEM_MINIMIZESTART,
};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetAncestor, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, PostThreadMessageW,
    TranslateMessage, GA_ROOT, MSG, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_APP, WM_QUIT,
};

// The event ranges listened to. Narrow on purpose: location changes fire constantly (every cursor
// and caret move), so they are not in this list. A separate hook hears them from the front
// window's thread only (see `follow_foreground`). Each event is then weighed in the callback
// (`filter`), so other apps' popups wake nothing.
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
type Hook = OnceLock<Mutex<Option<Box<dyn Fn() + Send>>>>;

/// Called when the window in front moves or resizes, for example into or out of fullscreen. Kept
/// apart from `WAKE` because these events are frequent and nothing in the window list changes.
static MOVED: Hook = OnceLock::new();
/// The location hook on the front window's thread, if one is installed. Only the watcher thread
/// touches it.
static MOVE_HOOK: AtomicIsize = AtomicIsize::new(0);
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

    /// Call `f` (from the watcher thread) whenever the window in front moves or resizes. It
    /// must be quick and must not block: it runs inside the event callback.
    /// Nothing is listened to for this until someone calls it.
    pub fn on_front_moved(&self, f: impl Fn() + Send + 'static) {
        *MOVED.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(Box::new(f));
        // The watcher thread hooks the window in front now; later it follows each change.
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_APP, WPARAM(0), LPARAM(0));
        }
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
        for hook in [WAKE.get(), MOVED.get()].into_iter().flatten() {
            *hook.lock().unwrap() = None;
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
            if msg.hwnd.is_invalid() && msg.message == WM_APP {
                follow_foreground(GetForegroundWindow());
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    for h in hooks {
        unsafe {
            let _ = UnhookWinEvent(h);
        }
    }
    unhook_moves();
}

/// Hear location changes from the thread of the window in front, and nobody else's: the hook is
/// replaced each time the foreground changes. A system-wide location hook would be woken for
/// every cursor and caret move. Does nothing until someone asked for `on_front_moved`.
unsafe fn follow_foreground(front: HWND) {
    let wanted = MOVED.get().is_some_and(|m| m.lock().unwrap().is_some());
    unhook_moves();
    if !wanted || front.is_invalid() {
        return;
    }
    let thread = GetWindowThreadProcessId(front, None);
    if thread != 0 {
        let hook = SetWinEventHook(
            EVENT_OBJECT_LOCATIONCHANGE,
            EVENT_OBJECT_LOCATIONCHANGE,
            None,
            Some(on_event),
            0,
            thread,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        MOVE_HOOK.store(hook.0 as isize, Ordering::Release);
        // A window that took its final place before the hook existed was missed: ask once more.
        notify_moved();
    }
}

fn notify_moved() {
    if let Some(m) = MOVED.get() {
        if let Some(moved) = m.lock().unwrap().as_ref() {
            moved();
        }
    }
}

fn unhook_moves() {
    let old = MOVE_HOOK.swap(0, Ordering::AcqRel);
    if old != 0 {
        unsafe {
            let _ = UnhookWinEvent(HWINEVENTHOOK(old as *mut _));
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
    if event == EVENT_SYSTEM_FOREGROUND {
        follow_foreground(hwnd);
    } else if GetAncestor(hwnd, GA_ROOT) != hwnd {
        return;
    }
    let front = || {
        let fg = GetForegroundWindow();
        !fg.is_invalid() && GetAncestor(fg, GA_ROOT) == hwnd
    };
    let in_front = matches!(event, EVENT_OBJECT_LOCATIONCHANGE | EVENT_OBJECT_NAMECHANGE) && front();
    let known = filter::is_known(hwnd.0 as isize);
    match filter::verdict(event, known, in_front, || super::is_app_window(hwnd)) {
        Verdict::Ignore => return,
        Verdict::FrontMoved => return notify_moved(),
        Verdict::FrontRenamed => FRONT_RENAMED.store(true, Ordering::Release),
        Verdict::List => DIRTY.store(true, Ordering::Release),
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
