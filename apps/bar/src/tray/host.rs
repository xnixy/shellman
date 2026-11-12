//! The tray window: a hidden window of class `Shell_TrayWnd`, the name every app looks for when
//! it shows a tray icon. It runs on its own thread with its own message loop, so apps that talk
//! to the tray never wait on the bar's drawing.

use super::data::{self, Notify, NIF_GUID, NIF_ICON, NIF_MESSAGE, NIF_STATE, NIF_TIP, NIS_HIDDEN};
use super::{TrayIcon, TrayState};
use std::sync::{Arc, Mutex, OnceLock};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetWindowThreadProcessId,
    RegisterClassExW, RegisterWindowMessageW, SendNotifyMessageW, TranslateMessage, HWND_BROADCAST,
    MSG, WM_COPYDATA, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_POPUP,
};

/// The window procedure has no context pointer, so the shared state and wake-up live here.
static STATE: OnceLock<Arc<Mutex<TrayState>>> = OnceLock::new();
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Create the tray window and run its message loop. Call on a thread of its own.
pub fn run(state: Arc<Mutex<TrayState>>, wake: Box<dyn Fn() + Send + Sync>) {
    let _ = STATE.set(state);
    let _ = WAKE.set(wake);
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: instance.into(),
            lpszClassName: w!("Shell_TrayWnd"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        let Ok(_hwnd) = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("Shell_TrayWnd"),
            w!(""),
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
        register_as_taskman(_hwnd);
        // Apps re-add their icons when the taskbar is (re)created; tell them it just was.
        let created = RegisterWindowMessageW(w!("TaskbarCreated"));
        let _ = SendNotifyMessageW(HWND_BROADCAST, created, WPARAM(0), LPARAM(0));
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Tell Windows this is the shell's task window, as Explorer does. Windows only hides minimized
/// windows (instead of drawing title-bar stubs on the desktop) when one is registered. The call
/// is exported by user32 but not declared in the SDK, so it is looked up by name.
unsafe fn register_as_taskman(hwnd: HWND) {
    use windows::core::s;
    use windows::Win32::System::LibraryLoader::GetProcAddress;
    let Ok(user32) = GetModuleHandleW(w!("user32.dll")) else { return };
    if let Some(f) = GetProcAddress(user32, s!("SetTaskmanWindow")) {
        let set: unsafe extern "system" fn(HWND) -> i32 = std::mem::transmute(f);
        set(hwnd);
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_COPYDATA {
        let cds = &*(lparam.0 as *const COPYDATASTRUCT);
        // 1 = tray icons. 0 (app bars) and 3 (icon positions) are answered "no": the bar manages
        // screen space itself, and icon rectangles are not tracked.
        if cds.lpData.is_null() || !matches!(cds.dwData, 1 | 3) {
            return LRESULT(0);
        }
        let bytes = std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize);
        if cds.dwData == 3 {
            return LRESULT(rect_of(bytes));
        }
        let Some(n) = data::parse(bytes) else {
            return LRESULT(0);
        };
        let ok = apply(n);
        if let Some(wake) = WAKE.get() {
            wake();
        }
        return LRESULT(ok as isize);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Run `f` on the icon `hwnd`/`uid`, if the bar hosts the tray and has it.
fn with_icon(hwnd: isize, uid: u32, f: impl FnOnce(&mut TrayIcon)) {
    let Some(state) = STATE.get() else { return };
    let mut state = state.lock().unwrap();
    if let Some(i) = state.icons.iter_mut().find(|i| i.hwnd == hwnd && i.uid == uid) {
        f(i);
    }
}

/// Remember where `device`'s top bar drew an icon (physical screen pixels), for
/// `Shell_NotifyIconGetRect`.
pub fn set_rect(hwnd: isize, uid: u32, device: &str, primary: bool, rect: [i32; 4]) {
    with_icon(hwnd, uid, |i| {
        i.record(device, primary, rect);
    });
}

/// The user hovered or clicked the icon on `device`: later `GetRect` answers come from there.
pub fn touch(hwnd: isize, uid: u32, device: &str) {
    with_icon(hwnd, uid, |i| i.touch(device));
}

/// `device`'s top bar no longer draws the tray.
pub fn forget_device(device: &str) {
    let Some(state) = STATE.get() else { return };
    for i in state.lock().unwrap().icons.iter_mut() {
        i.forget(device);
    }
}

/// Answer `Shell_NotifyIconGetRect`: an icon's position or size, or 0 when it is unknown.
fn rect_of(bytes: &[u8]) -> isize {
    let Some(req) = data::parse_rect_request(bytes) else { return 0 };
    let answer = STATE.get().and_then(|state| {
        let state = state.lock().unwrap();
        let by_guid = req.guid != [0; 16];
        state
            .icons
            .iter()
            .find(|i| {
                (i.hwnd == req.hwnd && i.uid == req.uid) || (by_guid && i.guid == Some(req.guid))
            })
            .and_then(|i| i.answer_rect())
    });
    let reply = answer.map_or(0, |r| data::rect_answer(req.message, r));
    super::log::line(&format!(
        "getrect message={} hwnd={:#x} uid={} guid={} -> rect={answer:?} reply={reply:#x}",
        req.message,
        req.hwnd,
        req.uid,
        req.guid != [0; 16]
    ));
    reply
}

/// Add, change or remove an icon.
fn apply(n: Notify) -> bool {
    let Some(state) = STATE.get() else { return false };
    let mut state = state.lock().unwrap();
    if super::log::enabled() {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(HWND(n.hwnd as *mut _), Some(&mut pid)) };
        super::log::line(&format!(
            "notify msg={} [{} pid={} uid={}] flags={:#x} callback={:#x} version={} guid={} tip={:?}",
            n.message,
            super::log::process_name(pid),
            pid,
            n.uid,
            n.flags,
            n.callback,
            n.version,
            n.guid.is_some(),
            n.tip
        ));
    }
    let key =|i: &TrayIcon| match (n.flags & NIF_GUID != 0, n.guid, i.guid) {
        (true, Some(g), Some(ig)) => g == ig,
        _ => i.hwnd == n.hwnd && i.uid == n.uid,
    };
    match n.message {
        data::NIM_ADD | data::NIM_MODIFY => {
            let pos = state.icons.iter().position(|i| key(i));
            if pos.is_none() && n.message == data::NIM_MODIFY {
                return false;
            }
            let idx = pos.unwrap_or_else(|| {
                let mut pid = 0u32;
                unsafe { GetWindowThreadProcessId(HWND(n.hwnd as *mut _), Some(&mut pid)) };
                state.icons.push(TrayIcon {
                    hwnd: n.hwnd,
                    uid: n.uid,
                    guid: if n.flags & NIF_GUID != 0 { n.guid } else { None },
                    pid,
                    ..Default::default()
                });
                state.icons.len() - 1
            });
            let icon = &mut state.icons[idx];
            if n.flags & NIF_MESSAGE != 0 {
                icon.callback = n.callback;
            }
            if n.flags & NIF_TIP != 0 {
                icon.tip = n.tip;
            }
            if n.flags & NIF_STATE != 0 && n.state_mask & NIS_HIDDEN != 0 {
                icon.hidden = n.state & NIS_HIDDEN != 0;
            }
            if n.flags & NIF_ICON != 0 {
                icon.image = super::icon::to_image(n.icon).map(Arc::new);
                icon.rev += 1;
            }
            true
        }
        data::NIM_DELETE => {
            let before = state.icons.len();
            state.icons.retain(|i| !key(i));
            state.icons.len() != before
        }
        data::NIM_SETVERSION => {
            match state.icons.iter_mut().find(|i| key(i)) {
                Some(i) => {
                    i.version = n.version;
                    true
                }
                None => false,
            }
        }
        _ => true,
    }
}
