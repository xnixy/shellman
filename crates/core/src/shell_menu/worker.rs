//! The thread the shell's menu lives on. Menu handlers are apartment-bound COM objects, and some
//! are slow or hang, so each walk runs on a single-threaded apartment of its own, never on the
//! window's thread. The thread keeps the menu alive after the walk so the picked command runs on
//! the very object that listed it, then lingers while any window the command opened is up.

use super::model::Item;
use super::paint::Palette;
use super::slow::Plan;
use super::target::{self, Target};
use super::walk::Walk;
use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::VecDeque;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::core::{BOOL, PCSTR, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, POINT};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::{CreateEventW, GetCurrentThreadId, SetEvent};
use windows::Win32::UI::Shell::{CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumThreadWindows, IsWindowVisible, MsgWaitForMultipleObjectsEx,
    PeekMessageW, TranslateMessage, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, SW_SHOWNORMAL,
};

// CMINVOKECOMMANDINFOEX masks: the Unicode fields are filled, and ptInvoke is where the pick was.
const CMIC_MASK_UNICODE: u32 = 0x0000_4000;
const CMIC_MASK_PTINVOKE: u32 = 0x2000_0000;
/// After a command, the thread stays at least this long for windows the command opens late.
const LINGER: Duration = Duration::from_secs(5);

pub(super) enum Request {
    /// Walk the menu for a target. `gen` names the walk in every event about it.
    Load { gen: u64, target: Target, hwnd: isize, plan: Plan, palette: Palette },
    /// Fill a submenu that was left for when the person opens it.
    Expand { gen: u64, path: Vec<usize>, plan: Plan },
    /// Run a command of the walk `gen`, owned by `hwnd`, as picked at `point` (screen pixels).
    Invoke { gen: u64, id: u32, hwnd: isize, point: (i32, i32) },
}

pub(super) enum Event {
    /// The menu as far as it is read. `done` once nothing is left to fill.
    Tree { gen: u64, items: Vec<Item>, done: bool },
    /// A submenu is being filled now, which the loader times from the outside in case it hangs.
    Filling { gen: u64, key: String },
    /// A submenu was filled, and how long its handler took.
    Filled { gen: u64, key: String, took: Duration },
    /// The shell gave no menu for the target.
    Failed { gen: u64 },
}

/// A kernel event the window's thread sets when it sends a request, so the worker can sleep on
/// its message queue and its requests at once.
struct Wake(HANDLE);
unsafe impl Send for Wake {}
unsafe impl Sync for Wake {}
impl Drop for Wake {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// The window's end of one worker thread.
pub(super) struct Worker {
    tx: Sender<Request>,
    wake: Arc<Wake>,
    /// The walk the window wants now; the worker stops a walk that is no longer it.
    wanted: Arc<AtomicU64>,
}

impl Worker {
    pub(super) fn spawn(events: Sender<Event>, repaint: Arc<dyn Fn() + Send + Sync>) -> Option<Self> {
        let wake = Arc::new(Wake(unsafe { CreateEventW(None, false, false, PCWSTR::null()) }.ok()?));
        let (tx, rx) = channel();
        let wanted = Arc::new(AtomicU64::new(0));
        let (w, a) = (wake.clone(), wanted.clone());
        std::thread::Builder::new()
            .name("shell-menu".into())
            .spawn(move || run(rx, w, a, events, repaint))
            .ok()?;
        Some(Self { tx, wake, wanted })
    }

