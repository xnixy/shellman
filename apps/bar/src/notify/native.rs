//! Windows' own notification banner, kept out of sight while shellman shows the toasts.
//!
//! Without Explorer nothing draws Windows' banners, but the modern-apps host (ShellAppRuntime)
//! starts ShellExperienceHost, and that one pops its banner up beside shellman's toast: every
//! notification showed twice. Its banner is one long-lived CoreWindow in the notification band
//! (`ZBID_IMMERSIVE_NOTIFICATION`; `EnumWindows` does not list it, `FindWindowEx` does), zero
//! pixels tall at rest and grown, and shown again, for each toast. A WinEvent hook on that
//! process hides it the moment it grows. The toast itself still goes to Windows' store, which is
//! where shellman reads it from, so nothing is lost.
//!
//! Only while the store can be read: when Windows' "let apps read notifications" switch is off
//! shellman sees no toasts, and Windows' banner is left alone ([`release`]). The window is looked
//! for when a read of the store succeeds ([`ensure`]), never polled; a ShellExperienceHost that
//! restarted is found again at the next read.

use std::cell::Cell;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::core::{s, w, BOOL};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Threading::{
    GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, FindWindowExW, GetMessageW, GetWindowRect, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
    PeekMessageW, PostThreadMessageW, ShowWindow, TranslateMessage, EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_SHOW, MSG,
    OBJID_WINDOW, PM_NOREMOVE, SW_HIDE, WINEVENT_OUTOFCONTEXT, WM_APP,
};

/// `ZBID_IMMERSIVE_NOTIFICATION`: the band Windows' banner lives in.
const NOTIFICATION_BAND: u32 = 4;
const ENSURE: u32 = WM_APP + 1;
const RELEASE: u32 = WM_APP + 2;

/// The z-order band of a window: `GetWindowBand`, undocumented, exported by name from user32
/// since Windows 8 (not in the import library, so looked up). None when it is missing.
fn window_band(hwnd: HWND) -> Option<u32> {
    type GetWindowBand = unsafe extern "system" fn(HWND, *mut u32) -> BOOL;
    static FN: std::sync::OnceLock<Option<GetWindowBand>> = std::sync::OnceLock::new();
    let f = (*FN.get_or_init(|| unsafe {
        let user32 = GetModuleHandleW(w!("user32.dll")).ok()?;
        GetProcAddress(user32, s!("GetWindowBand")).map(|p| std::mem::transmute::<_, GetWindowBand>(p))
    }))?;
    let mut band = 0;
    unsafe { f(hwnd, &mut band) }.as_bool().then_some(band)
}

/// The thread that owns the hook; 0 until it runs.
static THREAD: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static BANNER: Cell<isize> = const { Cell::new(0) };
    static HOOK: Cell<isize> = const { Cell::new(0) };
}

/// Start the thread (once). It does nothing until [`ensure`].
pub fn start() {
    if THREAD.load(Ordering::Acquire) != 0 {
        return;
    }
    let _ = std::thread::Builder::new().name("native banner".into()).spawn(run);
}

/// The store was read: make sure Windows' banner is hooked (cheap when it is).
pub fn ensure() {
    post(ENSURE);
}

/// The store cannot be read: let Windows show its banners again.
pub fn release() {
    post(RELEASE);
}

fn post(msg: u32) {
    let tid = THREAD.load(Ordering::Acquire);
    if tid != 0 {
        unsafe {
            let _ = PostThreadMessageW(tid, msg, WPARAM(0), LPARAM(0));
        }
    }
}

fn run() {
    unsafe {
        let mut msg = MSG::default();
        // Make the queue before anyone can post to it.
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
        THREAD.store(GetCurrentThreadId(), Ordering::Release);
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            match msg.message {
                ENSURE => hook(),
                RELEASE => unhook(),
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
}

fn hook() {
    let current = HWND(BANNER.get() as *mut _);
    if HOOK.get() != 0 && unsafe { IsWindow(Some(current)) }.as_bool() {
        return;
    }
    unhook();
    let Some(banner) = find() else { return };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(banner, Some(&mut pid)) };
    let h = unsafe {
        SetWinEventHook(EVENT_OBJECT_SHOW, EVENT_OBJECT_LOCATIONCHANGE, None, Some(on_event), pid, 0, WINEVENT_OUTOFCONTEXT)
    };
    if h.is_invalid() {
        return;
    }
    BANNER.set(banner.0 as isize);
    HOOK.set(h.0 as isize);
    hide_if_up(banner);
}

fn unhook() {
    let h = HOOK.replace(0);
    BANNER.set(0);
    if h != 0 {
        unsafe {
            let _ = UnhookWinEvent(HWINEVENTHOOK(h as *mut _));
        }
    }
}

/// ShellExperienceHost's CoreWindow in the notification band.
fn find() -> Option<HWND> {
    let mut after = None;
    loop {
        let h = unsafe { FindWindowExW(None, after, w!("Windows.UI.Core.CoreWindow"), None) }.ok()?;
        if window_band(h) == Some(NOTIFICATION_BAND) && is_experience_host(h) {
            return Some(h);
        }
        after = Some(h);
    }
}

fn is_experience_host(hwnd: HWND) -> bool {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else { return false };
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len) }.is_ok();
    unsafe {
        let _ = CloseHandle(process);
    }
    ok && String::from_utf16_lossy(&buf[..len as usize]).to_ascii_lowercase().ends_with("\\shellexperiencehost.exe")
}

/// Hidden when it is shown with some height (a banner); at rest it is zero pixels tall.
fn hide_if_up(hwnd: HWND) {
    let mut r = RECT::default();
    unsafe {
        if IsWindowVisible(hwnd).as_bool() && GetWindowRect(hwnd, &mut r).is_ok() && r.bottom > r.top {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

unsafe extern "system" fn on_event(_: HWINEVENTHOOK, _event: u32, hwnd: HWND, object: i32, _child: i32, _thread: u32, _time: u32) {
    if object == OBJID_WINDOW.0 && hwnd.0 as isize == BANNER.get() && !hwnd.is_invalid() {
        hide_if_up(hwnd);
    }
}
