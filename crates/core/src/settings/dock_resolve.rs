//! Working out what a dock pin is: its name, what a click starts, where its icon comes from, and
//! the folder its windows live in. Done once when an app is pinned (and for pins saved by older
//! versions, and pins whose program moved), on a worker thread; the answers are saved in the pin
//! so the dock shows and starts the right thing at once after a restart.
//!
//! Pure rules over a [`World`] (the disk, the Apps folder, the Start menu), so each kind of app is
//! tested with fakes; `dock_world` is the real one.
//!
//! What a click starts, in order of preference:
//! 1. a file pinned by a drop (folder, shortcut, `.url`, document): that file;
//! 2. the command the window asks to be relaunched with, when it is the app's own (see
//!    [`relaunch_allowed`]): Steam's client window (run by `steamwebhelper.exe`) asks for
//!    `steam.exe`, browsers' web apps for the browser with their app id;
//! 3. Store and MSIX apps: their Apps-folder entry, never their `WindowsApps` program;
//! 4. an AppUserModelID the Apps folder has (Firefox, Zed, web apps);
//! 5. a Squirrel app (Discord...): `Update.exe --processStart <app>.exe`, which survives updates;
//! 6. a Steam game: `steam://rungameid/<id>`, from the library's `appmanifest_<id>.acf`;
//! 7. the Start menu shortcut that starts the program (or, for a helper program, the app in a
//!    folder above it);
//! 8. the program itself.

use super::dock::{canonical_exe, is_packaged, norm, package_family, Launch, Pin};
use crate::winlist::Relaunch;
use std::path::{Path, PathBuf};

/// A Start menu shortcut (`.lnk`) or internet shortcut (`.url`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shortcut {
    pub path: PathBuf,
    /// Its file name without the extension.
    pub name: String,
    pub target: Option<PathBuf>,
    pub args: String,
    /// `System.AppUserModel.ID` of a `.lnk`.
    pub aumid: Option<String>,
    /// The `URL=` of a `.url`.
    pub url: Option<String>,
    /// Its icon file (`IconFile=` of a `.url`, the icon location of a `.lnk`), if it names one.
    pub icon: Option<PathBuf>,
}

/// Folders a relaunch command's program may never be shared from, and folders it may not be in.
#[derive(Clone, Debug, Default)]
pub struct Places {
    /// Too broad to count as an app's own folder: Program Files, AppData, the profile...
    pub broad: Vec<PathBuf>,
    /// Never a relaunch program: Windows' own folder (cmd, PowerShell, rundll32...).
    pub system: Vec<PathBuf>,
}

/// What the resolver may look at.
pub trait World {
    fn exists(&self, p: &Path) -> bool;
    fn is_dir(&self, p: &Path) -> bool;
    /// Names of the folders in `dir`.
    fn subdirs(&self, dir: &Path) -> Vec<String>;
    /// Names of the files in `dir`.
    fn files(&self, dir: &Path) -> Vec<String>;
    fn read_text(&self, p: &Path) -> Option<String>;
    /// The Apps folder's name for `shell:AppsFolder\<id>`, if it has that entry.
    fn apps_folder_name(&self, id: &str) -> Option<String>;
    /// The Start menu's shortcuts.
    fn shortcuts(&self) -> Vec<Shortcut>;
    /// A shortcut file read.
    fn link(&self, p: &Path) -> Option<Shortcut>;
    /// A program's `FileDescription`.
    fn description(&self, exe: &Path) -> Option<String>;
    fn places(&self) -> Places;
    /// Steam's own folder, if Steam is installed.
    fn steam_root(&self) -> Option<PathBuf>;
}

