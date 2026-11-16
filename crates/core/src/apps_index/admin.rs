//! Starting an app as administrator, behind the standard UAC prompt.
//!
//! A classic app (a program, a shortcut, a classic entry of the Apps folder) is started with the
//! shell's `runas` verb, so the prompt names the program itself. That verb works without
//! Explorer (measured on 26300, shellman as the shell).
//!
//! A Store app has no `runas` without Explorer (`shell:AppsFolder\<Family>!<App>` fails with
//! "class not registered", as `open` does). But a packaged desktop app (Terminal, Notepad: full
//! trust) started by the activation manager from an elevated process runs elevated (measured),
//! so those go through `shellman-elevate`'s `launch-app <id>`. A sandboxed Store app (Calculator,
//! Settings) cannot run as administrator at all: Windows offers no such thing for it, and here
//! it is not offered either (`can_elevate`) and, when asked for anyway, starts as usual.
//!
//! Everything that starts something blocks until the prompt is answered: worker threads only.

use super::activate::packaged_id;
use crate::elevate::{self, ops::Op, Outcome};
use std::collections::HashMap;
use std::sync::Mutex;
use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{GetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
use windows::Win32::Storage::Packaging::Appx::{GetPackagePathByFullName, GetPackagesByPackageFamily};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// The user said no at the UAC prompt.
const ERROR_CANCELLED: u32 = 1223;
/// The shell has no `runas` for this kind of thing (a document, a link to a web page).
const ERROR_NO_ASSOCIATION: u32 = 1155;

/// What can be run as administrator when it is not a Store app: programs, scripts, consoles and
/// shortcuts. Decided from the name alone, so asking never touches the disk.
const RUNNABLE: [&str; 6] = ["exe", "lnk", "bat", "cmd", "com", "msc"];

/// Whether "Run as administrator" makes sense for `target` (a path, or `shell:AppsFolder\<id>`),
/// as far as its spelling says: a Store app is `None`, since only its manifest knows (see
/// `can_elevate`).
pub fn by_name(target: &str) -> Option<bool> {
    if packaged_id(target).is_some() {
        return None;
    }
    if target.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("shell:")) {
        // A classic entry of the Apps folder is a program; any other shell place is not.
        return Some(target.get(..17).is_some_and(|p| p.eq_ignore_ascii_case("shell:AppsFolder\\")));
    }
    let ext = std::path::Path::new(target).extension().and_then(|e| e.to_str()).unwrap_or_default();
    Some(RUNNABLE.iter().any(|r| r.eq_ignore_ascii_case(ext)))
}

/// Whether `target` can run as administrator. Reads a Store app's manifest, so not for the UI
/// thread; `can_elevate_soon` is.
pub fn can_elevate(target: &str) -> bool {
    by_name(target).unwrap_or_else(|| packaged_id(target).is_some_and(packaged_full_trust))
}

/// `can_elevate` for the UI thread: the answer when the name gives it or it was found before,
/// else `None` while a worker finds it out, after which `wake` is called once.
pub fn can_elevate_soon(target: &str, wake: fn()) -> Option<bool> {
    static KNOWN: Mutex<Option<HashMap<String, Option<bool>>>> = Mutex::new(None);
    if let Some(known) = by_name(target) {
        return Some(known);
    }
    let mut known = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
    let known = known.get_or_insert_with(HashMap::new);
    if let Some(answer) = known.get(target) {
        return *answer;
    }
    known.insert(target.to_owned(), None);
    let target = target.to_owned();
    std::thread::spawn(move || {
        let answer = can_elevate(&target);
        if let Some(known) = KNOWN.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            known.insert(target, Some(answer));
        }
        wake();
    });
    None
}

/// Start `target` as administrator. Blocks until the UAC prompt is answered. False when nothing
/// was started and the user did not decline: the caller has nothing to show either way, but a
/// test wants to know. Something that cannot run as administrator is started as usual.
pub fn launch(target: &str) -> bool {
    if let Some(id) = packaged_id(target) {
        if !packaged_full_trust(id) {
            // On a thread of its own, as every plain start of a Store app.
            return super::activate::retry(target);
        }
        return match Op::launch_app(id) {
            Some(op) => matches!(elevate::run(&op), Outcome::Done | Outcome::Cancelled),
            None => false,
        };
    }
    match runas(target, None) {
        Ok(()) | Err(ERROR_CANCELLED) => true,
        Err(ERROR_NO_ASSOCIATION) => {
            crate::fsops::open(std::path::Path::new(target));
            true
        }
        Err(_) => false,
    }
}

/// Run what was typed as administrator, as `run_text` runs it: a path, a program name, or
/// `program arguments`. A folder opens in Files, as itself. Blocks like `launch`.
pub fn run_text(text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() {
        return false;
    }
    if std::path::Path::new(text.trim_matches('"')).is_dir() {
        return super::run_text(text);
    }
    match runas(text, None) {
        Ok(()) | Err(ERROR_CANCELLED) => return true,
        Err(ERROR_NO_ASSOCIATION) => return super::run_text(text),
        Err(_) => {}
    }
    match super::launch::split_command(text) {
        Some((file, args)) => matches!(runas(file, Some(args)), Ok(()) | Err(ERROR_CANCELLED)),
        None => false,
    }
}

/// The shell's `runas` on `file`; Err carries the Win32 error. No error box: the caller decides.
fn runas(file: &str, args: Option<&str>) -> Result<(), u32> {
    let file = HSTRING::from(file);
    let args = args.filter(|a| !a.is_empty()).map(HSTRING::from);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // The calling thread ends right after: the launch must be finished when this returns.
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: args.as_ref().map_or(PCWSTR::null(), |a| PCWSTR(a.as_ptr())),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: `info` and the strings it points to outlive the call.
    unsafe { ShellExecuteExW(&mut info).map_err(|_| GetLastError().0) }
}

