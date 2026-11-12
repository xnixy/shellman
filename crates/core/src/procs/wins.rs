//! Which processes have windows, for the Apps group and the window titles. Its own enumeration,
//! with the taskbar's rule (shown, real top level, titled), but counting windows on other virtual
//! desktops too: those are still apps.

use super::Window;
use windows::core::{w, BOOL};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, GetClassNameW, GetShellWindow, GetWindow, GetWindowLongPtrW, GetWindowTextW,
    GetWindowThreadProcessId, IsHungAppWindow, IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WS_EX_APPWINDOW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

/// Cloaked by the shell: on another virtual desktop. Other cloaks (a suspended Store app's frame,
/// an app hiding its window) are not shown windows.
const CLOAKED_BY_SHELL: u32 = 2;

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let list = &mut *(lparam.0 as *mut Vec<HWND>);
    list.push(hwnd);
    BOOL(1)
}

fn class(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n.min(buf.len())])
}

fn title(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n.min(buf.len())])
}

fn pid_of(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

fn is_app_window(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || hwnd == GetShellWindow() {
            return false;
        }
        let mut cloaked = 0u32;
        let read = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&raw mut cloaked).cast(), 4).is_ok();
        if read && cloaked != 0 && cloaked != CLOAKED_BY_SHELL {
            return false;
        }
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if ex & WS_EX_APPWINDOW.0 == 0 {
            let owned = GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid());
            if owned || ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) != 0 {
                return false;
            }
        }
    }
    !matches!(class(hwnd).as_str(), "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "Progman" | "WorkerW")
}

/// Every app window now, with the PID it belongs to. A Store app's frame belongs to
/// ApplicationFrameHost; it is counted for the app inside it when that is there.
pub fn app_windows() -> Vec<(u32, Window)> {
    let mut all: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&raw mut all as isize));
    }
    all.into_iter()
        .filter(|&h| is_app_window(h))
        .filter_map(|hwnd| {
            let title = title(hwnd);
            if title.is_empty() {
                return None;
            }
            let mut pid = pid_of(hwnd);
            if class(hwnd) == "ApplicationFrameWindow" {
                let inner = unsafe { FindWindowExW(Some(hwnd), None, w!("Windows.UI.Core.CoreWindow"), None) };
                if let Ok(inner) = inner {
                    let p = pid_of(inner);
                    if p != 0 {
                        pid = p;
                    }
                }
            }
            let hung = unsafe { IsHungAppWindow(hwnd) }.as_bool();
            Some((pid, Window { hwnd: hwnd.0 as isize, title, hung }))
        })
        .collect()
}
