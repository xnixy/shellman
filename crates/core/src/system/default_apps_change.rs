//! Changing a default app, the one way that works under shellman. Measured on Windows 11 build
//! 26300 without Explorer (notes in `docs/PLAN.md`, Default apps):
//!
//! - `IAssocHandler::MakeDefault` answers S_OK and writes nothing, even for a made-up type no
//!   driver guards. `SHOpenWithDialog` with the "register" flags only shows a notice ("To change
//!   your default apps, go to Settings > Apps > Default apps"). Windows' "Open with" chooser
//!   (`OpenWith.exe`, `OpenAs_RunDLL`) starts and shows nothing: its immersive flyout needs
//!   Explorer.
//! - Windows' Settings opens on one app's defaults with
//!   `ms-settings:defaultapps?registeredAppMachine=` / `registeredAppUser=` (the name the app is
//!   listed under in `RegisteredApplications`) or `registeredAUMID=` (a packaged app), and there
//!   the person picks; Settings writes the choice itself, protected types (`http`, `https`, `.pdf`,
//!   `.htm(l)`, Office) included, and offers browsers a one-click "Set default". It needs the
//!   modern-apps host (`app_host`).
//!
//! So shellman lists the apps that claim a type and opens Windows' page for the one picked.
//! Nothing here writes a `UserChoice` key, computes its hash, or touches the User Choice
//! Protection Driver (`UCPD.sys`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::default_apps::{clean_exe, Kind};
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumValueW, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_NOTIFY,
    KEY_READ, REG_EXPAND_SZ, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME, REG_NOTIFY_THREAD_AGNOSTIC, REG_SZ, RRF_RT_REG_SZ,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::UI::Shell::{
    AssocQueryStringW, IAssocHandler, IEnumAssocHandlers, SHAssocEnumHandlers, SHAssocEnumHandlersForProtocolByApplication, SHLoadIndirectString,
    ASSOCF_NOTRUNCATE, ASSOCSTR_EXECUTABLE, ASSOC_FILTER_NONE,
};

/// Windows' own Default apps page, for anything not tied to one app.
pub const SETTINGS_PAGE: &str = "ms-settings:defaultapps";

/// File types whose choice `UCPD.sys` guards on this build (the driver spells out
/// `FileExts\<ext>\UserChoice` and `UserChoiceLatest` for each).
pub const PROTECTED_EXTENSIONS: [&str; 9] = [".pdf", ".htm", ".html", ".doc", ".docx", ".xls", ".xlsx", ".ppt", ".pptx"];
/// Link kinds the driver guards (`UrlAssociations\<scheme>\UserChoice`).
pub const PROTECTED_PROTOCOLS: [&str; 2] = ["http", "https"];

/// Whether the driver blocks every writer but Windows' own for this type.
pub fn is_protected(key: &str, kind: Kind) -> bool {
    match kind {
        Kind::Extension => PROTECTED_EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(key)),
        Kind::Protocol => PROTECTED_PROTOCOLS.iter().any(|p| p.eq_ignore_ascii_case(key)),
    }
}

/// One app that claims a type, as Windows' Settings knows it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Candidate {
    /// The name people see.
    pub name: String,
    /// The ProgId it registered for the type; the person's choice names it when it is the default.
    pub progid: String,
    /// The program, when Windows names one.
    pub exe: Option<PathBuf>,
    /// A file whose shell image is the app's icon: the program, or a packaged app's logo.
    pub icon: Option<PathBuf>,
    /// The `ms-settings:` link that opens this app's defaults.
    pub link: String,
}

/// Where in `RegisteredApplications` an app was found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Registered {
    /// A desktop app listed for everyone (`HKLM`).
    Machine(String),
    /// A desktop app listed for this user (`HKCU`).
    User(String),
    /// A packaged app, by its app user model ID.
    Packaged(String),
}

/// The Settings link for an app.
pub fn settings_link(app: &Registered) -> String {
    let (field, value) = match app {
        Registered::Machine(n) => ("registeredAppMachine", n),
        Registered::User(n) => ("registeredAppUser", n),
        Registered::Packaged(id) => ("registeredAUMID", id),
    };
    format!("{SETTINGS_PAGE}?{field}={}", encode(value))
}