/// The pin with what it lacks worked out. `relaunch` is what one of its windows asks to be
/// relaunched with, if one is open. What the pin already has and still holds is kept; a launch
/// or icon file that is gone is worked out again, and a versioned program that moved (Squirrel's
/// `app-x.y.z`) is followed to its newest folder.
pub fn resolve(pin: &Pin, relaunch: Option<&Relaunch>, w: &dyn World) -> Pin {
    let mut out = pin.clone();
    if let Some(exe) = out.exe.clone() {
        let exe = canonical_exe(&exe);
        let exe = if w.exists(&exe) { exe } else { newest_squirrel(&exe, w).unwrap_or(exe) };
        if is_path_id(&out.app_id) && Some(&exe) != out.exe.as_ref() && is_program(&exe) {
            // Known by its program's path: the id moves with it.
            out.app_id = exe.to_string_lossy().to_lowercase();
        }
        out.exe = Some(exe);
    }
    if out.launch.as_ref().and_then(Launch::file).is_some_and(|f| !w.exists(f)) {
        out.launch = None;
        out.icon = None;
    }
    if out.icon.as_deref().is_some_and(|i| is_file_path(i) && !w.exists(i)) {
        out.icon = None;
    }
    let found = find(&out, relaunch, w);
    // A launch set by a drop (a shortcut with its arguments) is the user's: keep it.
    let preset = out.launch.is_some();
    out.launch = out.launch.or(found.launch);
    out.name = out.name.or(found.name);
    out.home = out.home.or(found.home);
    out.icon = out.icon.or(if preset { pin.launch.as_ref().and_then(Launch::file).map(Path::to_path_buf) } else { None }).or(found.icon);
    out
}

/// Whether the pin's app is gone: nothing to start, and nothing of it left on the disk.
pub fn is_gone(pin: &Pin, w: &dyn World) -> bool {
    match &pin.launch {
        Some(l) => l.file().is_some_and(|f| !w.exists(f)),
        None => pin.exe.as_deref().is_none_or(|e| !w.exists(e)) && !is_packaged(&pin.app_id),
    }
}

#[derive(Default)]
struct Found {
    launch: Option<Launch>,
    name: Option<String>,
    icon: Option<PathBuf>,
    home: Option<PathBuf>,
}

