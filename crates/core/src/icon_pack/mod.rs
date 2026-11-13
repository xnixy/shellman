//! Icon packs: the user picks a pack once and every app icon shellman draws comes from it when it
//! has a match, and from the app itself when it does not.
//!
//! Every app calls `resolve` (through `Thumbs`, which all icon drawing goes through) and gets the
//! file to draw instead of the app's own, or none. The pack is read once into an index on a
//! background thread; until it is ready, and for apps it has nothing for, the real icon shows.
//! `icons.json` is looked at again at most twice a second, and a change (made by Settings, in
//! another process) bumps `generation`, so whoever caches by it starts afresh.

pub mod builtin;
pub mod import;
pub mod index;
pub mod names;
pub mod raster;
pub mod settings;

pub use index::PackIndex;
pub use names::AppRef;
pub use settings::IconSettings;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

/// How often the settings file is looked at.
const CHECK_EVERY: Duration = Duration::from_millis(500);

type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(PartialEq, Eq, Clone, Default)]
struct Stamp {
    settings: Option<(SystemTime, u64)>,
    pack: Option<SystemTime>,
}

#[derive(Default)]
struct Inner {
    checked: Option<Instant>,
    stamp: Stamp,
    settings: IconSettings,
    /// The pack the index was built for (or is being built for).
    pack: Option<String>,
    index: Option<Arc<PackIndex>>,
    generation: u64,
    /// What each app resolved to, until anything changes.
    cache: HashMap<String, Option<PathBuf>>,
    wakers: Vec<Waker>,
}

