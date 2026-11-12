//! The app windows a dock shows: which top-level windows count as apps, which app each belongs
//! to, and a watcher that says when that may have changed.
//!
//! The rules match what Explorer's taskbar shows, so the same windows appear in the same places.

mod identity;
mod watch;

pub use watch::WindowWatcher;

use identity::Identity;
use std::collections::HashMap;
use std::path::PathBuf;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetAncestor, GetClassNameW, GetForegroundWindow, GetShellWindow, GetWindow,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, IsIconic, IsWindowVisible, GA_ROOTOWNER,
    GWL_EXSTYLE, GW_OWNER, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

#[derive(Clone, Debug, PartialEq)]
pub struct AppWindow {
    pub hwnd: isize,
    pub title: String,
    /// What groups windows under one dock icon: the window's AppUserModelID when it has one,
    /// otherwise its program's path, lowercased.
    pub app_id: String,
    pub exe: Option<PathBuf>,
    pub minimized: bool,
}

/// The app windows, with each window's identity remembered so a refresh only asks Windows about
/// windows it has not seen before.
#[derive(Default)]
pub struct WindowList {
    known: HashMap<isize, Identity>,
}

impl WindowList {
    /// Every app window now, front to back.
    pub fn refresh(&mut self) -> Vec<AppWindow> {
        let mut hwnds: Vec<HWND> = Vec::with_capacity(128);
        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM((&raw mut hwnds) as isize));
        }
        let mut out = Vec::with_capacity(hwnds.len());
        for hwnd in hwnds {
            if !is_app_window(hwnd) {
                continue;
            }
            let id = self
                .known
                .entry(hwnd.0 as isize)
                .or_insert_with(|| identity::of(hwnd));
            out.push(AppWindow {
                hwnd: hwnd.0 as isize,
                title: title(hwnd),
                app_id: id.app_id.clone(),
                exe: id.exe.clone(),
                minimized: unsafe { IsIconic(hwnd).as_bool() },
            });
        }
        // Forget windows that are gone, so handles reused later get a fresh identity.
        self.known
            .retain(|h, _| out.iter().any(|w| w.hwnd == *h));
        out
    }
}

/// The app window in front: the root owner of the foreground window, so a dialog in front
/// still counts as its app's window.
pub fn foreground() -> Option<isize> {
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_invalid() {
        return None;
    }
    let root = unsafe { GetAncestor(fg, GA_ROOTOWNER) };
    Some(if root.is_invalid() { fg } else { root }.0 as isize)
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let list = &mut *(lparam.0 as *mut Vec<HWND>);
    list.push(hwnd);
    BOOL(1)
}

/// Explorer's rule for taskbar buttons: shown, on this virtual desktop, a real top-level window
/// (no owner, not a tool window) unless it explicitly asks to be shown, and titled.
fn is_app_window(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || hwnd == GetShellWindow() || is_cloaked(hwnd) {
            return false;
        }
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let asks_to_show = ex & WS_EX_APPWINDOW.0 != 0;
        if !asks_to_show {
            let owned = GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid());
            let tool = ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) != 0;
            if owned || tool {
                return false;
            }
        }
        if GetWindowTextLengthW(hwnd) == 0 {
            return false;
        }
        !matches!(
            class_name(hwnd).as_str(),
            "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "Progman" | "WorkerW"
        )
    }
}

/// Windows on other virtual desktops, and suspended Store apps, stay "visible" but are cloaked.
fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked: u32 = 0;
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&raw mut cloaked).cast(),
            size_of::<u32>() as u32,
        )
    }
    .is_ok()
        && cloaked != 0
}

/// A window's title, read now.
pub fn title_of(hwnd: isize) -> String {
    title(HWND(hwnd as *mut _))
}

fn title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..n])
}

pub(crate) fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..n])
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };

    /// A real top-level window, shown, owned by this test process.
    pub(crate) fn test_window(title: windows::core::PCWSTR, ex: WINDOW_EX_STYLE) -> HWND {
        unsafe {
            CreateWindowExW(
                ex,
                w!("STATIC"),
                title,
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                -2000,
                -2000,
                200,
                100,
                None,
                None,
                None,
                None,
            )
            .expect("create test window")
        }
    }

    #[test]
    fn lists_app_windows_and_skips_tool_windows() {
        let app = test_window(w!("shellman winlist app"), WINDOW_EX_STYLE(0));
        let tool = test_window(w!("shellman winlist tool"), WS_EX_TOOLWINDOW);
        let windows = WindowList::default().refresh();
        let mine = windows.iter().find(|w| w.hwnd == app.0 as isize).expect("app window listed");
        assert_eq!(mine.title, "shellman winlist app");
        let exe = std::env::current_exe().unwrap();
        assert_eq!(mine.exe.as_deref(), Some(exe.as_path()));
        assert!(!windows.iter().any(|w| w.hwnd == tool.0 as isize), "tool window skipped");
        unsafe {
            let _ = DestroyWindow(app);
            let _ = DestroyWindow(tool);
        }
    }
}

#[cfg(test)]
mod live {
    /// Prints how the open windows group into dock icons. Run by hand:
    /// `cargo test -p shellman-core -- --ignored --nocapture winlist_groups`
    #[test]
    #[ignore]
    fn winlist_groups() {
        let windows = super::WindowList::default().refresh();
        let mut groups: Vec<(String, usize)> = Vec::new();
        for w in &windows {
            match groups.iter_mut().find(|(id, _)| *id == w.app_id) {
                Some(g) => g.1 += 1,
                None => groups.push((w.app_id.clone(), 1)),
            }
        }
        for (id, n) in groups {
            println!("{n} x {id}");
        }
    }
}