fn find(pin: &Pin, relaunch: Option<&Relaunch>, w: &dyn World) -> Found {
    let app_id = pin.app_id.as_str();
    // Store apps hosted by ApplicationFrameHost say nothing about themselves by their program.
    let exe = pin.exe.clone().filter(|e| !is_frame_host(e));
    let own_name = |e: &Path| (!is_own(e)).then(|| describe(e, w)).flatten();

    // 1. A file pinned by a drop.
    if let Some(p) = exe.as_deref().filter(|p| !is_program(p) || w.is_dir(p)) {
        return pinned_file(p, w);
    }
    // 2. The window's own relaunch command.
    if let Some(r) = relaunch {
        if let Some(found) = from_relaunch(r, app_id, exe.as_deref(), w) {
            return found;
        }
    }
    // 3. Store and MSIX apps.
    if is_packaged(app_id) {
        let in_package = exe.as_deref().filter(|e| norm(e).contains(r"\windowsapps\"));
        return Found {
            launch: Some(Launch::AppsFolder { id: app_id.to_owned() }),
            name: w.apps_folder_name(app_id),
            // Their own program's icon, as the taskbar shows it; without one, the Apps folder's.
            icon: in_package.map(Path::to_path_buf).or_else(|| Some(apps_folder(app_id))),
            home: None,
        };
    }
    // 4. An AppUserModelID the Apps folder knows.
    if !is_path_id(app_id) && !app_id.is_empty() {
        if let Some(name) = w.apps_folder_name(app_id) {
            return Found {
                launch: Some(Launch::AppsFolder { id: app_id.to_owned() }),
                name: Some(name),
                icon: Some(apps_folder(app_id)),
                home: None,
            };
        }
    }
    let Some(exe) = exe else { return Found::default() };
    // 5. Squirrel.
    if let Some((root, file)) = squirrel(&exe, w) {
        let current = if w.exists(&exe) { Some(exe.clone()) } else { newest_squirrel(&exe, w) };
        return Found {
            launch: Some(Launch::Exe { path: root.join("Update.exe"), args: format!("--processStart {file}") }),
            name: current.as_deref().and_then(own_name).or_else(|| stem(Path::new(&file))),
            icon: current.or_else(|| Some(root.join("app.ico")).filter(|i| w.exists(i))),
            home: Some(root),
        };
    }
    // 6. A Steam game.
    if let Some(game) = steam_game_of_exe(&exe, w) {
        return game.found(&exe, w);
    }
    // 7. Its Start menu shortcut (not for shellman's own apps, which the bar draws itself).
    if let Some(s) = (!is_own(&exe)).then(|| shortcut_for(app_id, &exe, w)).flatten() {
        return Found {
            name: Some(s.name.clone()),
            icon: Some(s.path.clone()),
            launch: Some(Launch::Open { path: s.path }),
            home: None,
        };
    }
    // 8. The program.
    if w.exists(&exe) && !norm(&exe).contains(r"\windowsapps\") {
        return Found {
            name: own_name(&exe),
            icon: Some(exe.clone()),
            launch: Some(Launch::Exe { path: exe, args: String::new() }),
            home: None,
        };
    }
    Found::default()
}

/// A folder, shortcut, `.url` or document pinned by dropping it on the dock.
fn pinned_file(p: &Path, w: &dyn World) -> Found {
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let open = Found {
        launch: Some(Launch::Open { path: p.to_path_buf() }),
        icon: Some(p.to_path_buf()),
        // A folder by its whole name ("v1.2" is not "v1"), a drive by its letter, a shortcut
        // by its name without `.lnk`.
        name: if w.is_dir(p) {
            p.file_name().map(|n| n.to_string_lossy().into_owned()).or_else(|| Some(p.to_string_lossy().into_owned()))
        } else if matches!(ext.as_str(), "lnk" | "url") {
            stem(p)
        } else {
            p.file_name().map(|n| n.to_string_lossy().into_owned())
        },
        home: None,
    };
    if w.is_dir(p) {
        return open;
    }
    let Some(link) = (ext == "lnk" || ext == "url").then(|| w.link(p)).flatten() else { return open };
    // A Steam game's internet shortcut: its id finds the game's folder, for its windows.
    if let Some(game) = link.url.as_deref().and_then(steam_id).and_then(|id| steam_game_by_id(id, w)) {
        let icon = link.icon.clone().filter(|i| w.exists(i)).unwrap_or_else(|| p.to_path_buf());
        return Found {
            launch: Some(Launch::Url { url: format!("steam://rungameid/{}", game.id) }),
            name: game.name.or(open.name),
            icon: Some(icon),
            home: Some(game.home),
        };
    }
    // A shortcut to a Squirrel app (`Update.exe --processStart Discord.exe`): its windows run
    // from a versioned folder under Update.exe's.
    let squirrel_home = link
        .target
        .as_deref()
        .filter(|t| t.file_name().is_some_and(|n| n.eq_ignore_ascii_case("update.exe")))
        .filter(|_| link.args.to_ascii_lowercase().contains("--processstart"))
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    Found { home: squirrel_home, ..open }
}

/// The relaunch command, when it is the app's own.
fn from_relaunch(r: &Relaunch, app_id: &str, exe: Option<&Path>, w: &dyn World) -> Option<Found> {
    let command = r.command.trim();
    // `shell:AppsFolder\<its own id>` is as good as its id.
    let shell = command.trim_matches('"');
    if let Some(id) = shell.get(..APPS_FOLDER.len()).filter(|p| p.eq_ignore_ascii_case(APPS_FOLDER)).map(|_| &shell[APPS_FOLDER.len()..]) {
        return id.eq_ignore_ascii_case(app_id).then(|| Found {
            launch: Some(Launch::AppsFolder { id: id.to_owned() }),
            name: plain_name(r).or_else(|| w.apps_folder_name(id)),
            icon: Some(apps_folder(id)),
            home: None,
        });
    }
    let (program, args) = split_command(command)?;
    if !relaunch_allowed(&program, exe, app_id, &w.shortcuts(), &w.places()) || !w.exists(&program) {
        return None;
    }
    // `file,index`: the file's own icon is index 0, all the dock can show.
    let icon = r.icon.as_deref().and_then(|i| {
        let (file, index) = i.rsplit_once(',').unwrap_or((i, "0"));
        let file = PathBuf::from(file.trim().trim_matches('"'));
        (index.trim() == "0" && w.exists(&file)).then_some(file)
    });
    Some(Found {
        name: plain_name(r).or_else(|| describe(&program, w)),
        icon: icon.or_else(|| Some(program.clone())),
        launch: Some(Launch::Exe { path: program, args }),
        home: None,
    })
}

/// A relaunch name that is text, not a string resource (`@file,-id`).
fn plain_name(r: &Relaunch) -> Option<String> {
    r.name.as_deref().map(str::trim).filter(|n| !n.is_empty() && !n.starts_with('@')).map(str::to_owned)
}

/// Whether a relaunch command's `program` may be run for the app. The command comes from another
/// app's window, so it is run only when its program is that app's own: in the folder of the
/// window's program or a folder above it that belongs to the app (not Program Files, AppData or
/// the like), or the target of the Start menu shortcut carrying the app's id, or in the app's own
/// Store package. Never anything in Windows' own folder (a planted `cmd /c ...`).
pub fn relaunch_allowed(program: &Path, exe: Option<&Path>, app_id: &str, shortcuts: &[Shortcut], places: &Places) -> bool {
    let p = norm(program);
    if !p.ends_with(".exe") || !is_absolute(&p) {
        return false;
    }
    if places.system.iter().map(|s| norm(s)).any(|s| p.starts_with(&format!("{s}\\"))) {
        return false;
    }
    if let Some(exe) = exe {
        let (pd, ed) = (parent(&p), parent(&norm(exe)));
        let broad = places.broad.iter().any(|b| norm(b) == pd);
        let own = ed == pd || ed.starts_with(&format!("{pd}\\"));
        // Below a drive's root and the broad folders: `c:\steam` is fine, `c:` is not.
        if own && !broad && pd.matches('\\').count() >= 1 {
            return true;
        }
    }
    if shortcuts.iter().any(|s| s.aumid.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(app_id)) && s.target.as_deref().is_some_and(|t| norm(t) == p)) {
        return true;
    }
    // A Store app's own package: `WindowsApps\<Name>_<version>_<arch>__<publisher id>\...`.
    if let Some((name, publisher)) = package_family(app_id).and_then(|f| f.rsplit_once('_')) {
        let folder = p.split(r"\windowsapps\").nth(1).and_then(|r| r.split('\\').next()).unwrap_or("");
        let (name, publisher) = (name.to_lowercase(), publisher.to_lowercase());
        if folder.starts_with(&format!("{name}_")) && folder.ends_with(&format!("__{publisher}")) {
            return true;
        }
    }
    false
}

/// A command line's program and the rest. An unquoted program may hold spaces (Steam's is
/// `C:\Program Files (x86)\Steam\steam.exe`): it runs to the first `.exe` that ends a word.
pub fn split_command(command: &str) -> Option<(PathBuf, String)> {
    let c = command.trim();
    if let Some(rest) = c.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some((PathBuf::from(&rest[..end]), rest[end + 1..].trim().to_owned()));
    }
    let lower = c.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find(".exe") {
        let end = from + at + 4;
        if lower[end..].chars().next().is_none_or(char::is_whitespace) {
            return Some((PathBuf::from(&c[..end]), c[end..].trim().to_owned()));
        }
        from = end;
    }
    let (program, rest) = c.split_once(char::is_whitespace).unwrap_or((c, ""));
    (!program.is_empty()).then(|| (PathBuf::from(program), rest.trim().to_owned()))
}

/// A Squirrel app's root and program name: the program sits in `<root>\app-x.y.z\` and
/// `<root>\Update.exe` exists.
pub fn squirrel(exe: &Path, w: &dyn World) -> Option<(PathBuf, String)> {
    let dir = exe.parent()?;
    let versioned = dir.file_name()?.to_string_lossy().to_ascii_lowercase().starts_with("app-");
    let root = dir.parent()?;
    (versioned && w.exists(&root.join("Update.exe"))).then(|| (root.to_path_buf(), exe.file_name().unwrap().to_string_lossy().into_owned()))
}

/// The same program in the newest `app-x.y.z` folder next to a Squirrel app's old one.
pub fn newest_squirrel(exe: &Path, w: &dyn World) -> Option<PathBuf> {
    let file = exe.file_name()?;
    let dir = exe.parent()?;
    if !dir.file_name()?.to_string_lossy().to_ascii_lowercase().starts_with("app-") {
        return None;
    }
    let root = dir.parent()?;
    w.subdirs(root)
        .into_iter()
        .filter_map(|d| {
            let version: Vec<u64> = d.get(4..)?.split('.').map(|n| n.parse().ok()).collect::<Option<_>>()?;
            d.to_ascii_lowercase().starts_with("app-").then_some((version, d))
        })
        .filter(|(_, d)| w.exists(&root.join(d).join(file)))
        .max()
        .map(|(_, d)| root.join(d).join(file))
}

/// A Steam game, from its library's `appmanifest_<id>.acf`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamGame {
    pub id: u64,
    pub name: Option<String>,
    /// `<library>\steamapps\common\<installdir>`.
    pub home: PathBuf,
}