fn state() -> &'static Mutex<Inner> {
    static STATE: OnceLock<Mutex<Inner>> = OnceLock::new();
    STATE.get_or_init(Default::default)
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn settings_stamp() -> Option<(SystemTime, u64)> {
    let path = crate::config::config_dir()?.join(format!("{}.json", settings::NAME));
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Notice a changed `icons.json` or pack folder, if it is time to look.
fn refresh(inner: &mut Inner, force: bool) {
    if !force && inner.checked.is_some_and(|t| t.elapsed() < CHECK_EVERY) {
        return;
    }
    inner.checked = Some(Instant::now());
    // The built-in pack has no folder to watch.
    let pack_dir = |s: &IconSettings| s.pack.as_ref().filter(|p| !builtin::is_builtin(p)).and_then(|p| import::packs_dir().map(|d| d.join(p)));
    let first = inner.generation == 0;
    let settings_now = settings_stamp();
    let pack_now = pack_dir(&inner.settings).and_then(|d| modified(&d));
    if !first && inner.stamp.settings == settings_now && inner.stamp.pack == pack_now {
        return;
    }
    let settings = IconSettings::load();
    let pack_changed = settings.pack != inner.pack || inner.stamp.pack != pack_now;
    inner.settings = settings;
    inner.stamp = Stamp { settings: settings_now, pack: pack_dir(&inner.settings).and_then(|d| modified(&d)) };
    inner.generation += 1;
    inner.cache.clear();
    if pack_changed || first {
        inner.index = None;
        inner.pack = inner.settings.pack.clone();
        match inner.pack.clone() {
            // Compiled in: ready at once, so the first frame already shows it.
            Some(name) if builtin::is_builtin(&name) => inner.index = Some(builtin_index()),
            Some(name) => {
                if let Some(root) = import::packs_dir() {
                    start_build(name, root);
                }
            }
            None => {}
        }
    }
}

/// The built-in pack's index, made once.
fn builtin_index() -> Arc<PackIndex> {
    static INDEX: OnceLock<Arc<PackIndex>> = OnceLock::new();
    INDEX.get_or_init(|| Arc::new(builtin::index())).clone()
}

/// Read a pack on its own thread, and hand the index to whoever asks next.
fn start_build(name: String, root: PathBuf) {
    std::thread::spawn(move || {
        let index = Arc::new(PackIndex::build(&root.join(&name), Some(&root)));
        let wakers = {
            let mut s = state().lock().unwrap();
            // The user may have picked another pack while this one was being read.
            if s.pack.as_deref() != Some(name.as_str()) {
                return;
            }
            s.index = Some(index);
            s.generation += 1;
            s.cache.clear();
            s.wakers.clone()
        };
        for wake in wakers {
            wake();
        }
    });
}

/// Called when icons may have changed (the pack arrived, the settings changed) from a background
/// thread: for requesting a repaint.
pub fn on_change(wake: impl Fn() + Send + Sync + 'static) {
    state().lock().unwrap().wakers.push(Arc::new(wake));
}

/// Counts up whenever what `resolve` answers may have changed. Looks at the settings file when it
/// has not for a moment, so calling it every frame is fine.
pub fn generation() -> u64 {
    let mut s = state().lock().unwrap();
    refresh(&mut s, false);
    s.generation
}

/// The settings as last read.
pub fn current_settings() -> IconSettings {
    let mut s = state().lock().unwrap();
    refresh(&mut s, false);
    s.settings.clone()
}

/// The keys an app's overrides and skips are looked up by: its id and its program's path.
fn keys(app: &AppRef) -> Vec<String> {
    let mut keys = Vec::new();
    if !app.id.is_empty() {
        keys.push(settings::key(app.id));
        // A Store app's package family (`Name_publisherid`), which is how Settings' Apps page
        // knows it, where everything else knows it by `Name_publisherid!App`.
        if let Some((family, _)) = app.id.split_once('!') {
            keys.push(settings::key(family));
        }
    }
    if let Some(exe) = app.exe {
        keys.push(settings::key(&exe.to_string_lossy()));
    }
    keys
}

/// The pack name of one of shellman's own apps (`shellman-files`), from its program's file name.
fn own_name(app: &AppRef) -> Option<String> {
    let name = |p: &str| p.rsplit(['\\', '/']).next().unwrap_or(p).to_lowercase();
    let own = |f: String| {
        let stem = f.strip_suffix(".exe")?;
        (stem.len() > "shellman-".len() && stem.starts_with("shellman-")).then(|| stem.to_string())
    };
    app.exe.and_then(|e| own(name(&e.to_string_lossy()))).or_else(|| own(name(app.id)))
}

#[cfg(test)]
fn is_shellmans_own(app: &AppRef) -> bool {
    own_name(app).is_some()
}

/// The image file to draw for the app instead of its own icon: its override, else the pack's, or
/// none (draw the app's own). Shellman's own apps are matched only by their exact names
/// (`shellman-files`), which only a pack made for shellman has; any other pack leaves them their
/// accent-coloured icons.
pub fn resolve(app: &AppRef) -> Option<PathBuf> {
    let keys = keys(app);
    if keys.is_empty() && app.name.is_none() {
        return None;
    }
    let memo = format!("{}|{}|{}", keys.join("|"), app.name.unwrap_or(""), app.exe.is_some());
    let (settings, index) = {
        let mut s = state().lock().unwrap();
        refresh(&mut s, false);
        if let Some(hit) = s.cache.get(&memo) {
            return hit.clone();
        }
        (s.settings.clone(), s.index.clone())
    };
    let answer = decide(&settings, index.as_deref(), app, &keys);
    // Not remembered while the pack is still being read: that answer is only for now.
    let waiting = settings.pack.is_some() && index.is_none();
    if !waiting {
        let mut s = state().lock().unwrap();
        s.cache.insert(memo, answer.clone());
    }
    answer
}

/// The order: a manual override, then the pack (unless the app is skipped), then nothing.
pub fn decide(settings: &IconSettings, index: Option<&PackIndex>, app: &AppRef, keys: &[String]) -> Option<PathBuf> {
    if let Some(image) = settings.override_for(keys) {
        let path = PathBuf::from(image);
        // A moved or deleted image: back to the app's own icon.
        return path.is_file().then_some(path);
    }
    if settings.is_skipped(keys) {
        return None;
    }
    let index = index?;
    if let Some(own) = own_name(app) {
        return index.get_exact(&own);
    }
    index.lookup(&names::candidates(app))
}

/// The pack's image for something that is not an app, by its icon name: `user-trash` and
/// `user-trash-full` for the Recycle Bin. None when the pack has no such icon (or is not read yet).
pub fn resolve_named(name: &str) -> Option<PathBuf> {
    let index = {
        let mut s = state().lock().unwrap();
        refresh(&mut s, false);
        s.index.clone()
    };
    index?.get_exact(name)
}

/// What a path handed to the icon code is, as far as matching goes: a program, a shortcut, or an
/// entry of the Apps folder. Anything else (a photo, a document) is not an app, unless the caller
/// says whose icon it is by `id` or `name`.
pub fn resolve_app(path: Option<&Path>, id: &str, name: &str) -> Option<PathBuf> {
    let text = path.map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let name = (!name.trim().is_empty()).then_some(name);
    let mut id = id.to_string();
    let mut exe = None;
    let mut name = name;
    let stem = path.and_then(|p| p.file_stem()).and_then(|s| s.to_str());
    const APPS_FOLDER: &str = "shell:appsfolder\\";
    if text.len() > APPS_FOLDER.len() && text[..APPS_FOLDER.len()].eq_ignore_ascii_case(APPS_FOLDER) {
        if id.is_empty() {
            id = text[APPS_FOLDER.len()..].to_string();
        }
    } else if let Some(p) = path {
        match p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
            Some("exe") => exe = Some(p),
            Some("lnk") => {
                if id.is_empty() {
                    id = text.clone();
                }
                name = name.or(stem);
            }
            _ => {}
        }
    }
    resolve(&AppRef { id: &id, exe, name })
}

