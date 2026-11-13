//! Closing a panel that never takes the keyboard: a click outside it, or Esc.
//!
//! Like `outside` (whose single watch the popup menu uses), but a watch of its own per panel, so
//! a panel and a menu of the same process can both be open: each [`Watch`] runs its own thread
//! with a low-level mouse hook and a low-level keyboard hook, which Windows calls on that thread,
//! so their state lives in that thread's own slot. The hooks only compare a point with a few
//! numbers or look at one key; they never wait, and they pass every event on (Esc too: an app in
//! front that wants it still gets it). They go when the watch is dropped.

use crate::outside::{outside_all, Rect};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, PeekMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, KBDLLHOOKSTRUCT,
    LLKHF_UP, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_NCLBUTTONDOWN,
    WM_QUIT, WM_RBUTTONDOWN, WM_XBUTTONDOWN,
};

/// What a watch shares with its hook thread.
struct Shared {
    inside: Mutex<Vec<Rect>>,
    fired: AtomicBool,
    wake: Box<dyn Fn() + Send + Sync>,
}

thread_local! {
    /// The watch whose hooks run on this thread.
    static MINE: RefCell<Option<Arc<Shared>>> = const { RefCell::new(None) };
}

/// A running watch; dropping it removes the hooks.
pub struct Watch {
    shared: Arc<Shared>,
    thread: Arc<AtomicU32>,
}

impl Watch {
    /// Start watching. `wake` runs on the hook thread after a click outside or Esc.
    pub fn start(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let shared = Arc::new(Shared { inside: Mutex::new(Vec::new()), fired: AtomicBool::new(false), wake: Box::new(wake) });
        let thread = Arc::new(AtomicU32::new(0));
        let (s, t) = (shared.clone(), thread.clone());
        let _ = std::thread::Builder::new().name("dismiss".into()).spawn(move || unsafe {
            let mut msg = MSG::default();
            // The queue first, so a stop's WM_QUIT always has somewhere to go.
            let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
            if t.compare_exchange(0, GetCurrentThreadId(), Ordering::AcqRel, Ordering::Acquire).is_err() {
                return; // stopped before it started
            }
            MINE.with(|m| *m.borrow_mut() = Some(s));
            let module = GetModuleHandleW(None).ok().map(|m| m.into());
            let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(on_mouse), module, 0).ok();
            let keys = SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), module, 0).ok();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
            for hook in [mouse, keys].into_iter().flatten() {
                let _ = UnhookWindowsHookEx(hook);
            }
            MINE.with(|m| *m.borrow_mut() = None);
        });
        Self { shared, thread }
    }

    /// Where the panel is now (and what else counts as inside, such as the button that opened
    /// it), in physical screen pixels.
    pub fn set_rects(&self, rects: &[Rect]) {
        if let Ok(mut inside) = self.shared.inside.lock() {
            inside.clear();
            inside.extend_from_slice(rects);
        }
    }

    /// Whether a click outside or Esc came since the last call.
    pub fn take(&self) -> bool {
        self.shared.fired.swap(false, Ordering::AcqRel)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // u32::MAX tells a thread that has not started yet not to.
        let thread = self.thread.swap(u32::MAX, Ordering::AcqRel);
        if thread != 0 && thread != u32::MAX {
            unsafe {
                let _ = PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
    }
}

fn fire() {
    MINE.with(|m| {
        if let Some(s) = m.borrow().as_ref() {
            if !s.fired.swap(true, Ordering::AcqRel) {
                (s.wake)();
            }
        }
    });
}

unsafe extern "system" fn on_mouse(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let down = matches!(wparam.0 as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN | WM_NCLBUTTONDOWN);
    if code >= 0 && down {
        let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let out = MINE.with(|m| {
            m.borrow().as_ref().is_some_and(|s| s.inside.try_lock().map(|r| outside_all(info.pt.x, info.pt.y, &r)).unwrap_or(false))
        });
        if out {
            fire();
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if info.vkCode == VK_ESCAPE.0 as u32 && info.flags.0 & LLKHF_UP.0 == 0 {
            fire();
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}
