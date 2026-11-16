//! The dock's settings, saved in `%APPDATA%\shellman\dock.json`: the pinned apps and the icon
//! size.
//!
//! A pin is self-sufficient: besides what matches a running window to it (its app id and
//! program), it keeps what it is called, what a click starts and where its icon comes from, all
//! worked out once when it is pinned (see `dock_resolve`). A pin saved by an older bar has only
//! the id and the program; it still loads, and the bar fills in the rest the same way. Fields this
//! version does not know are kept as they were.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const NAME: &str = "dock";

/// What a click on a pin starts.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Launch {
    /// The app's entry in the Apps folder (`shell:AppsFolder\<id>`): Store and MSIX apps (only
    /// ever started this way, never by their `WindowsApps` path), and installed apps whose
    /// AppUserModelID is there (browsers' web apps, Firefox, Zed...).
    AppsFolder { id: String },
    /// A program and its command line after the program (empty for none).
    Exe {
        path: PathBuf,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        args: String,
    },
    /// A URL the shell opens with its handler (a Steam game's `steam://rungameid/<id>`).
    Url { url: String },
    /// A file the shell opens: a folder (in Files), a shortcut, a document.
    Open { path: PathBuf },
}

impl Launch {
    /// The program this starts, when it names one.
    pub fn program(&self) -> Option<&Path> {
        match self {
            Launch::Exe { path, .. } => Some(path),
            _ => None,
        }
    }