/// Whether the Store app `<Family>!<App>` is a desktop program in a package, which can be
/// elevated, rather than a sandboxed one. Reads the package's manifest.
fn packaged_full_trust(id: &str) -> bool {
    let Some((family, app)) = id.split_once('!') else { return false };
    package_dirs(family).iter().any(|dir| {
        std::fs::read_to_string(std::path::Path::new(dir).join("AppxManifest.xml")).is_ok_and(|m| full_trust(&m, app))
    })
}

/// Whether `manifest` declares the application `app` as full trust: its `<Application>` element
/// has the desktop entry point or the medium trust level.
pub(crate) fn full_trust(manifest: &str, app: &str) -> bool {
    let ids = [format!("Id=\"{app}\""), format!("Id='{app}'")];
    manifest.split("<Application").skip(1).any(|rest| {
        // `<Applications>` is not one, and the element's own tag ends at the first `>`.
        let Some(tag) = rest.strip_prefix(char::is_whitespace).and_then(|r| r.split('>').next()) else { return false };
        // The id is a whole attribute, not the tail of another (`uap:Id`).
        let named = ids.iter().any(|id| tag.match_indices(id.as_str()).any(|(at, _)| at == 0 || tag[..at].ends_with(char::is_whitespace)));
        named && (tag.contains("Windows.FullTrustApplication") || tag.contains("mediumIL"))
    })
}

/// Where the installed packages of a family are.
fn package_dirs(family: &str) -> Vec<String> {
    let family = HSTRING::from(family);
    let (mut count, mut len) = (0u32, 0u32);
    // SAFETY: the first call only asks for the sizes; the second fills buffers of those sizes,
    // and the names it points to live in `buffer`, which outlives their use.
    unsafe {
        if GetPackagesByPackageFamily(&family, &mut count, None, &mut len, None) != ERROR_INSUFFICIENT_BUFFER || count == 0 {
            return Vec::new();
        }
        let mut names = vec![PWSTR::null(); count as usize];
        let mut buffer = vec![0u16; len as usize];
        if GetPackagesByPackageFamily(&family, &mut count, Some(names.as_mut_ptr()), &mut len, Some(PWSTR(buffer.as_mut_ptr()))) != ERROR_SUCCESS {
            return Vec::new();
        }
        names.iter().take(count as usize).filter_map(|&name| package_dir(PCWSTR(name.0))).collect()
    }
}

/// SAFETY: `full_name` must be a NUL-ended string.
unsafe fn package_dir(full_name: PCWSTR) -> Option<String> {
    let mut len = 0u32;
    if GetPackagePathByFullName(full_name, &mut len, None) != ERROR_INSUFFICIENT_BUFFER || len == 0 {
        return None;
    }
    let mut path = vec![0u16; len as usize];
    if GetPackagePathByFullName(full_name, &mut len, Some(PWSTR(path.as_mut_ptr()))) != ERROR_SUCCESS {
        return None;
    }
    Some(String::from_utf16_lossy(&path[..path.iter().position(|&c| c == 0).unwrap_or(path.len())]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"<Package><Applications>
        <Application Id="App" Executable="Notepad\Notepad.exe" EntryPoint="Windows.FullTrustApplication">
          <uap:VisualElements DisplayName="Notepad"/></Application>
        <Application Id="Calc" Executable="Calc.exe" EntryPoint="Calc.App"><uap:VisualElements/></Application>
        <Application uap10:TrustLevel="mediumIL" Id='Medium' Executable="m.exe" uap10:RuntimeBehavior="packagedClassicApp"/>
        <Application uap:Id="App2" Id="Other" EntryPoint="Windows.FullTrustApplication"/>
      </Applications></Package>"#;

    #[test]
    fn only_desktop_apps_in_a_package_are_full_trust() {
        assert!(full_trust(MANIFEST, "App"));
        assert!(full_trust(MANIFEST, "Medium"));
        assert!(full_trust(MANIFEST, "Other"));
        assert!(!full_trust(MANIFEST, "Calc"));
        assert!(!full_trust(MANIFEST, "App2"), "another attribute's tail is not the id");
        assert!(!full_trust(MANIFEST, "Missing"));
        assert!(!full_trust("", "App"));
    }

    #[test]
    fn the_name_says_what_can_be_elevated() {
        assert_eq!(by_name(r"C:\Windows\System32\cmd.exe"), Some(true));
        assert_eq!(by_name(r"C:\Users\x\Start Menu\Tool.LNK"), Some(true));
        assert_eq!(by_name(r"shell:AppsFolder\{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\cmd.exe"), Some(true));
        assert_eq!(by_name(r"shell:AppsFolder\Chrome"), Some(true));
        // A pinned folder, a document, the bin.
        assert_eq!(by_name(r"C:\Users\x\Projects"), Some(false));
        assert_eq!(by_name(r"C:\Users\x\notes.txt"), Some(false));
        assert_eq!(by_name("shell:RecycleBinFolder"), Some(false));
        assert_eq!(by_name(""), Some(false));
        // A Store app: its manifest knows.
        assert_eq!(by_name(r"shell:AppsFolder\Microsoft.WindowsNotepad_8wekyb3d8bbwe!App"), None);
    }

    #[test]
    #[ignore = "reads this machine's packages"]
    fn notepad_can_and_calculator_cannot() {
        assert!(can_elevate(r"shell:AppsFolder\Microsoft.WindowsNotepad_8wekyb3d8bbwe!App"));
        assert!(!can_elevate(r"shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"));
        assert!(!can_elevate(r"shell:AppsFolder\No.Such.Package_8wekyb3d8bbwe!App"));
    }
}