/// A packaged app's ID from the path of its `Capabilities` key:
/// `...\Packages\<Name>_<Version>_<Arch>_<Resource>_<PublisherId>\<AppId>\Capabilities` gives
/// `<Name>_<PublisherId>!<AppId>`. Settings does not accept the `AppX...` registration name.
pub fn packaged_aumid(capabilities: &str) -> Option<String> {
    let mut parts = capabilities.trim_end_matches('\\').rsplit('\\');
    if !parts.next()?.eq_ignore_ascii_case("Capabilities") {
        return None;
    }
    let app = parts.next()?;
    let full = parts.next()?;
    if !parts.next()?.eq_ignore_ascii_case("Packages") {
        return None;
    }
    let fields: Vec<&str> = full.split('_').collect();
    if fields.len() != 5 || app.is_empty() || fields[0].is_empty() || fields[4].is_empty() {
        return None;
    }
    Some(format!("{}_{}!{app}", fields[0], fields[4]))
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Where the person's choice for a type lives (read only).
pub fn choice_path(key: &str, kind: Kind) -> String {
    match kind {
        Kind::Extension => format!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\{key}\\UserChoice"),
        Kind::Protocol => format!("Software\\Microsoft\\Windows\\Shell\\Associations\\UrlAssociations\\{key}\\UserChoice"),
    }
}

/// The ProgId the person chose for a type, if they chose one.
pub fn user_choice(key: &str, kind: Kind) -> Option<String> {
    read_sz(HKEY_CURRENT_USER, &choice_path(key, kind), "ProgId")
}

/// Which of `list` is the current default: the one whose ProgId the choice names, else the one
/// running the program Windows opens the type with, else the one of the name Windows gives.
pub fn current_index(list: &[Candidate], choice: Option<&str>, exe: Option<&std::path::Path>, name: Option<&str>) -> Option<usize> {
    if let Some(c) = choice {
        if let Some(i) = list.iter().position(|a| a.progid.eq_ignore_ascii_case(c)) {
            return Some(i);
        }
    }
    if let Some(exe) = exe {
        if let Some(i) = list.iter().position(|a| a.exe.as_deref().is_some_and(|p| p.as_os_str().eq_ignore_ascii_case(exe.as_os_str()))) {
            return Some(i);
        }
    }
    let name = name?;
    list.iter().position(|a| a.name.eq_ignore_ascii_case(name))
}

/// The apps that claim a type in `RegisteredApplications` (per user first, then for everyone),
/// sorted by name, each with the link that opens its page in Windows' Settings. Reads the
/// registry and app manifests: call it from a worker.
pub fn candidates(key: &str, kind: Kind) -> Vec<Candidate> {
    let _com = Com::new();
    let section = match kind {
        Kind::Extension => "FileAssociations",
        Kind::Protocol => "UrlAssociations",
    };
    let icons = handler_icons(key, kind);
    let mut out: Vec<Candidate> = Vec::new();
    for (root, per_user) in [(HKEY_CURRENT_USER, true), (HKEY_LOCAL_MACHINE, false)] {
        for (reg_name, caps) in values(root, "Software\\RegisteredApplications") {
            let Some(progid) = read_sz(root, &format!("{caps}\\{section}"), key) else { continue };
            let registered = match packaged_aumid(&caps) {
                Some(id) => Registered::Packaged(id),
                None if per_user => Registered::User(reg_name.clone()),
                None => Registered::Machine(reg_name.clone()),
            };
            let name = read_sz(root, &caps, "ApplicationName").map(|n| resolve_text(&n)).filter(|n| !n.is_empty()).unwrap_or_else(|| reg_name.clone());
            let exe = assoc_exe(&progid);
            let by_exe = exe.as_ref().and_then(|e| icons.iter().find(|(id, _, _)| id.eq_ignore_ascii_case(&e.to_string_lossy())));
            let by_name = || icons.iter().find(|(_, ui, _)| ui.eq_ignore_ascii_case(&name));
            let icon = by_exe.or_else(by_name).map(|(_, _, i)| i.clone()).or_else(|| exe.clone());
            let link = settings_link(&registered);
            if out.iter().any(|o| o.link == link) {
                continue;
            }
            out.push(Candidate { name, progid, exe, icon, link });
        }
    }
    out.sort_by_key(|c| c.name.to_lowercase());
    out
}

/// `@{Package?ms-resource://...}` and `@dll,-id` strings made into the text or path they name.
fn resolve_text(raw: &str) -> String {
    if !raw.starts_with('@') {
        return raw.to_owned();
    }
    let mut buf = [0u16; 1024];
    // SAFETY: the buffer is valid for the call.
    if unsafe { SHLoadIndirectString(&HSTRING::from(raw), &mut buf, None) }.is_err() {
        return String::new();
    }
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// The program a ProgId runs.
fn assoc_exe(progid: &str) -> Option<PathBuf> {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: the buffer and its length are valid for the call.
    unsafe { AssocQueryStringW(ASSOCF_NOTRUNCATE, ASSOCSTR_EXECUTABLE, &HSTRING::from(progid), None, Some(PWSTR(buf.as_mut_ptr())), &mut len) }.ok().ok()?;
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    clean_exe(&String::from_utf16_lossy(&buf[..end])).filter(|p| p.exists())
}

/// The shell's handlers for a type, as (program path, name, icon file): it knows packaged apps'
/// logos.
fn handler_icons(key: &str, kind: Kind) -> Vec<(String, String, PathBuf)> {
    let key = HSTRING::from(key);
    // SAFETY: plain calls with a valid string.
    let list: Option<IEnumAssocHandlers> = unsafe {
        match kind {
            Kind::Extension => SHAssocEnumHandlers(&key, ASSOC_FILTER_NONE).ok(),
            Kind::Protocol => SHAssocEnumHandlersForProtocolByApplication(&key).ok(),
        }
    };
    let mut out = Vec::new();
    let Some(list) = list else { return out };
    loop {
        let mut one: [Option<IAssocHandler>; 1] = [None];
        let mut got = 0u32;
        // SAFETY: one slot, and the count it fills.
        if unsafe { list.Next(&mut one, Some(&mut got)) }.is_err() || got == 0 {
            return out;
        }
        let Some(h) = &one[0] else { continue };
        // SAFETY: the handler is live; the strings are freed by `take`.
        unsafe {
            let Ok(name) = h.GetName() else { continue };
            let id = take(name);
            let ui = h.GetUIName().map(take).unwrap_or_default();
            let mut path = PWSTR::null();
            let mut index = 0i32;
            if h.GetIconLocation(&mut path, &mut index).is_ok() {
                let raw = take(path);
                let file = if raw.starts_with('@') { Some(PathBuf::from(resolve_text(&raw))) } else { clean_exe(&expand(&raw)) };
                if let Some(f) = file.filter(|f| f.exists()) {
                    out.push((id, ui, f));
                }
            }
        }
    }
}

fn take(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: the shell returned a NUL-terminated string it allocated with CoTaskMemAlloc.
    let s = unsafe { p.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(p.0 as _)) };
    s
}

fn expand(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                out.push_str(&std::env::var(name).unwrap_or_else(|_| format!("%{name}%")));
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

struct Com(bool);
impl Com {
    fn new() -> Self {
        // SAFETY: balanced in Drop.
        Com(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: pairs with the successful init.
            unsafe { CoUninitialize() };
        }
    }
}

/// The string values of a key, as (name, data).
fn values(root: HKEY, path: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut key = HKEY::default();
    // SAFETY: valid strings and out-pointer; the key is closed below.
    if unsafe { RegOpenKeyExW(root, &HSTRING::from(path), None, KEY_READ, &mut key) }.is_err() {
        return out;
    }
    for i in 0.. {
        let mut name = [0u16; 512];
        let mut name_len = name.len() as u32;
        let mut data = [0u8; 2048];
        let mut data_len = data.len() as u32;
        let mut ty = 0u32;
        // SAFETY: buffers and lengths are valid.
        let r = unsafe { RegEnumValueW(key, i, Some(PWSTR(name.as_mut_ptr())), &mut name_len, None, Some(&mut ty), Some(data.as_mut_ptr()), Some(&mut data_len)) };
        if r.is_err() {
            break;
        }
        if ty == REG_SZ.0 || ty == REG_EXPAND_SZ.0 {
            let wide: Vec<u16> = data[..data_len as usize].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            let end = wide.iter().position(|c| *c == 0).unwrap_or(wide.len());
            out.push((String::from_utf16_lossy(&name[..name_len as usize]), String::from_utf16_lossy(&wide[..end])));
        }
    }
    // SAFETY: opened above.
    let _ = unsafe { RegCloseKey(key) };
    out
}

fn read_sz(root: HKEY, path: &str, value: &str) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: the buffer and its byte length are valid.
    unsafe { RegGetValueW(root, &HSTRING::from(path), &HSTRING::from(value), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as _), Some(&mut len)) }
        .ok()
        .ok()?;
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    let s = String::from_utf16_lossy(&buf[..end]);
    (!s.is_empty()).then_some(s)
}

