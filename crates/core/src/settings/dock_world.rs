//! The real [`World`] the dock's pins are worked out against: this machine's disk, Apps folder,
//! Start menu and Steam. Blocks on the shell and the disk, so it is only used on worker threads.

use super::dock_resolve::{Places, Shortcut, World};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use windows::core::{Interface, HSTRING, PWSTR};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    FOLDERID_CommonStartMenu, FOLDERID_StartMenu, IShellItem, IShellLinkW, SHCreateItemFromParsingName, SHGetKnownFolderPath,
    ShellLink, KF_FLAG_DEFAULT, SIGDN_NORMALDISPLAY,
};

/// The Start menu is read at most this often; pins are not made many times a minute.
const SHORTCUTS_FRESH: Duration = Duration::from_secs(120);

static SHORTCUTS: LazyLock<Mutex<Option<(Instant, Vec<Shortcut>)>>> = LazyLock::new(Mutex::default);

pub struct Real;

impl Real {
    /// Ready for use on this thread (the shell's objects want COM).
    pub fn new() -> Self {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        Real
    }
}

impl Default for Real {
    fn default() -> Self {
        Self::new()
    }
}

impl World for Real {
    fn exists(&self, p: &Path) -> bool {
        p.exists()
    }

    fn is_dir(&self, p: &Path) -> bool {
        p.is_dir()
    }

    fn subdirs(&self, dir: &Path) -> Vec<String> {
        entries(dir, true)
    }

    fn files(&self, dir: &Path) -> Vec<String> {
        entries(dir, false)
    }

    fn read_text(&self, p: &Path) -> Option<String> {
        // Manifests and shortcuts are small; anything big is not one.
        if std::fs::metadata(p).ok()?.len() > 1 << 20 {
            return None;
        }
        let bytes = std::fs::read(p).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn apps_folder_name(&self, id: &str) -> Option<String> {
        if id.is_empty() {
            return None;
        }
        unsafe {
            let path = HSTRING::from(format!("shell:AppsFolder\\{id}"));
            let item: IShellItem = SHCreateItemFromParsingName(&path, None).ok()?;
            let raw: PWSTR = item.GetDisplayName(SIGDN_NORMALDISPLAY).ok()?;
            let s = raw.to_string().ok();
            CoTaskMemFree(Some(raw.0 as _));
            s.filter(|s| !s.trim().is_empty())
        }
    }

    fn shortcuts(&self) -> Vec<Shortcut> {
        let mut cache = SHORTCUTS.lock().unwrap();
        if let Some((at, list)) = cache.as_ref() {
            if at.elapsed() < SHORTCUTS_FRESH {
                return list.clone();
            }
        }
        let mut files = Vec::new();
        for id in [&FOLDERID_StartMenu, &FOLDERID_CommonStartMenu] {
            if let Some(dir) = known_folder(id) {
                collect(&dir.join("Programs"), &mut files, 0);
            }
        }
        let list: Vec<Shortcut> = files.iter().filter_map(|f| self.link(f)).collect();
        *cache = Some((Instant::now(), list.clone()));
        list
    }

    fn link(&self, p: &Path) -> Option<Shortcut> {
        let ext = p.extension()?.to_string_lossy().to_lowercase();
        let name = p.file_stem()?.to_string_lossy().into_owned();
        let base = Shortcut { path: p.to_path_buf(), name, ..Default::default() };
        match ext.as_str() {
            "url" => Some(internet_shortcut(base, &self.read_text(p)?)),
            "lnk" => unsafe { shell_link(base) },
            _ => None,
        }
    }

    fn description(&self, exe: &Path) -> Option<String> {
        crate::system::app_name::file_description(exe)
    }

    fn places(&self) -> Places {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        let mut broad: Vec<PathBuf> =
            ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "ProgramData", "LOCALAPPDATA", "APPDATA", "USERPROFILE", "PUBLIC"]
                .iter()
                .filter_map(|k| env(k))
                .collect();
        if let Some(local) = env("LOCALAPPDATA") {
            broad.push(local.join("Programs"));
            broad.push(local.join("Temp"));
        }
        if let Some(home) = env("USERPROFILE") {
            broad.extend(["Downloads", "Desktop", "Documents"].map(|d| home.join(d)));
        }
        let system = env("SystemRoot").or_else(|| env("windir")).into_iter().collect();
        Places { broad, system }
    }

    fn steam_root(&self) -> Option<PathBuf> {
        use crate::registry::{Hive, Registry};
        let v = crate::registry::Real.get(Hive::Hkcu, r"Software\Valve\Steam", "SteamPath")?;
        let p = PathBuf::from(v.text()?.replace('/', "\\"));
        p.is_dir().then_some(p)
    }
}

fn entries(dir: &Path, dirs: bool) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    read.flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() == dirs))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    unsafe {
        let raw: PWSTR = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let text = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0.cast()));
        text.map(PathBuf::from)
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let is = |x: &str| path.extension().is_some_and(|e| e.eq_ignore_ascii_case(x));
        match e.file_type() {
            Ok(t) if t.is_dir() && depth < 4 => collect(&path, out, depth + 1),
            Ok(t) if t.is_file() && (is("lnk") || is("url")) => out.push(path),
            _ => {}
        }
    }
}