/// `resolve_app` for a path alone.
pub fn resolve_path(path: &Path, name: Option<&str>) -> Option<PathBuf> {
    resolve_app(Some(path), "", name.unwrap_or(""))
}

/// How many of the launcher's apps the pack has an icon for, and how many there are.
pub fn coverage(index: &PackIndex, apps: &[crate::apps_index::App]) -> (usize, usize) {
    let listed: Vec<&crate::apps_index::App> = apps.iter().filter(|a| a.is_listed()).collect();
    let matched = listed
        .iter()
        .filter(|a| index.lookup(&names::candidates(&AppRef { id: &a.id, exe: None, name: Some(&a.name) })).is_some())
        .count();
    (matched, listed.len())
}

/// The pack for `name` as an index, read now (for Settings, which reads on its own thread).
pub fn read_pack(name: &str) -> PackIndex {
    if builtin::is_builtin(name) {
        return builtin::index();
    }
    match import::packs_dir() {
        Some(root) => PackIndex::build(&root.join(name), Some(&root)),
        None => PackIndex::default(),
    }
}

/// Names that show a pack off in Settings: popular apps, as the Windows names that map to them.
pub const SHOWCASE: [&str; 12] =
    ["firefox", "chrome", "code", "spotify", "discord", "steam", "vlc", "windowsterminal", "notepad", "explorer", "obs64", "7zfm"];

#[cfg(test)]
mod tests {
    use super::*;
    use index::tests_support::{temp, touch};

    fn pack() -> PackIndex {
        let dir = temp("decide");
        touch(&dir, "firefox.svg", "<svg/>");
        touch(&dir, "visual-studio-code.svg", "<svg/>");
        let index = PackIndex::build(&dir, None);
        let _ = std::fs::remove_dir_all(dir);
        index
    }