/// Calls `changed` whenever a per-user file or link choice changes, until it is dropped.
pub struct Watch {
    stop: Arc<AtomicBool>,
}

impl Watch {
    pub fn start(changed: impl Fn() + Send + 'static) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || watch(&flag, changed));
        Watch { stop }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

const WATCHED: [&str; 2] = ["Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts", "Software\\Microsoft\\Windows\\Shell\\Associations\\UrlAssociations"];

fn watch(stop: &AtomicBool, changed: impl Fn()) {
    let mut keys = Vec::new();
    for path in WATCHED {
        let mut key = HKEY::default();
        // SAFETY: valid string and out-pointer; closed below.
        if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), None, KEY_NOTIFY, &mut key) }.is_ok() {
            keys.push(key);
        }
    }
    // SAFETY: an unnamed auto-reset event, closed below.
    if let Ok(event) = unsafe { CreateEventW(None, false, false, None) } {
        let filter = REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_THREAD_AGNOSTIC;
        'outer: while !stop.load(Ordering::Relaxed) {
            for k in &keys {
                // SAFETY: the key and event are live; asynchronous, so it returns at once.
                if unsafe { RegNotifyChangeKeyValue(*k, true, filter, Some(event), true) }.is_err() {
                    break 'outer;
                }
            }
            loop {
                // Wake now and then to see whether the page has gone.
                // SAFETY: the event is live.
                let r = unsafe { WaitForSingleObject(event, 500) };
                if stop.load(Ordering::Relaxed) {
                    break 'outer;
                }
                if r == WAIT_OBJECT_0 {
                    // Several values change together; let them settle, then tell once.
                    std::thread::sleep(Duration::from_millis(200));
                    changed();
                    break;
                }
            }
        }
        // SAFETY: created above.
        let _ = unsafe { CloseHandle(event) };
    }
    for k in keys {
        // SAFETY: opened above.
        let _ = unsafe { RegCloseKey(k) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_driver_guarded_types_are_protected() {
        for k in [".pdf", ".PDF", ".html", ".htm", ".docx", ".xlsx", ".pptx"] {
            assert!(is_protected(k, Kind::Extension), "{k}");
        }
        assert!(is_protected("https", Kind::Protocol));
        assert!(is_protected("HTTP", Kind::Protocol));
        for k in [".txt", ".png", ".jpg", ".mp3", ".mp4", ".zip"] {
            assert!(!is_protected(k, Kind::Extension), "{k}");
        }
        assert!(!is_protected("mailto", Kind::Protocol));
        assert!(!is_protected("http", Kind::Extension));
    }

    #[test]
    fn settings_links_name_the_app() {
        assert_eq!(settings_link(&Registered::Machine("Firefox-308046B0AF4A39CB".into())), "ms-settings:defaultapps?registeredAppMachine=Firefox-308046B0AF4A39CB");
        assert_eq!(settings_link(&Registered::User("My App".into())), "ms-settings:defaultapps?registeredAppUser=My%20App");
        assert_eq!(
            settings_link(&Registered::Packaged("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App".into())),
            "ms-settings:defaultapps?registeredAUMID=Microsoft.WindowsNotepad_8wekyb3d8bbwe%21App"
        );
    }

    #[test]
    fn packaged_apps_are_named_by_their_aumid() {
        let caps = "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Packages\\Microsoft.WindowsNotepad_11.2609.11.0_x64__8wekyb3d8bbwe\\App\\Capabilities";
        assert_eq!(packaged_aumid(caps).as_deref(), Some("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App"));
        assert_eq!(packaged_aumid("Software\\Clients\\StartMenuInternet\\Firefox-308046B0AF4A39CB\\Capabilities"), None);
        assert_eq!(packaged_aumid("Software\\PdfCraft\\Capabilities"), None);
        assert_eq!(packaged_aumid(""), None);
    }

    #[test]
    fn choice_paths_are_per_kind() {
        assert!(choice_path(".txt", Kind::Extension).ends_with("FileExts\\.txt\\UserChoice"));
        assert!(choice_path("https", Kind::Protocol).ends_with("UrlAssociations\\https\\UserChoice"));
    }

    fn app(name: &str, progid: &str, exe: Option<&str>) -> Candidate {
        Candidate { name: name.into(), progid: progid.into(), exe: exe.map(PathBuf::from), icon: None, link: format!("l:{name}") }
    }

    #[test]
    fn the_current_default_is_found_by_progid_then_program() {
        let list = [app("Firefox", "FirefoxPDF-1", Some("C:\\F\\firefox.exe")), app("Edge", "MSEdgePDF", Some("C:\\E\\msedge.exe"))];
        assert_eq!(current_index(&list, Some("msedgepdf"), None, None), Some(1));
        assert_eq!(current_index(&list, Some("Other"), Some(Path::new("c:\\f\\FIREFOX.exe")), None), Some(0));
        assert_eq!(current_index(&list, Some("Other"), Some(Path::new("C:\\x.exe")), Some("edge")), Some(1));
        assert_eq!(current_index(&list, None, None, None), None);
        assert_eq!(current_index(&list, Some("Other"), Some(Path::new("C:\\x.exe")), Some("Paint")), None);
    }

    #[test]
    fn environment_names_are_expanded() {
        std::env::set_var("SHELLMAN_TEST_DIR", "C:\\X");
        assert_eq!(expand("%SHELLMAN_TEST_DIR%\\a.exe"), "C:\\X\\a.exe");
        assert_eq!(expand("100%"), "100%");
    }

    /// The watch hears a change under `FileExts`: writes and removes one value under a throw-away
    /// type's key (`.shellmantest`, never a real type or a `UserChoice` key).
    #[test]
    #[ignore]
    fn watch_hears_a_change() {
        let (tx, rx) = std::sync::mpsc::channel();
        let _w = Watch::start(move || {
            let _ = tx.send(());
        });
        std::thread::sleep(Duration::from_millis(300));
        let key = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\.shellmantest\\ShellmanProbe";
        let reg = |args: &[&str]| std::process::Command::new("reg").args(args).output().unwrap();
        reg(&["add", key, "/v", "probe", "/d", "1", "/f"]);
        let heard = rx.recv_timeout(Duration::from_secs(3)).is_ok();
        reg(&["delete", key, "/f"]);
        assert!(heard);
    }

    /// Lists the apps for a few types: `cargo test -p shellman-core --lib -- --ignored --nocapture
    /// --exact system::default_apps_change::tests::list_this_machine`.
    #[test]
    #[ignore]
    fn list_this_machine() {
        for (k, kind) in [(".txt", Kind::Extension), (".pdf", Kind::Extension), (".png", Kind::Extension), ("https", Kind::Protocol), ("mailto", Kind::Protocol)] {
            let list = candidates(k, kind);
            let choice = user_choice(k, kind);
            println!("{k}: choice {choice:?} current {:?}", current_index(&list, choice.as_deref(), None, None));
            for c in list {
                println!("    {:28} {:40} {}  {:?}", c.name, c.progid, c.link, c.icon);
            }
        }
    }
}
