//! Hearing what apps tell the taskbar about their windows: overlay badges and progress
//! (`ITaskbarList3`) and calls for attention (`FlashWindowEx`). See `core::taskbar_list` for how
//! a shell that is not Explorer receives them.
//!
//! - Badges and progress arrive at the shell's task window, which is the tray window when the bar
//!   is the shell; `tray::host` hands them to [`on_message`]. Apps send them only once told their
//!   button exists, so the dock sends "TaskbarButtonCreated" to each new app window while the bar
//!   is the task window ([`announce`]).
//! - Flashing arrives as `HSHELL_FLASH` at a shell hook window: one hidden window of the dock's
//!   own, on a thread of its own, registered with `RegisterShellHookWindow` (which any process
//!   may do, so a test bar hears it too). It also takes the taskbar messages, so a test can send
//!   them to it.
//!
//! Each change is recorded here and wakes the docks; nothing polls.

use super::status::Window;
use shellman_core::taskbar_list::{self as tl, Message, Progress};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::Instant;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetWindowThreadProcessId, PostMessageW,
    RegisterClassExW, RegisterShellHookWindow, RegisterWindowMessageW, TranslateMessage, MSG, WNDCLASSEXW,
    WS_EX_TOOLWINDOW, WS_POPUP,
};

#[derive(Default)]
struct State {
    windows: HashMap<isize, Window>,
    /// Counts badge pictures, so a dock knows when to upload a new one.
    rev: u64,
    /// App windows already told their button exists.
    announced: HashSet<isize>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Mutex::default);
static SHELLHOOK: OnceLock<u32> = OnceLock::new();

/// Start listening (once per process).
pub fn start() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        std::thread::spawn(run);
    });
}

/// What the windows have said, for the docks' frames.
pub fn snapshot() -> HashMap<isize, Window> {
    STATE.lock().unwrap().windows.clone()
}

/// Forget windows that are gone, and say a window is in front (its call for attention is
/// answered).
pub fn settle(alive: &[isize], front: Option<isize>) {
    let mut s = STATE.lock().unwrap();
    s.windows.retain(|h, _| alive.contains(h));
    s.announced.retain(|h| alive.contains(h));
    if let Some(w) = front.and_then(|f| s.windows.get_mut(&f)) {
        w.attention = None;
    }
}

/// Called on the tray's or the hook window's thread: through the host (see `wake_docks_from_thread`).
fn wake() {
    super::wake_docks_from_thread();
}

/// A message sent to the shell's task window: handled (and its result) if it is about a button.
pub fn on_message(msg: u32, wparam: usize, lparam: isize) -> Option<LRESULT> {
    let m = tl::decode(msg, wparam, lparam)?;
    // An overlay's icon is valid only while the call lasts: copy its pixels now.
    let picture = match m {
        Message::Overlay { icon, .. } if icon != 0 => crate::tray::icon::to_image(icon).map(Arc::new),
        _ => None,
    };
    {
        let mut s = STATE.lock().unwrap();
        s.rev += 1;
        let rev = s.rev;
        match m {
            Message::Value { hwnd, done } => {
                let w = s.windows.entry(hwnd).or_default();
                w.done = done;
                // A value without a state means normal progress, as Explorer has it.
                if w.progress == Progress::None {
                    w.progress = Progress::Normal;
                    w.progress_since = Some(Instant::now());
                }
            }
            Message::State { hwnd, state } => {
                let w = s.windows.entry(hwnd).or_default();
                if w.progress == Progress::None && state != Progress::None {
                    w.progress_since = Some(Instant::now());
                }
                w.progress = state;
                if state == Progress::None {
                    w.done = 0.0;
                    w.progress_since = None;
                }
            }
            Message::Overlay { hwnd, .. } => {
                s.windows.entry(hwnd).or_default().badge = picture.map(|p| (rev, p));
            }
        }
    }
    wake();
    Some(LRESULT(0))
}

/// A shell hook notification: a window flashes, or came to the front.
fn on_shell_hook(code: usize, hwnd: isize) {
    let mut s = STATE.lock().unwrap();
    match code {
        tl::HSHELL_FLASH => {
            let w = s.windows.entry(hwnd).or_default();
            // Repeated flashes keep the first start, so the pulses play once.
            if w.attention.is_none() {
                w.attention = Some(Instant::now());
            } else {
                return;
            }
        }
        tl::HSHELL_WINDOWACTIVATED | tl::HSHELL_RUDEAPPACTIVATED => match s.windows.get_mut(&hwnd) {
            Some(w) if w.attention.is_some() => w.attention = None,
            _ => return,
        },
        tl::HSHELL_WINDOWDESTROYED => {
            if s.windows.remove(&hwnd).is_none() {
                return;
            }
        }
        _ => return,
    }
    drop(s);
    wake();
}

/// Tell new app windows their button exists, so they start sending badges and progress; only
/// while this bar is the shell's task window (else they would talk to another shell's).
pub fn announce(windows: &[isize]) {
    if !is_task_window_ours() {
        return;
    }
    let created = unsafe { RegisterWindowMessageW(w!("TaskbarButtonCreated")) };
    let mut s = STATE.lock().unwrap();
    for &w in windows {
        if s.announced.insert(w) {
            unsafe {
                let _ = PostMessageW(Some(HWND(w as *mut _)), created, WPARAM(0), LPARAM(0));
            }
        }
    }
}

/// Whether the shell's task window (`GetTaskmanWindow`) belongs to this process.
fn is_task_window_ours() -> bool {
    use windows::core::s;
    use windows::Win32::System::LibraryLoader::GetProcAddress;
    unsafe {
        let Ok(user32) = GetModuleHandleW(w!("user32.dll")) else { return false };
        let Some(f) = GetProcAddress(user32, s!("GetTaskmanWindow")) else { return false };
        let get: unsafe extern "system" fn() -> HWND = std::mem::transmute(f);
        let hwnd = get();
        if hwnd.is_invalid() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid == std::process::id()
    }
}

/// The hook window's thread: a hidden tool window and its message loop.
fn run() {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: instance.into(),
            lpszClassName: w!("ShellmanDockTasks"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("ShellmanDockTasks"),
            w!("shellman dock tasks"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        ) else {
            return;
        };
        let _ = SHELLHOOK.set(RegisterWindowMessageW(w!("SHELLHOOK")));
        let _ = RegisterShellHookWindow(hwnd);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if SHELLHOOK.get().is_some_and(|&m| m != 0 && m == msg) {
        on_shell_hook(wparam.0, lparam.0);
        return LRESULT(0);
    }
    if let Some(r) = on_message(msg, wparam.0, lparam.0) {
        return r;
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}