impl SteamGame {
    fn found(self, exe: &Path, w: &dyn World) -> Found {
        let url = format!("steam://rungameid/{}", self.id);
        // The Start menu's internet shortcut Steam made for it carries the game's own icon.
        let shortcut = w.shortcuts().into_iter().find(|s| s.url.as_deref().is_some_and(|u| u.eq_ignore_ascii_case(&url)));
        let icon = shortcut.as_ref().and_then(|s| s.icon.clone().filter(|i| w.exists(i)).or_else(|| Some(s.path.clone())));
        Found {
            launch: Some(Launch::Url { url }),
            name: self.name.or_else(|| shortcut.map(|s| s.name)).or_else(|| describe(exe, w)),
            icon: icon.or_else(|| Some(exe.to_path_buf())),
            home: Some(self.home),
        }
    }
}

/// The game a program under `<library>\steamapps\common\<installdir>\` belongs to.
pub fn steam_game_of_exe(exe: &Path, w: &dyn World) -> Option<SteamGame> {
    let s = exe.to_string_lossy();
    let lower = s.to_lowercase();
    let at = lower.find(r"\steamapps\common\")?;
    let steamapps = PathBuf::from(&s[..at + r"\steamapps".len()]);
    let installdir = s[at + r"\steamapps\common\".len()..].split('\\').next()?.to_owned();
    w.files(&steamapps)
        .iter()
        .filter(|f| f.to_ascii_lowercase().starts_with("appmanifest_") && f.to_ascii_lowercase().ends_with(".acf"))
        .filter_map(|f| w.read_text(&steamapps.join(f)))
        .filter_map(|text| parse_manifest(&text, &steamapps))
        .find(|g| g.home.file_name().is_some_and(|d| d.to_string_lossy().eq_ignore_ascii_case(&installdir)))
}

/// The installed game with this id, in any of Steam's libraries.
pub fn steam_game_by_id(id: u64, w: &dyn World) -> Option<SteamGame> {
    let root = w.steam_root()?;
    let mut libraries = vec![root.clone()];
    if let Some(text) = w.read_text(&root.join("steamapps").join("libraryfolders.vdf")) {
        libraries.extend(vdf_values(&text, "path").into_iter().map(|p| PathBuf::from(p.replace(r"\\", r"\"))));
    }
    libraries.iter().find_map(|lib| {
        let steamapps = lib.join("steamapps");
        let text = w.read_text(&steamapps.join(format!("appmanifest_{id}.acf")))?;
        parse_manifest(&text, &steamapps).filter(|g| g.id == id)
    })
}

fn parse_manifest(text: &str, steamapps: &Path) -> Option<SteamGame> {
    let first = |k: &str| vdf_values(text, k).into_iter().next();
    let id = first("appid")?.parse().ok()?;
    let dir = first("installdir")?;
    Some(SteamGame { id, name: first("name"), home: steamapps.join("common").join(dir) })
}

/// Every `"key" "value"` pair's value for `key`, in a Valve KeyValues text.
fn vdf_values(text: &str, key: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('"').skip(1).step_by(2);
            let k = parts.next()?;
            let v = parts.next()?;
            k.eq_ignore_ascii_case(key).then(|| v.to_owned())
        })
        .collect()
}

/// `steam://rungameid/<id>` (or `steam://run/<id>`).
pub fn steam_id(url: &str) -> Option<u64> {
    let lower = url.trim().to_ascii_lowercase();
    let rest = lower.strip_prefix("steam://rungameid/").or_else(|| lower.strip_prefix("steam://run/"))?;
    rest.trim_end_matches('/').split(['/', '?']).next()?.parse().ok()
}

/// The Start menu shortcut that starts this app: one carrying its id, else one whose target is
/// its program, else (for a helper program such as `steamwebhelper.exe`) the closest one whose
/// program sits in a folder above it that is the app's own. That last guess is made only for an
/// app known by an id its window declared (`Valve.Steam.Client`): a pin known by its program's
/// path is that program, and starts as itself. And never in Windows' own folders, where unrelated
/// programs sit under one another (`System32\notepad.exe` below a tool's `C:\Windows\...`).
fn shortcut_for(app_id: &str, exe: &Path, w: &dyn World) -> Option<Shortcut> {
    let shortcuts: Vec<Shortcut> = w
        .shortcuts()
        .into_iter()
        .filter(|s| s.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")))
        .filter(|s| !crate::apps_index::noise::is_noise(&s.name, &s.target.as_deref().map(|t| t.to_string_lossy().into_owned()).unwrap_or_default()))
        .collect();
    if let Some(s) = shortcuts.iter().find(|s| s.aumid.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(app_id))) {
        return Some(s.clone());
    }
    let e = norm(exe);
    if let Some(s) = shortcuts.iter().find(|s| s.target.as_deref().is_some_and(|t| norm(t) == e)) {
        return Some(s.clone());
    }
    let places = w.places();
    let in_system = |d: &str| places.system.iter().map(|s| norm(s)).any(|s| d == s || d.starts_with(&format!("{s}\\")));
    let ed = parent(&e);
    if is_path_id(app_id) || in_system(&ed) {
        return None;
    }
    shortcuts
        .iter()
        .filter_map(|s| {
            let t = norm(s.target.as_deref()?);
            let td = parent(&t);
            let above = ed.starts_with(&format!("{td}\\"));
            let broad = places.broad.iter().any(|b| norm(b) == td) || td.matches('\\').count() < 1 || in_system(&td);
            (t.ends_with(".exe") && above && !broad).then_some((td.len(), s))
        })
        .max_by_key(|(depth, _)| *depth)
        .map(|(_, s)| s.clone())
}

const APPS_FOLDER: &str = "shell:AppsFolder\\";

fn apps_folder(id: &str) -> PathBuf {
    PathBuf::from(format!("{APPS_FOLDER}{id}"))
}

fn describe(exe: &Path, w: &dyn World) -> Option<String> {
    w.description(exe).filter(|d| !d.trim().is_empty()).or_else(|| stem(exe))
}

fn stem(p: &Path) -> Option<String> {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|s| !s.trim().is_empty())
}

fn is_program(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"))
}

/// One of shellman's own programs, named by the bar itself.
fn is_own(p: &Path) -> bool {
    p.file_name().is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().starts_with("shellman-"))
}

fn is_frame_host(p: &Path) -> bool {
    p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("ApplicationFrameHost.exe"))
}

/// A file on disk, as opposed to `shell:` and other shell names.
fn is_file_path(p: &Path) -> bool {
    is_absolute(&p.to_string_lossy())
}

fn is_absolute(p: &str) -> bool {
    let b = p.as_bytes();
    (b.len() >= 3 && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')) || p.starts_with(r"\\")
}

fn is_path_id(app_id: &str) -> bool {
    app_id.contains('\\')
}

/// The folder part of a normalised path.
fn parent(p: &str) -> String {
    p.rsplit_once('\\').map_or(String::new(), |(d, _)| d.to_owned())
}

#[cfg(test)]
#[path = "dock_resolve_tests.rs"]
mod tests;