    /// The file this needs to exist, when it needs one (an Apps-folder entry or a URL is
    /// checked by the shell when started).
    pub fn file(&self) -> Option<&Path> {
        match self {
            Launch::Exe { path, .. } | Launch::Open { path } => Some(path),
            _ => None,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Pin {
    /// The id windows are grouped by: an AppUserModelID, or a program's path lower-cased.
    pub app_id: String,
    /// The window's program when pinned (or the file pinned by a drop): for matching windows.
    #[serde(default)]
    pub exe: Option<PathBuf>,
    /// The name the dock shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What a click starts when the app has no window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch: Option<Launch>,
    /// Where the icon comes from: a program, shortcut, `.ico`, or `shell:AppsFolder\<id>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<PathBuf>,
    /// A folder whose programs' windows all belong to this pin: a Squirrel app's root (its
    /// program moves to a new `app-x.y.z` folder on every update), a Steam game's install folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<PathBuf>,
    /// Fields written by a newer version, kept as they are.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Pin {
    /// A pin known only by its id and program, as older versions saved them; the rest is
    /// filled in by `dock_resolve`.
    pub fn new(app_id: impl Into<String>, exe: Option<PathBuf>) -> Self {
        Pin { app_id: app_id.into(), exe, name: None, launch: None, icon: None, home: None, extra: Default::default() }
    }

    /// Whether a window with this app id belongs to the pin. Ids are compared without case, since
    /// paths are and AppUserModelIDs are written both ways.
    pub fn matches(&self, app_id: &str) -> bool {
        self.app_id.eq_ignore_ascii_case(app_id)
    }

    /// Whether the name, the launch and the icon have been worked out (see `dock_resolve`).
    pub fn is_resolved(&self) -> bool {
        self.launch.is_some() && self.icon.is_some()
    }

    /// What a click opens, for the shell: the saved launch when there is one, else what older
    /// versions opened.
    pub fn launch_or_default(&self) -> Option<Launch> {
        if let Some(l) = &self.launch {
            return Some(l.clone());
        }
        let target = launch_target(&self.app_id, self.exe.as_deref())?;
        Some(match target.to_str().and_then(|t| t.strip_prefix(APPS_FOLDER)) {
            Some(id) => Launch::AppsFolder { id: id.to_owned() },
            None if is_program(&target) => Launch::Exe { path: target, args: String::new() },
            None => Launch::Open { path: target },
        })
    }

    /// Where the icon comes from: the saved source, else what older versions showed.
    pub fn icon_source(&self) -> Option<PathBuf> {
        self.icon.clone().or_else(|| icon_source(&self.app_id, self.exe.as_deref()))
    }
}

/// How well a window belongs to a pin, best first; `None` when it does not.
///
/// 1. the same AppUserModelID (or the same program path used as the id);
/// 2. the same Store package (a package's apps are one app here);
/// 3. the same program, compared without case and `\\?\`, and with shellman's own programs
///    renamed aside during an update (`shellman-x.old123.exe`) counted as themselves;
/// 4. a program inside the pin's home folder (a Squirrel app updated into a new `app-*` folder,
///    or a Steam game started from a `steam://` pin);
/// 5. the pin's own launch program, or the program the window asks to be relaunched with (Steam's
///    client window is `steamwebhelper.exe` but relaunches as `steam.exe`).
pub fn match_rank(pin: &Pin, app_id: &str, exe: Option<&Path>, relaunch: Option<&Path>) -> Option<u8> {
    if pin.matches(app_id) {
        return Some(1);
    }
    if let (Some(a), Some(b)) = (package_family(&pin.app_id), package_family(app_id)) {
        if a.eq_ignore_ascii_case(b) {
            return Some(2);
        }
    }
    // Two different AppUserModelIDs are two apps, whatever program runs them: each web app of
    // one browser, Firefox and its private windows, apps sharing a host process.
    let is_aumid = |id: &str| !id.is_empty() && !id.contains('\\');
    if is_aumid(&pin.app_id) && is_aumid(app_id) {
        return None;
    }
    let exe = exe.map(norm);
    // A window that names its own app is not that program's plain windows (a web app is not
    // its browser).
    let same_program_counts = !is_aumid(app_id);
    if let (Some(e), Some(p), true) = (&exe, pin.exe.as_deref().filter(|p| is_program(p)).map(norm), same_program_counts) {
        if *e == p {
            return Some(3);
        }
    }
    if let (Some(e), Some(home)) = (&exe, pin.home.as_deref().map(norm)) {
        if !home.is_empty() && e.starts_with(&format!("{home}\\")) {
            return Some(4);
        }
    }
    let launch = pin.launch.as_ref().and_then(Launch::program).map(norm);
    if let Some(l) = launch {
        if exe.as_ref() == Some(&l) || relaunch.map(norm).as_ref() == Some(&l) {
            return Some(5);
        }
    }
    if let (Some(r), Some(p)) = (relaunch.map(norm), pin.exe.as_deref().map(norm)) {
        if r == p {
            return Some(5);
        }
    }
    None
}

/// The pin a window belongs to, by `match_rank`: the best one, the first of equals.
pub fn pin_for_window(pins: &[Pin], app_id: &str, exe: Option<&Path>, relaunch: Option<&Path>) -> Option<usize> {
    pins.iter()
        .enumerate()
        .filter_map(|(i, p)| match_rank(p, app_id, exe, relaunch).map(|r| (r, i)))
        .min()
        .map(|(_, i)| i)
}

/// A path as compared: lower-case, backslashes, no `\\?\` prefix or trailing slash, and
/// shellman's own programs under their real names (see `canonical_exe`).
pub fn norm(p: &Path) -> String {
    let p = canonical_exe(p);
    let s = p.to_string_lossy().replace('/', "\\").to_lowercase();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.trim_end_matches('\\').to_owned()
}

/// shellman's installer renames a running program aside (`shellman-files.old222032.exe`) to put
/// the new one in its place; windows of such a program are the app itself.
pub fn canonical_exe(p: &Path) -> PathBuf {
    let Some(name) = p.file_name().and_then(|n| n.to_str()) else { return p.to_path_buf() };
    let lower = name.to_ascii_lowercase();
    if !lower.starts_with("shellman-") || !lower.ends_with(".exe") {
        return p.to_path_buf();
    }
    let stem = &name[..name.len() - 4];
    match lower[..lower.len() - 4].rfind(".old") {
        Some(at) if stem[at + 4..].chars().all(|c| c.is_ascii_alphanumeric()) && !stem[..at].contains('.') => {
            p.with_file_name(format!("{}.exe", &stem[..at]))
        }
        _ => p.to_path_buf(),
    }
}

/// `Family` of a Store app's `Family!App`.
pub fn package_family(app_id: &str) -> Option<&str> {
    is_packaged(app_id).then(|| app_id.split('!').next()).flatten()
}

fn is_program(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"))
}

/// How big the dock's icons are.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum IconSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl IconSize {
    pub const ALL: [IconSize; 3] = [IconSize::Small, IconSize::Medium, IconSize::Large];

    pub fn label(self) -> &'static str {
        match self {
            IconSize::Small => "Small",
            IconSize::Medium => "Medium",
            IconSize::Large => "Large",
        }
    }

    /// The icon's side, in points. The dock's spacing and height follow from it (see the bar's
    /// `dock::metrics`).
    pub fn icon(self) -> f32 {
        match self {
            IconSize::Small => 40.0,
            IconSize::Medium => 48.0,
            IconSize::Large => 56.0,
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DockSettings {
    pub pins: Vec<Pin>,
    pub icon_size: IconSize,
    /// Settings written by a newer version, kept as they are.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl DockSettings {
    pub fn load() -> Self {
        crate::config::load(NAME)
    }

    /// The saved settings, or why they could not be read (keep what you have then).
    pub fn try_load() -> Result<Self, crate::config::LoadError> {
        crate::config::try_load(NAME)
    }

    pub fn save(&self) {
        crate::config::save(NAME, self);
    }

    pub fn contains(&self, app_id: &str) -> bool {
        self.pins.iter().any(|p| p.matches(app_id))
    }

    /// Add a pin at the end, unless the app is already pinned.
    pub fn add(&mut self, app_id: &str, exe: Option<&Path>) {
        self.add_pin(Pin::new(app_id, exe.map(Path::to_path_buf)));
    }

    /// Add a pin at the end, unless the app is already pinned.
    pub fn add_pin(&mut self, pin: Pin) {
        if !pin.app_id.is_empty() && !self.contains(&pin.app_id) {
            self.pins.push(pin);
        }
    }

    pub fn remove(&mut self, app_id: &str) {
        self.pins.retain(|p| !p.matches(app_id));
    }

    /// Move the pin at `index` one place earlier (`down` false) or later. False at the ends.
    pub fn shift(&mut self, index: usize, down: bool) -> bool {
        let other = if down { index + 1 } else { index.wrapping_sub(1) };
        if index >= self.pins.len() || other >= self.pins.len() {
            return false;
        }
        self.pins.swap(index, other);
        true
    }
}

const APPS_FOLDER: &str = "shell:AppsFolder\\";

/// Store apps (package family and app, joined by `!`) can only be started through the Apps folder.
pub fn is_packaged(app_id: &str) -> bool {
    !app_id.contains('\\') && app_id.contains('!')
}

fn apps_folder(app_id: &str) -> PathBuf {
    PathBuf::from(format!("{APPS_FOLDER}{app_id}"))
}

/// What older versions opened to start the app: its program, or its entry in the Apps folder.
pub fn launch_target(app_id: &str, exe: Option<&Path>) -> Option<PathBuf> {
    if is_packaged(app_id) {
        return Some(apps_folder(app_id));
    }
    icon_source(app_id, exe)
}

/// Where older versions took the icon from: the program file's own icon, as Explorer's taskbar
/// shows it. Only without one, the app's entry in the Apps folder.
pub fn icon_source(app_id: &str, exe: Option<&Path>) -> Option<PathBuf> {
    let is_path = app_id.contains('\\');
    exe.map(Path::to_path_buf)
        .or_else(|| (!is_path && !app_id.is_empty()).then(|| apps_folder(app_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_remove_and_match_ignore_case() {
        let mut pins = DockSettings::default();
        pins.add("C:\\Apps\\Foo.exe", Some(Path::new("C:\\Apps\\Foo.exe")));
        pins.add("c:\\apps\\foo.exe", None);
        pins.add("", None);
        assert_eq!(pins.pins.len(), 1);
        assert!(pins.contains("c:\\apps\\FOO.exe"));
        pins.remove("C:\\APPS\\foo.exe");
        assert!(pins.pins.is_empty());
    }

    #[test]
    fn shifting_stops_at_the_ends() {
        let mut s = DockSettings::default();
        for id in ["a", "b", "c"] {
            s.add(id, None);
        }
        let order = |s: &DockSettings| s.pins.iter().map(|p| p.app_id.clone()).collect::<String>();
        assert!(!s.shift(0, false));
        assert!(!s.shift(2, true));
        assert!(!s.shift(7, false));
        assert!(s.shift(0, true));
        assert_eq!(order(&s), "bac");
        assert!(s.shift(2, false));
        assert_eq!(order(&s), "bca");
    }

    #[test]
    fn launch_targets() {
        let exe = Path::new("C:\\a\\b.exe");
        assert_eq!(launch_target("c:\\a\\b.exe", Some(exe)), Some(exe.to_path_buf()));
        assert_eq!(
            launch_target("Pkg_123!App", Some(Path::new("C:\\host.exe"))),
            Some(PathBuf::from("shell:AppsFolder\\Pkg_123!App"))
        );
        assert_eq!(launch_target("Chrome", None), Some(PathBuf::from("shell:AppsFolder\\Chrome")));
        assert_eq!(launch_target("c:\\gone.exe", None), None);
        // An old pin's launch, as the dock reads it.
        let old = Pin::new("Pkg_1!App", Some(PathBuf::from(r"C:\Program Files\WindowsApps\p\a.exe")));
        assert_eq!(old.launch_or_default(), Some(Launch::AppsFolder { id: "Pkg_1!App".into() }));
        let folder = Pin::new(r"c:\work", Some(PathBuf::from(r"C:\Work")));
        assert_eq!(folder.launch_or_default(), Some(Launch::Open { path: PathBuf::from(r"C:\Work") }));
        let exe = Pin::new(r"c:\a\b.exe", Some(PathBuf::from(r"C:\a\b.exe")));
        assert_eq!(exe.launch_or_default(), Some(Launch::Exe { path: PathBuf::from(r"C:\a\b.exe"), args: String::new() }));
    }

    #[test]
    fn file_round_trip() {
        let dir = std::env::temp_dir().join(format!("shellman-dock-{}", std::process::id()));
        let path = dir.join("dock.json");
        let mut s = DockSettings::default();
        s.add("x", Some(Path::new("C:\\x.exe")));
        s.pins.push(Pin {
            name: Some("Steam".into()),
            launch: Some(Launch::Exe { path: PathBuf::from(r"C:\Steam\steam.exe"), args: String::new() }),
            icon: Some(PathBuf::from(r"C:\Steam\steam.exe")),
            ..Pin::new("Valve.Steam.Client", Some(PathBuf::from(r"C:\Steam\bin\cef\cef.win64\steamwebhelper.exe")))
        });
        s.pins.push(Pin { launch: Some(Launch::Url { url: "steam://rungameid/1".into() }), ..Pin::new("g", None) });
        s.icon_size = IconSize::Large;
        crate::config::save_to(&path, &s);
        assert_eq!(crate::config::load_from::<DockSettings>(&path), s);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn old_files_without_new_fields_still_load() {
        // What the bar wrote before the icon size existed.
        let old = r#"{ "pins": [ { "app_id": "a", "exe": null }, { "app_id": "b" } ] }"#;
        let s: DockSettings = serde_json::from_str(old).unwrap();
        assert_eq!(s.pins.len(), 2);
        assert_eq!(s.icon_size, IconSize::Medium);
        assert!(!s.pins[0].is_resolved());
        // And an empty object, or a missing file, is all defaults.
        assert_eq!(serde_json::from_str::<DockSettings>("{}").unwrap(), DockSettings::default());
        let missing = crate::config::load_from::<DockSettings>(Path::new("Z:\\no\\such\\dock.json"));
        assert_eq!(missing, DockSettings::default());
    }

    #[test]
    fn the_users_saved_pins_load_and_unknown_fields_survive() {
        let saved = r#"{"pins":[
 {"app_id":"308046B0AF4A39CB","exe":"C:\\Program Files\\Mozilla Firefox\\firefox.exe"},
 {"app_id":"c:\\users\\x\\appdata\\local\\discord\\app-1.0.9268\\discord.exe","exe":"C:\\Users\\X\\AppData\\Local\\Discord\\app-1.0.9268\\Discord.exe","future":[1,2]}],
 "icon_size":"Small","later":true}"#;
        let s: DockSettings = serde_json::from_str(saved).unwrap();
        assert_eq!(s.pins.len(), 2);
        assert_eq!(s.icon_size, IconSize::Small);
        let back = serde_json::to_value(&s).unwrap();
        assert_eq!(back["later"], serde_json::json!(true));
        assert_eq!(back["pins"][1]["future"], serde_json::json!([1, 2]));
        // Nothing new is written for fields that are not known.
        assert!(back["pins"][0].get("name").is_none() && back["pins"][0].get("launch").is_none());
    }

    #[test]
    fn sizes_grow_in_steps() {
        let [s, m, l] = IconSize::ALL.map(IconSize::icon);
        assert_eq!((s, m, l), (40.0, 48.0, 56.0));
    }

    #[test]
    fn shellman_programs_renamed_aside_are_themselves() {
        let c = |p: &str| canonical_exe(Path::new(p));
        assert_eq!(c(r"C:\P\shellman-files.old222032.exe"), PathBuf::from(r"C:\P\shellman-files.exe"));
        assert_eq!(c(r"C:\P\Shellman-Terminal.OLD1.exe"), PathBuf::from(r"C:\P\Shellman-Terminal.OLD1.exe").with_file_name("Shellman-Terminal.exe"));
        assert_eq!(c(r"C:\P\shellman-files.exe"), PathBuf::from(r"C:\P\shellman-files.exe"));
        assert_eq!(c(r"C:\P\other.old1.exe"), PathBuf::from(r"C:\P\other.old1.exe"));
        assert_eq!(c(r"C:\P\shellman-x.old1.2.exe"), PathBuf::from(r"C:\P\shellman-x.old1.2.exe"));
        assert_eq!(norm(Path::new(r"\\?\C:\P\Shellman-Files.old9.exe")), r"c:\p\shellman-files.exe");
    }

    /// Which pin each kind of window lights up.
    #[test]
    fn windows_find_their_pins() {
        let p = |s: &str| PathBuf::from(s);
        let steam = Pin {
            launch: Some(Launch::Exe { path: p(r"C:\Program Files (x86)\Steam\steam.exe"), args: String::new() }),
            ..Pin::new("Valve.Steam.Client", Some(p(r"C:\Program Files (x86)\Steam\bin\cef\cef.win64\steamwebhelper.exe")))
        };
        // Steam pinned by dropping its shortcut: known as steam.exe.
        let steam_lnk = Pin::new(r"c:\program files (x86)\steam\steam.exe", Some(p(r"C:\Program Files (x86)\Steam\steam.exe")));
        let game = Pin {
            launch: Some(Launch::Url { url: "steam://rungameid/1938090".into() }),
            home: Some(p(r"C:\Program Files (x86)\Steam\steamapps\common\Call of Duty HQ")),
            ..Pin::new(r"c:\users\x\start menu\steam\call of duty.url", Some(p(r"C:\Users\X\Start Menu\Steam\Call of Duty.url")))
        };
        let discord = Pin {
            launch: Some(Launch::Exe { path: p(r"C:\U\Discord\Update.exe"), args: "--processStart Discord.exe".into() }),
            home: Some(p(r"C:\U\Discord")),
            ..Pin::new(r"c:\u\discord\app-1.0.9268\discord.exe", Some(p(r"C:\U\Discord\app-1.0.9268\Discord.exe")))
        };
        let store = Pin::new("Agilebits.1Password_amwd9z03whsfe!Agilebits.OnePassword", None);
        let files = Pin::new(r"c:\p\shellman-files.exe", Some(p(r"C:\P\shellman-files.exe")));
        let pins = [steam.clone(), game, discord, store, files];
        let at = |id: &str, exe: &str, re: Option<&str>| pin_for_window(&pins, id, Some(Path::new(exe)), re.map(Path::new));
        let helper = r"C:\Program Files (x86)\Steam\bin\cef\cef.win64\steamwebhelper.exe";
        assert_eq!(at("Valve.Steam.Client", helper, Some(r"C:\Program Files (x86)\Steam\steam.exe")), Some(0));
        // Steam's own (tray) process, and the game it started, each on their own pin.
        assert_eq!(at(r"c:\program files (x86)\steam\steam.exe", r"C:\Program Files (x86)\Steam\steam.exe", None), Some(0));
        let cod = r"C:\Program Files (x86)\Steam\steamapps\common\Call of Duty HQ\cod.exe";
        assert_eq!(at(&cod.to_lowercase(), cod, None), Some(1));
        // A helper window without Steam's id (a popup) is Steam's too: the same program.
        assert_eq!(at(&helper.to_lowercase(), helper, None), Some(0));
        // Discord after an update: a new versioned folder, still its pin.
        assert_eq!(at(r"c:\u\discord\app-1.0.9300\discord.exe", r"C:\U\Discord\app-1.0.9300\Discord.exe", None), Some(2));
        // Another app of the same package.
        assert_eq!(at("Agilebits.1Password_amwd9z03whsfe!Other", r"C:\Program Files\WindowsApps\x\1Password.exe", None), Some(3));
        // shellman's Files while an update has its program renamed aside.
        assert_eq!(at(r"c:\p\shellman-files.old222032.exe", r"C:\P\shellman-files.old222032.exe", None), Some(4));
        assert_eq!(at(r"c:\other.exe", r"C:\other.exe", None), None);
        // The Steam shortcut pin also takes Steam's client window, by the window's relaunch command.
        let only_lnk = [steam_lnk];
        assert_eq!(
            pin_for_window(&only_lnk, "Valve.Steam.Client", Some(Path::new(helper)), Some(Path::new(r"C:\Program Files (x86)\Steam\steam.exe"))),
            Some(0)
        );
        // Web apps of one browser are each their own app, though one program runs them all.
        let chrome = r"C:\Program Files\Google\Chrome\Application\chrome.exe";
        let proxy = r"C:\Program Files\Google\Chrome\Application\chrome_proxy.exe";
        let pwa = Pin {
            launch: Some(Launch::Exe { path: p(proxy), args: "--app-id=a".into() }),
            ..Pin::new("Chrome._crx_a", Some(p(chrome)))
        };
        let browser = Pin::new(chrome.to_lowercase(), Some(p(chrome)));
        let web = [pwa, browser];
        let w = |id: &str| pin_for_window(&web, id, Some(Path::new(chrome)), Some(Path::new(proxy)));
        assert_eq!(w("Chrome._crx_a"), Some(0));
        assert_eq!(w("Chrome._crx_b"), None);
        assert_eq!(pin_for_window(&web, &chrome.to_lowercase(), Some(Path::new(chrome)), None), Some(1));
        // A better match wins over an earlier, weaker one.
        let both = [discord_like_home(), steam];
        assert_eq!(pin_for_window(&both, "Valve.Steam.Client", Some(Path::new(helper)), None), Some(1));
    }

    fn discord_like_home() -> Pin {
        Pin { home: Some(PathBuf::from(r"C:\Program Files (x86)\Steam")), ..Pin::new("x", None) }
    }
}
