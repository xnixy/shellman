//! Noticing a click outside the panel, even when the panel never got the keyboard focus.
//!
//! The panel is usually summoned from the top bar, which never takes the focus, and Windows may
//! refuse to hand the focus to the panel; then "focus lost" never comes and a click elsewhere
//! would leave the panel standing. So, only while the panel shows, a low-level mouse hook on a
//! thread of its own watches for a button going down outside the panel's rectangle. The hook
//! only compares a point with four numbers and never waits; it ends when the panel hides.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_NCLBUTTONDOWN,
    WM_QUIT, WM_RBUTTONDOWN, WM_XBUTTONDOWN,
};

/// The panel's rectangle in physical pixels, as the hook compares it.
static LEFT: AtomicI32 = AtomicI32::new(0);
static TOP: AtomicI32 = AtomicI32::new(0);
static RIGHT: AtomicI32 = AtomicI32::new(0);
static BOTTOM: AtomicI32 = AtomicI32::new(0);
/// Set by the hook; taken by the UI.
static CLICKED: AtomicBool = AtomicBool::new(false);
/// The hook thread's id while it runs, 0 when not.
static THREAD: AtomicU32 = AtomicU32::new(0);
/// What to call when a click lands outside: the UI's repaint.
static WAKE: OnceLock<Mutex<Option<WakeFn>>> = OnceLock::new();
type WakeFn = Box<dyn Fn() + Send>;

/// Where the panel is now; call whenever it moves or resizes.
pub fn set_rect(x: i32, y: i32, w: i32, h: i32) {
    LEFT.store(x, Ordering::Relaxed);
    TOP.store(y, Ordering::Relaxed);
    RIGHT.store(x + w, Ordering::Relaxed);
    BOTTOM.store(y + h, Ordering::Relaxed);
}

/// Start watching (no-op if already watching). `wake` runs on the hook thread after an outside
/// click.
pub fn start(wake: impl Fn() + Send + 'static) {
    if THREAD.compare_exchange(0, STARTING, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return;
    }
    if let Ok(mut slot) = WAKE.get_or_init(Default::default).lock() {
        *slot = Some(Box::new(wake));
    }
    CLICKED.store(false, Ordering::Release);
    let spawned = std::thread::Builder::new().name("outside-click".into()).spawn(|| unsafe {
        let me = GetCurrentThreadId();
        // Make the message queue first, so `stop`'s WM_QUIT always has somewhere to go.
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
        // A `stop` before this point cancelled the start.
        if THREAD.compare_exchange(STARTING, me, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return;
        }
        let module = GetModuleHandleW(None).ok().map(|m| m.into());
        if let Ok(hook) = SetWindowsHookExW(WH_MOUSE_LL, Some(on_mouse), module, 0) {
            // Low-level hooks are called through this thread's message loop.
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
            let _ = UnhookWindowsHookEx(hook);
        }
        let _ = THREAD.compare_exchange(me, 0, Ordering::AcqRel, Ordering::Acquire);
    });
    if spawned.is_err() {
        THREAD.store(0, Ordering::Release);
    }
}

/// Stands in for the thread id while the hook thread starts.
const STARTING: u32 = u32::MAX;

/// Stop watching; the hook is removed at once.
pub fn stop() {
    let thread = THREAD.swap(0, Ordering::AcqRel);
    if thread != 0 && thread != STARTING {
        unsafe {
            let _ = PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

/// Whether a click landed outside since the last call.
pub fn take_click() -> bool {
    CLICKED.swap(false, Ordering::AcqRel)
}

/// Whether the point is outside the rectangle `(left, top, right, bottom)`.
pub fn outside(x: i32, y: i32, rect: (i32, i32, i32, i32)) -> bool {
    let (l, t, r, b) = rect;
    x < l || x >= r || y < t || y >= b
}

unsafe extern "system" fn on_mouse(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let down = matches!(
        wparam.0 as u32,
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN | WM_NCLBUTTONDOWN
    );
    if code >= 0 && down {
        let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let rect = (
            LEFT.load(Ordering::Relaxed),
            TOP.load(Ordering::Relaxed),
            RIGHT.load(Ordering::Relaxed),
            BOTTOM.load(Ordering::Relaxed),
        );
        if outside(info.pt.x, info.pt.y, rect) && !CLICKED.swap(true, Ordering::AcqRel) {
            if let Some(Ok(slot)) = WAKE.get().map(|w| w.lock()) {
                if let Some(wake) = slot.as_ref() {
                    wake();
                }
            }
        }
    }
    // The click always goes on to whatever was clicked.
    CallNextHookEx(None, code, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_count_as_inside() {
        let r = (100, 50, 460, 550);
        assert!(!outside(100, 50, r));
        assert!(!outside(459, 549, r));
        assert!(outside(460, 100, r));
        assert!(outside(99, 100, r));
        assert!(outside(200, 49, r));
        assert!(outside(-5000, 10, r));
    }
}
