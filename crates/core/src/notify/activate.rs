//! Delivering a click on a toast to its app, the way Windows' own shell does.
//!
//! A desktop app that sends toasts registers a COM class (its "toast activator") and names it by
//! its AppUserModelID: in the Start menu shortcut's `System.AppUserModel.ToastActivatorCLSID`, or
//! in `HKCU\Software\Classes\AppUserModelId\<id>\CustomActivator`. The class is a local server
//! (`LocalServer32`), so any process may create it, which starts the app if it is not running,
//! and call `INotificationActivationCallback::Activate` with the toast's arguments and whatever
//! was typed into its text boxes. Tested between two unrelated unpackaged processes, text box
//! contents included. Protocol activations open their URI. Packaged apps are activated by the
//! platform with identity shellman lacks; for those (and apps with no activator) the caller
//! brings the app forward instead.

use super::source::{Invoke, Outcome};
use super::model::Activation;
use windows::core::{HSTRING, PCWSTR, GUID};
use windows::Win32::Storage::EnhancedStorage::{PKEY_AppUserModel_ID, PKEY_AppUserModel_ToastActivatorCLSID};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED};
use windows::Win32::UI::Notifications::{INotificationActivationCallback, NOTIFICATION_USER_INPUT_DATA};
use windows::Win32::UI::Shell::{
    IShellItem2, SHCreateItemFromParsingName, ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY, SW_SHOWNORMAL};

/// Hand `invoke` to `app_id`. Blocking (it may start the app); call it off the UI thread.
pub fn deliver(app_id: &str, invoke: &Invoke) -> Result<Outcome, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        // The user just clicked shellman: let whatever answers come to the front.
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    if invoke.kind == Activation::Protocol {
        return if open(&invoke.args) { Ok(Outcome::Opened) } else { Err(format!("cannot open {}", invoke.args)) };
    }
    let Some(clsid) = activator(app_id) else { return Ok(Outcome::NoHandler) };
    let callback: INotificationActivationCallback =
        unsafe { CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER) }.map_err(|e| format!("{:#010x} {}", e.code().0, e.message()))?;
    // Kept alive until the call returns: the pointers below point into these.
    let strings: Vec<(HSTRING, HSTRING)> =
        invoke.inputs.iter().map(|(k, v)| (HSTRING::from(k.as_str()), HSTRING::from(v.as_str()))).collect();
    let data: Vec<NOTIFICATION_USER_INPUT_DATA> = strings
        .iter()
        .map(|(k, v)| NOTIFICATION_USER_INPUT_DATA { Key: PCWSTR(k.as_ptr()), Value: PCWSTR(v.as_ptr()) })
        .collect();
    unsafe { callback.Activate(&HSTRING::from(app_id), &HSTRING::from(invoke.args.as_str()), &data) }
        .map_err(|e| format!("{:#010x} {}", e.code().0, e.message()))?;
    Ok(Outcome::Delivered)
}

/// The app's toast activator: its shortcut's, else the registry's.
pub fn activator(app_id: &str) -> Option<GUID> {
    if app_id.is_empty() || app_id.contains('!') {
        // A packaged app (Package_family!App): activated by the platform only.
        return None;
    }
    from_shortcut(app_id).or_else(|| from_registry(app_id))
}

/// From the app's Start menu shortcut (the one carrying its AppUserModelID). The Apps folder's
/// item for the app does not pass this property on, so the shortcuts are looked through.
fn from_shortcut(app_id: &str) -> Option<GUID> {
    let roots = [dirs::data_dir(), std::env::var_os("ProgramData").map(std::path::PathBuf::from)];
    let mut stack: Vec<std::path::PathBuf> = roots
        .into_iter()
        .flatten()
        .map(|d| d.join("Microsoft\\Windows\\Start Menu\\Programs"))
        .collect();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
                if let Some(clsid) = shortcut_activator(&path, app_id) {
                    return Some(clsid);
                }
            }
        }
    }
    None
}

/// The shortcut's toast activator, if the shortcut is `app_id`'s.
fn shortcut_activator(path: &std::path::Path, app_id: &str) -> Option<GUID> {
    unsafe {
        let item: IShellItem2 = SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None).ok()?;
        let text = item.GetString(&PKEY_AppUserModel_ID).ok()?;
        let id = text.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(text.0 as *const _));
        if !id.is_some_and(|s| s.eq_ignore_ascii_case(app_id)) {
            return None;
        }
        item.GetCLSID(&PKEY_AppUserModel_ToastActivatorCLSID).ok().filter(|g| *g != GUID::zeroed())
    }
}

fn from_registry(app_id: &str) -> Option<GUID> {
    use crate::registry::{Hive, Real, Registry};
    let key = format!("Software\\Classes\\AppUserModelId\\{app_id}");
    let value = Real.get(Hive::Hkcu, &key, "CustomActivator")?;
    parse_guid(value.text()?)
}

/// `{xxxxxxxx-xxxx-...}` or without the braces.
pub fn parse_guid(text: &str) -> Option<GUID> {
    GUID::try_from(text.trim().trim_start_matches('{').trim_end_matches('}')).ok()
}

/// Open a URI or path with whatever handles it; never with an error box of Windows' own (an
/// app named by a toast may be gone).
pub fn open(target: &str) -> bool {
    let verb = HSTRING::from("open");
    let file = HSTRING::from(target);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_FLAG_NO_UI,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guids_with_or_without_braces() {
        let g = GUID::from_u128(0xe9852f6c_08d6_4ec9_bd14_8a0fde975159);
        assert_eq!(parse_guid("{E9852F6C-08D6-4EC9-BD14-8A0FDE975159}"), Some(g));
        assert_eq!(parse_guid("e9852f6c-08d6-4ec9-bd14-8a0fde975159"), Some(g));
        assert_eq!(parse_guid("nope"), None);
    }

    #[test]
    fn packaged_apps_have_no_activator_shellman_can_call() {
        assert_eq!(activator("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"), None);
        assert_eq!(activator(""), None);
    }
}