    fn app(exe: &str) -> AppRef<'_> {
        AppRef { id: "", exe: Some(Path::new(exe)), name: None }
    }

    #[test]
    fn a_pack_match_beats_the_real_icon() {
        let index = pack();
        let s = IconSettings { pack: Some("P".into()), ..Default::default() };
        let a = app(r"C:\x\firefox.exe");
        let got = decide(&s, Some(&index), &a, &keys(&a)).unwrap();
        assert!(got.ends_with("firefox.svg"));
        let none = app(r"C:\x\unknown.exe");
        assert_eq!(decide(&s, Some(&index), &none, &keys(&none)), None);
        // No pack, or one not read yet: the real icon.
        assert_eq!(decide(&IconSettings::default(), None, &a, &keys(&a)), None);
        assert_eq!(decide(&s, None, &a, &keys(&a)), None);
    }

    #[test]
    fn skip_means_the_real_icon_even_with_a_match() {
        let index = pack();
        let mut s = IconSettings { pack: Some("P".into()), ..Default::default() };
        let a = app(r"C:\x\firefox.exe");
        s.use_original(r"c:\x\firefox.exe");
        assert_eq!(decide(&s, Some(&index), &a, &keys(&a)), None);
    }

    #[test]
    fn an_override_beats_the_pack_and_a_skip() {
        let index = pack();
        let dir = temp("override");
        touch(&dir, "mine.png", "png");
        let mine = dir.join("mine.png");
        let mut s = IconSettings { pack: Some("P".into()), ..Default::default() };
        let a = app(r"C:\x\firefox.exe");
        s.use_original(r"c:\x\firefox.exe");
        s.set_override(r"C:\X\Firefox.exe", &mine.to_string_lossy());
        assert_eq!(decide(&s, Some(&index), &a, &keys(&a)), Some(mine.clone()));
        // Even with no pack at all.
        let only = IconSettings { overrides: s.overrides.clone(), ..Default::default() };
        assert_eq!(decide(&only, None, &a, &keys(&a)), Some(mine));
        // A deleted image is not drawn.
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(decide(&only, None, &a, &keys(&a)), None);
    }

    #[test]
    fn an_override_can_be_by_app_id() {
        let dir = temp("override-id");
        touch(&dir, "mine.png", "png");
        let mine = dir.join("mine.png");
        let mut s = IconSettings::default();
        s.set_override("Pkg_1!App", &mine.to_string_lossy());
        let a = AppRef { id: "pkg_1!app", exe: None, name: None };
        assert_eq!(decide(&s, None, &a, &keys(&a)), Some(mine));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn shellmans_own_apps_only_take_their_exact_names() {
        let own = AppRef { id: "", exe: Some(Path::new(r"C:\x\shellman-files.exe")), name: Some("Files") };
        let s = IconSettings::default();
        // A pack not made for shellman, with icons for the words in its apps' names: left alone.
        let dir = temp("own");
        touch(&dir, "files.svg", "<svg/>");
        touch(&dir, "folder.svg", "<svg/>");
        touch(&dir, "shellman.svg", "<svg/>");
        let other = PackIndex::build(&dir, None);
        assert_eq!(decide(&s, Some(&other), &own, &keys(&own)), None);
        // One that has it by name.
        touch(&dir, "shellman-files.svg", "<svg/>");
        let made = PackIndex::build(&dir, None);
        assert!(decide(&s, Some(&made), &own, &keys(&own)).unwrap().ends_with("shellman-files.svg"));
        let _ = std::fs::remove_dir_all(dir);
        // Islands has every one the dock and launcher show.
        let islands = builtin::index();
        for exe in ["shellman-files", "shellman-settings", "shellman-tasks", "shellman-terminal", "shellman-launcher", "shellman-control"] {
            let path = format!(r"C:\Programs\shellman\{exe}.exe");
            let a = AppRef { id: &path, exe: Some(Path::new(&path)), name: None };
            assert!(decide(&s, Some(&islands), &a, &keys(&a)).is_some(), "{exe}");
        }
        assert!(islands.get_exact("user-trash").is_some() && islands.get_exact("user-trash-full").is_some());
        assert!(is_shellmans_own(&AppRef { id: r"c:\x\shellman-bar.exe", exe: None, name: None }));
        assert!(!is_shellmans_own(&app(r"C:\x\firefox.exe")));
        assert!(!is_shellmans_own(&app(r"C:\x\shellman-.exe")));
    }

    #[test]
    fn only_apps_are_matched_by_path() {
        assert_eq!(resolve_path(Path::new(r"C:\photos\cat.jpg"), None), None);
        assert_eq!(resolve_path(Path::new(r"C:\docs\notes.txt"), None), None);
    }
}