    /// False when the thread is gone.
    pub(super) fn send(&self, req: Request) -> bool {
        if let Request::Load { gen, .. } = &req {
            self.wanted.store(*gen, Ordering::SeqCst);
        }
        let ok = self.tx.send(req).is_ok();
        unsafe {
            let _ = SetEvent(self.wake.0);
        }
        ok
    }
}

/// The walk this thread holds: its number, the menu, and the tree as read so far.
struct Held {
    gen: u64,
    walk: Walk,
    items: Vec<Item>,
}

fn run(
    rx: Receiver<Request>,
    wake: Arc<Wake>,
    wanted: Arc<AtomicU64>,
    events: Sender<Event>,
    repaint: Arc<dyn Fn() + Send + Sync>,
) {
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let emit = |e: Event| {
        let ok = events.send(e).is_ok();
        repaint();
        ok
    };
    let mut held: Option<Held> = None;
    // Requests read early, while a walk was still filling submenus.
    let mut stash: VecDeque<Request> = VecDeque::new();
    'outer: while let Some(req) = stash.pop_front().or_else(|| next(&rx, &wake)) {
        match req {
            Request::Load { gen, target, hwnd, plan, palette } => {
                // A newer walk is already wanted: skip this one.
                if wanted.load(Ordering::SeqCst) != gen {
                    continue;
                }
                held = None;
                let walk = target::context_menu(HWND(hwnd as *mut _), &target)
                    .and_then(|cm| Walk::query(cm, target.extended, palette));
                let Some(mut walk) = walk else {
                    emit(Event::Failed { gen });
                    continue;
                };
                let items = walk.top(&plan);
                if !emit(Event::Tree { gen, items: items.clone(), done: walk.pending() == 0 }) {
                    break;
                }
                let mut h = Held { gen, walk, items };
                while h.walk.pending() > 0 {
                    pump();
                    // A pick on what is already listed runs now, before the rest is filled.
                    while let Ok(r) = rx.try_recv() {
                        stash.push_back(r);
                    }
                    let picked = stash.iter().any(|r| matches!(r, Request::Invoke { .. }));
                    if picked || wanted.load(Ordering::SeqCst) != gen {
                        break;
                    }
                    if let Some(key) = h.walk.next_key() {
                        emit(Event::Filling { gen, key });
                    }
                    if let Some((key, took)) = h.walk.fill_next(&mut h.items, &plan) {
                        emit(Event::Filled { gen, key, took });
                    }
                    let done = h.walk.pending() == 0;
                    if !emit(Event::Tree { gen, items: h.items.clone(), done }) {
                        break 'outer;
                    }
                }
                held = Some(h);
            }
            Request::Expand { gen, path, plan } => {
                let Some(h) = held.as_mut().filter(|h| h.gen == gen) else {
                    continue;
                };
                let key = super::model::at(&h.items, &path).map(Item::slow_key).unwrap_or_default();
                emit(Event::Filling { gen, key });
                if let Some((key, took)) = h.walk.fill_deferred(&path, &mut h.items, &plan) {
                    emit(Event::Filled { gen, key, took });
                }
                // Anything found inside it is filled too.
                while h.walk.pending() > 0 {
                    h.walk.fill_next(&mut h.items, &plan);
                }
                emit(Event::Tree { gen, items: h.items.clone(), done: true });
            }
            Request::Invoke { gen, id, hwnd, point } => {
                let Some(h) = held.take().filter(|h| h.gen == gen) else {
                    continue;
                };
                invoke(&h, id, hwnd, point);
                // This thread's work is done; the window starts a new one for the next menu.
                // Stay while what the command opened needs this thread, then let go of it.
                linger();
                drop(h);
                break;
            }
        }
    }
    drop(held);
    if com {
        unsafe { CoUninitialize() };
    }
}

/// Waits for the next request, running this thread's messages meanwhile (handlers and the
/// windows they open live on it). None when the window side is gone.
fn next(rx: &Receiver<Request>, wake: &Wake) -> Option<Request> {
    loop {
        match rx.try_recv() {
            Ok(r) => return Some(r),
            Err(TryRecvError::Disconnected) => return None,
            Err(TryRecvError::Empty) => {}
        }
        unsafe {
            MsgWaitForMultipleObjectsEx(Some(&[wake.0]), u32::MAX, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
        pump();
    }
}

/// Runs the messages waiting on this thread.
fn pump() {
    let mut msg = MSG::default();
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn invoke(h: &Held, id: u32, hwnd: isize, point: (i32, i32)) {
    let info = CMINVOKECOMMANDINFOEX {
        cbSize: size_of::<CMINVOKECOMMANDINFOEX>() as u32,
        fMask: CMIC_MASK_UNICODE | CMIC_MASK_PTINVOKE,
        hwnd: HWND(hwnd as *mut _),
        // MAKEINTRESOURCE: the command's offset, passed in place of a verb string.
        lpVerb: PCSTR(id as usize as *const u8),
        lpVerbW: PCWSTR(id as usize as *const u16),
        nShow: SW_SHOWNORMAL.0,
        ptInvoke: POINT { x: point.0, y: point.1 },
        ..Default::default()
    };
    unsafe {
        let _ = h.walk.cm.InvokeCommand((&raw const info).cast::<CMINVOKECOMMANDINFO>());
    }
}

/// Keeps running this thread's messages while it has a visible window (a dialog or sheet the
/// command opened here), and for a few seconds in any case.
fn linger() {
    let start = Instant::now();
    let mut seen = super::raise::visible();
    loop {
        unsafe {
            MsgWaitForMultipleObjectsEx(None, 250, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
        pump();
        if start.elapsed() < Duration::from_secs(4) {
            super::raise::raise_new(&mut seen);
        }
        if start.elapsed() > LINGER && !has_visible_window() {
            return;
        }
    }
}

fn has_visible_window() -> bool {
    unsafe extern "system" fn visit(hwnd: HWND, found: LPARAM) -> BOOL {
        if unsafe { IsWindowVisible(hwnd) }.as_bool() {
            unsafe { *(found.0 as *mut bool) = true };
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut found = false;
    unsafe {
        let _ = EnumThreadWindows(GetCurrentThreadId(), Some(visit), LPARAM(&raw mut found as isize));
    }
    found
}
