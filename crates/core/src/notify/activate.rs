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
use windows::Win32::System::Com::{
    CoAllowSetForegroundWindow, CoCreateInstance, CoInitializeEx, CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Notifications::{INotificationActivationCallback, NOTIFICATION_USER_INPUT_DATA};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::UI::Shell::{
    IShellItem2, SHCreateItemFromParsingName, ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, SW_SHOWNORMAL};

/// Hand `invoke` to `app_id`. Blocking (it may start the app); call it off the UI thread.
pub fn deliver(app_id: &str, invoke: &Invoke) -> Result<Outcome, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    // The user just clicked shellman: let whatever answers come to the front. Windows refuses
    // the grant when shellman may not take the foreground itself (the click was a while ago, or
    // on a window that never activates); then one unused key press makes it so.
    crate::foreground::allow_foreground(ASFW_ANY);
    if invoke.kind == Activation::Protocol {
        return if open(&invoke.args) { Ok(Outcome::Opened) } else { Err(format!("cannot open {}", invoke.args)) };
    }
    let Some(clsid) = activator(app_id) else { return Ok(Outcome::NoHandler) };
    let callback: INotificationActivationCallback =
        unsafe { CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER) }.map_err(|e| format!("{:#010x} {}", e.code().0, e.message()))?;
    // The documented grant for a COM server about to show its window; the one above covers a
    // helper process of the app's.
    unsafe {
        let _ = CoAllowSetForegroundWindow(&callback, None);
    }
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
    in_shortcuts(|path| shortcut_activator(path, app_id))
}

/// The folder of the program `app_id`'s Start menu shortcut starts, lowercased, for finding the
/// app's windows when they do not carry its AppUserModelID (Discord's shortcut starts
/// `Discord\Update.exe`, its windows belong to `Discord\app-1.0\Discord.exe` and declare no
/// id). None for a packaged app, an app without such a shortcut, and a program in Windows' own
/// folder, which is nobody's in particular. Walks the Start menu: not for the UI thread.
pub fn program_dir(app_id: &str) -> Option<String> {
    if app_id.is_empty() || app_id.contains('!') {
        return None;
    }
    let target = in_shortcuts(|path| shortcut_target(path, app_id))?;
    let dir = std::path::Path::new(&target).parent()?.to_string_lossy().to_lowercase();
    let windows = std::env::var("windir").unwrap_or_else(|_| r"C:\Windows".into()).to_lowercase();
    (!dir.is_empty() && !dir.starts_with(&windows)).then_some(dir)
}

/// Whether the program `exe` lives in `dir` (from `program_dir`) or a folder under it.
pub fn in_program_dir(exe: &std::path::Path, dir: &str) -> bool {
    let exe = exe.to_string_lossy().to_lowercase();
    exe.strip_prefix(dir).is_some_and(|rest| rest.starts_with('\\'))
}

/// The first answer `found` gives for a Start menu shortcut (the user's, then everyone's).
fn in_shortcuts<T>(found: impl Fn(&std::path::Path) -> Option<T>) -> Option<T> {
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
                if let Some(answer) = found(&path) {
                    return Some(answer);
                }
            }
        }
    }
    None
}

/// The shortcut as a shell item, if the shortcut is `app_id`'s.
fn shortcut_of(path: &std::path::Path, app_id: &str) -> Option<IShellItem2> {
    let item: IShellItem2 = unsafe { SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None) }.ok()?;
    string_of(&item, &PKEY_AppUserModel_ID).is_some_and(|s| s.eq_ignore_ascii_case(app_id)).then_some(item)
}

fn string_of(item: &IShellItem2, key: &PROPERTYKEY) -> Option<String> {
    unsafe {
        let text = item.GetString(key).ok()?;
        let s = text.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(text.0 as *const _));
        s
    }
}

/// The shortcut's toast activator, if the shortcut is `app_id`'s.
fn shortcut_activator(path: &std::path::Path, app_id: &str) -> Option<GUID> {
    let item = shortcut_of(path, app_id)?;
    unsafe { item.GetCLSID(&PKEY_AppUserModel_ToastActivatorCLSID) }.ok().filter(|g| *g != GUID::zeroed())
}

/// The program the shortcut starts, if the shortcut is `app_id`'s.
fn shortcut_target(path: &std::path::Path, app_id: &str) -> Option<String> {
    /// `System.Link.TargetParsingPath` (the windows crate lacks it).
    const TARGET: PROPERTYKEY = PROPERTYKEY { fmtid: GUID::from_u128(0xb9b4b3fc_2b51_4a42_b5d8_324146afcf25), pid: 2 };
    string_of(&shortcut_of(path, app_id)?, &TARGET).filter(|t| t.contains('\\'))
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
    // A Store app's entry in the Apps folder is refused when no Explorer runs.
    unsafe { ShellExecuteExW(&mut info) }.is_ok() || crate::apps_index::activate::retry(target)
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
    fn a_window_is_the_apps_when_its_program_is_under_the_shortcuts_folder() {
        let dir = r"c:\users\x\appdata\local\discord";
        let exe = |p: &str| std::path::PathBuf::from(p);
        assert!(in_program_dir(&exe(r"C:\Users\X\AppData\Local\Discord\app-1.0.9268\Discord.exe"), dir));
        assert!(in_program_dir(&exe(r"C:\Users\X\AppData\Local\Discord\Update.exe"), dir));
        // A neighbour whose name only starts the same is another app.
        assert!(!in_program_dir(&exe(r"C:\Users\X\AppData\Local\DiscordPTB\Discord.exe"), dir));
        assert_eq!(program_dir("Family_abc!App"), None);
        assert_eq!(program_dir(""), None);
    }

    #[test]
    fn packaged_apps_have_no_activator_shellman_can_call() {
        assert_eq!(activator("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"), None);
        assert_eq!(activator(""), None);
    }
}
