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

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_COPYDATA {
        let cds = &*(lparam.0 as *const COPYDATASTRUCT);
        // 1 = tray icons. 0 (app bars) and 3 (icon positions) are answered "no": the bar manages
        // screen space itself, and icon rectangles are not tracked.
        if cds.dwData != 1 || cds.lpData.is_null() {
            return LRESULT(0);
        }
        let bytes = std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize);
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

/// Add, change or remove an icon.
fn apply(n: Notify) -> bool {
    let Some(state) = STATE.get() else { return false };
    let mut state = state.lock().unwrap();
    let key = |i: &TrayIcon| match (n.flags & NIF_GUID != 0, n.guid, i.guid) {
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
            if let Some(i) = state.icons.iter_mut().find(|i| key(i)) {
                i.version = n.version;
            }
            true
        }
        _ => true,
    }
}