/// A `.url` file's `URL=` and `IconFile=` (an INI file).
pub fn internet_shortcut(mut s: Shortcut, text: &str) -> Shortcut {
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        match k.trim().to_ascii_lowercase().as_str() {
            "url" => s.url = Some(v.trim().to_owned()),
            "iconfile" => s.icon = Some(PathBuf::from(v.trim())),
            _ => {}
        }
    }
    s
}

/// A `.lnk`'s target, arguments, icon file and AppUserModelID, without resolving it (which could
/// search the disk).
unsafe fn shell_link(mut s: Shortcut) -> Option<Shortcut> {
    let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
    link.cast::<IPersistFile>().ok()?.Load(&HSTRING::from(s.path.as_os_str()), STGM_READ).ok()?;
    let mut buf = [0u16; 1024];
    if link.GetPath(&mut buf, std::ptr::null_mut(), 0).is_ok() {
        s.target = Some(PathBuf::from(wide(&buf))).filter(|t| !t.as_os_str().is_empty());
    }
    let mut args = [0u16; 2048];
    if link.GetArguments(&mut args).is_ok() {
        s.args = wide(&args);
    }
    let mut icon = [0u16; 1024];
    let mut index = 0i32;
    if link.GetIconLocation(&mut icon, &mut index).is_ok() && index == 0 {
        let icon = wide(&icon);
        s.icon = (!icon.is_empty()).then(|| PathBuf::from(expand(&icon)));
    }
    if let Ok(store) = link.cast::<IPropertyStore>() {
        if let Ok(value) = store.GetValue(&PKEY_AppUserModel_ID) {
            if let Ok(text) = PropVariantToStringAlloc(&value) {
                s.aumid = text.to_string().ok().filter(|t| !t.is_empty());
                CoTaskMemFree(Some(text.0 as *const _));
            }
        }
    }
    Some(s)
}

fn wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// `%VAR%`s in a shortcut's icon path.
fn expand(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        let Some(len) = rest[start + 1..].find('%') else { break };
        let name = &rest[start + 1..start + 1 + len];
        out.push_str(&rest[..start]);
        match std::env::var(name) {
            Ok(v) => out.push_str(&v),
            Err(_) => out.push_str(&rest[start..start + len + 2]),
        }
        rest = &rest[start + len + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::dock::Pin;
    use crate::settings::dock_resolve::resolve;

    #[test]
    fn internet_shortcuts_give_their_url_and_icon() {
        let s = internet_shortcut(Shortcut::default(), "[{000214A0-0000-0000-C000-000000000046}]\r\nProp3=19,0\r\n[InternetShortcut]\r\nIDList=\r\nIconIndex=0\r\nURL=steam://rungameid/1938090\r\nIconFile=C:\\Program Files (x86)\\Steam\\steam\\games\\x.ico\r\n");
        assert_eq!(s.url.as_deref(), Some("steam://rungameid/1938090"));
        assert_eq!(s.icon, Some(PathBuf::from(r"C:\Program Files (x86)\Steam\steam\games\x.ico")));
    }

    #[test]
    fn variables_expand() {
        let root = std::env::var("SystemRoot").unwrap();
        assert_eq!(expand(r"%SystemRoot%\a.ico"), format!(r"{root}\a.ico"));
        assert_eq!(expand(r"%NO_SUCH_VAR_HERE%\a"), r"%NO_SUCH_VAR_HERE%\a");
        assert_eq!(expand("50%"), "50%");
    }

    /// Prints what each pinned app on this machine would be. Read-only; launches nothing. Run by
    /// hand: `cargo test -p shellman-core -- --ignored --nocapture what_pins_resolve_to`, with
    /// `SHELLMAN_PINS` naming a dock.json to read (default: the user's).
    #[test]
    #[ignore = "reads this machine's Start menu, Apps folder and Steam"]
    fn what_pins_resolve_to() {
        let path = std::env::var_os("SHELLMAN_PINS").map(PathBuf::from);
        let settings: super::super::dock::DockSettings = match path {
            Some(p) => crate::config::load_from(&p),
            None => super::super::dock::DockSettings::load(),
        };
        let w = Real::new();
        let mut pins = settings.pins;
        let helper = r"C:\Program Files (x86)\Steam\bin\cef\cef.win64\steamwebhelper.exe";
        let mut cases: Vec<(Pin, Option<crate::winlist::Relaunch>)> = pins.drain(..).map(|p| (p, None)).collect();
        // What pinning Steam's client window saves, with and without the window open.
        let steam = Pin::new("Valve.Steam.Client", Some(PathBuf::from(helper)));
        let relaunch = crate::winlist::Relaunch { command: r"C:\Program Files (x86)\Steam\steam.exe".into(), name: Some("Steam".into()), icon: None };
        cases.push((steam.clone(), Some(relaunch)));
        cases.push((steam, None));
        if let Ok(more) = std::env::var("SHELLMAN_MORE_PINS") {
            cases.extend(more.split(';').filter(|s| !s.is_empty()).map(|e| (Pin::new(e.to_lowercase(), Some(PathBuf::from(e))), None)));
        }
        for (pin, relaunch) in cases {
            let r = resolve(&pin, relaunch.as_ref(), &w);
            println!("{} | exe {:?}\n  -> name {:?}\n     launch {:?}\n     icon {:?}\n     home {:?}\n     gone {}", pin.app_id, pin.exe, r.name, r.launch, r.icon, r.home, super::super::dock_resolve::is_gone(&r, &w));
        }
    }
}
