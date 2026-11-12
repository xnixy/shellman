//! Which app a window belongs to: its AppUserModelID (what Windows itself groups taskbar
//! buttons by), then the package's ID for Store apps, then the path of the program that owns it.
//! The IDs stay the same across app updates; a path may not, since it can hold a version.

use std::path::PathBuf;
use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::WindowsAndMessaging::{EnumChildWindows, GetWindowThreadProcessId};

#[derive(Clone, Debug)]
pub(super) struct Identity {
    pub app_id: String,
    pub exe: Option<PathBuf>,
}

pub(super) fn of(hwnd: HWND) -> Identity {
    let pid = owner_pid(hwnd);
    let exe = exe_of(pid);
    let app_id = aumid(hwnd)
        .or_else(|| package_aumid(pid))
        .or_else(|| exe.as_ref().map(|p| p.to_string_lossy().to_lowercase()))
        .unwrap_or_default();
    Identity { app_id, exe }
}

/// The AppUserModelID the window or its process declared, if any.
fn aumid(hwnd: HWND) -> Option<String> {
    unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd).ok()?;
        let value = store.GetValue(&PKEY_AppUserModel_ID).ok()?;
        let text: PWSTR = PropVariantToStringAlloc(&value).ok()?;
        let s = text.to_string().ok();
        CoTaskMemFree(Some(text.0 as *const _));
        s.filter(|s| !s.is_empty())
    }
}

/// The process that really owns the window. Store apps run inside a frame window owned by
/// ApplicationFrameHost; their own process owns the frame's CoreWindow child.
fn owner_pid(hwnd: HWND) -> u32 {
    let owner = if super::class_name(hwnd) == "ApplicationFrameWindow" {
        core_window(hwnd).unwrap_or(hwnd)
    } else {
        hwnd
    };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(owner, Some(&mut pid)) };
    pid
}

/// A packaged (Store or MSIX) app's ID, from its process, for windows that don't declare one.
fn package_aumid(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        let status = GetApplicationUserModelId(process, &mut len, Some(PWSTR(buf.as_mut_ptr())));
        let _ = CloseHandle(process);
        // Unpackaged processes answer APPMODEL_ERROR_NO_APPLICATION.
        if status.is_err() || len == 0 {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(len as usize);
        Some(String::from_utf16_lossy(&buf[..end])).filter(|s| !s.is_empty())
    }
}

fn core_window(frame: HWND) -> Option<HWND> {
    let mut found: Option<HWND> = None;
    unsafe {
        let _ = EnumChildWindows(Some(frame), Some(find_core), LPARAM((&raw mut found) as isize));
    }
    found
}

unsafe extern "system" fn find_core(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if super::class_name(hwnd) == "Windows.UI.Core.CoreWindow" {
        *(lparam.0 as *mut Option<HWND>) = Some(hwnd);
        return BOOL(0);
    }
    BOOL(1)
}

fn exe_of(pid: u32) -> Option<PathBuf> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
            .is_ok();
        let _ = CloseHandle(process);
        ok.then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
    }
}
