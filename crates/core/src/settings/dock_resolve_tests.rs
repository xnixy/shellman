//! Each kind of app the dock pins, against a made-up disk, Apps folder and Start menu shaped
//! like this machine's.

use super::*;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
struct Fake {
    files: HashSet<String>,
    dirs: HashSet<String>,
    texts: HashMap<String, String>,
    apps: HashMap<String, String>,
    shortcuts: Vec<Shortcut>,
    descriptions: HashMap<String, String>,
    steam: Option<PathBuf>,
}

fn key(p: &Path) -> String {
    p.to_string_lossy().to_lowercase()
}

impl Fake {
    fn file(mut self, p: &str) -> Self {
        let mut at = Path::new(p).parent();
        while let Some(d) = at.filter(|d| d.parent().is_some()) {
            self.dirs.insert(key(d));
            at = d.parent();
        }
        self.files.insert(p.to_lowercase());
        self
    }
    fn text(mut self, p: &str, t: &str) -> Self {
        self.texts.insert(p.to_lowercase(), t.to_owned());
        self.file(p)
    }
    fn app(mut self, id: &str, name: &str) -> Self {
        self.apps.insert(id.to_lowercase(), name.to_owned());
        self
    }
    fn desc(mut self, exe: &str, d: &str) -> Self {
        self.descriptions.insert(exe.to_lowercase(), d.to_owned());
        self
    }
    fn shortcut(mut self, s: Shortcut) -> Self {
        let p = s.path.to_string_lossy().into_owned();
        self.shortcuts.push(s);
        self.file(&p)
    }
    fn children(&self, dir: &Path, of: &HashSet<String>) -> Vec<String> {
        let d = format!("{}\\", key(dir));
        of.iter()
            .filter_map(|p| p.strip_prefix(&d))
            .filter(|rest| !rest.contains('\\'))
            .map(str::to_owned)
            .collect()
    }
}

impl World for Fake {
    fn exists(&self, p: &Path) -> bool {
        self.files.contains(&key(p)) || self.dirs.contains(&key(p))
    }
    fn is_dir(&self, p: &Path) -> bool {
        self.dirs.contains(&key(p))
    }
    fn subdirs(&self, dir: &Path) -> Vec<String> {
        self.children(dir, &self.dirs)
    }
    fn files(&self, dir: &Path) -> Vec<String> {
        self.children(dir, &self.files)
    }
    fn read_text(&self, p: &Path) -> Option<String> {
        self.texts.get(&key(p)).cloned()
    }
    fn apps_folder_name(&self, id: &str) -> Option<String> {
        self.apps.get(&id.to_lowercase()).cloned()
    }
    fn shortcuts(&self) -> Vec<Shortcut> {
        self.shortcuts.clone()
    }
    fn link(&self, p: &Path) -> Option<Shortcut> {
        self.shortcuts.iter().find(|s| key(&s.path) == key(p)).cloned()
    }
    fn description(&self, exe: &Path) -> Option<String> {
        self.descriptions.get(&key(exe)).cloned()
    }
    fn places(&self) -> Places {
        Places {
            broad: [r"C:\Program Files", r"C:\Program Files (x86)", r"C:\Users\X\AppData\Local", r"C:\Users\X\AppData\Local\Programs", r"C:\Users\X\AppData\Roaming", r"C:\Users\X"]
                .map(PathBuf::from)
                .to_vec(),
            system: vec![PathBuf::from(r"C:\Windows")],
        }
    }
    fn steam_root(&self) -> Option<PathBuf> {
        self.steam.clone()
    }
}

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

const STEAM: &str = r"C:\Program Files (x86)\Steam\steam.exe";
const HELPER: &str = r"C:\Program Files (x86)\Steam\bin\cef\cef.win64\steamwebhelper.exe";
const COD: &str = r"C:\Program Files (x86)\Steam\steamapps\common\Call of Duty HQ\cod.exe";
const START: &str = r"C:\Users\X\AppData\Roaming\Microsoft\Windows\Start Menu\Programs";

fn steam_machine() -> Fake {
    let manifest = "\"AppState\"\n{\n\t\"appid\"\t\t\"1938090\"\n\t\"name\"\t\t\"Call of Duty®\"\n\t\"installdir\"\t\t\"Call of Duty HQ\"\n}\n";
    let mut f = Fake::default()
        .file(STEAM)
        .file(HELPER)
        .file(COD)
        .file(r"C:\Program Files (x86)\Steam\steam\games\abc.ico")
        .text(r"C:\Program Files (x86)\Steam\steamapps\appmanifest_1938090.acf", manifest)
        .text(r"C:\Program Files (x86)\Steam\steamapps\libraryfolders.vdf", "\"libraryfolders\"\n{\n\"0\"\n{\n\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n}\n}\n")
        .desc(HELPER, "Steam Client WebHelper")
        .desc(STEAM, "Steam")
        .desc(COD, "Call of Duty")
        .shortcut(Shortcut { path: p(&format!(r"{START}\Steam\Steam.lnk")), name: "Steam".into(), target: Some(p(STEAM)), ..Default::default() })
        .shortcut(Shortcut {
            path: p(&format!(r"{START}\Steam\Call of Duty®.url")),
            name: "Call of Duty®".into(),
            url: Some("steam://rungameid/1938090".into()),
            icon: Some(p(r"C:\Program Files (x86)\Steam\steam\games\abc.ico")),
            ..Default::default()
        });
    f.steam = Some(p(r"C:\Program Files (x86)\Steam"));
    f
}

fn steam_relaunch() -> Relaunch {
    Relaunch { command: STEAM.into(), name: Some("Steam".into()), icon: None }
}

#[test]
fn steam_pinned_from_its_window_is_steam() {
    let w = steam_machine();
    // What the dock saved before: the helper that runs the window.
    let old = Pin::new("Valve.Steam.Client", Some(p(HELPER)));
    let pin = resolve(&old, Some(&steam_relaunch()), &w);
    assert_eq!(pin.name.as_deref(), Some("Steam"));
    assert_eq!(pin.launch, Some(Launch::Exe { path: p(STEAM), args: String::new() }));
    assert_eq!(pin.icon, Some(p(STEAM)));
    assert_eq!(pin.app_id, "Valve.Steam.Client");
    // An old pin upgraded while Steam is not running: the Start menu's Steam, above the helper.
    let lazy = resolve(&old, None, &w);
    assert_eq!(lazy.name.as_deref(), Some("Steam"));
    assert_eq!(lazy.launch, Some(Launch::Open { path: p(&format!(r"{START}\Steam\Steam.lnk")) }));
    assert_eq!(lazy.icon, Some(p(&format!(r"{START}\Steam\Steam.lnk"))));
}

#[test]
fn a_steam_game_starts_through_steam() {
    let w = steam_machine();
    let from_window = resolve(&Pin::new(COD.to_lowercase(), Some(p(COD))), None, &w);
    assert_eq!(from_window.launch, Some(Launch::Url { url: "steam://rungameid/1938090".into() }));
    assert_eq!(from_window.name.as_deref(), Some("Call of Duty®"));
    assert_eq!(from_window.icon, Some(p(r"C:\Program Files (x86)\Steam\steam\games\abc.ico")));
    assert_eq!(from_window.home, Some(p(r"C:\Program Files (x86)\Steam\steamapps\common\Call of Duty HQ")));
    // Pinned by dropping Steam's internet shortcut: the same game, and its windows find it.
    let url = format!(r"{START}\Steam\Call of Duty®.url");
    let dropped = resolve(&Pin::new(url.to_lowercase(), Some(p(&url))), None, &w);
    assert_eq!(dropped.launch, from_window.launch);
    assert_eq!(dropped.home, from_window.home);
    assert_eq!(dropped.name.as_deref(), Some("Call of Duty®"));
    assert_eq!(crate::settings::dock::match_rank(&dropped, &COD.to_lowercase(), Some(Path::new(COD)), None), Some(4));
}

#[test]
fn discord_starts_through_squirrel_and_follows_updates() {
    let root = r"C:\Users\X\AppData\Local\Discord";
    let old_exe = format!(r"{root}\app-1.0.9268\Discord.exe");
    let new_exe = format!(r"{root}\app-1.0.9300\Discord.exe");
    let w = Fake::default()
        .file(&format!(r"{root}\Update.exe"))
        .file(&format!(r"{root}\app-1.0.10\Discord.exe"))
        .file(&new_exe)
        .file(&format!(r"{root}\app-1.0.9999\other.txt"))
        .desc(&new_exe, "Discord")
        .app("com.squirrel.Discord.Discord", "Discord");
    // The user's saved pin, after Discord updated past it.
    let saved = Pin::new(old_exe.to_lowercase(), Some(p(&old_exe)));
    let pin = resolve(&saved, None, &w);
    assert_eq!(pin.exe, Some(p(&new_exe)));
    assert_eq!(pin.app_id, new_exe.to_lowercase());
    assert_eq!(pin.launch, Some(Launch::Exe { path: p(&format!(r"{root}\Update.exe")), args: "--processStart Discord.exe".into() }));
    assert_eq!(pin.home, Some(p(root)));
    assert_eq!(pin.name.as_deref(), Some("Discord"));
    assert_eq!(pin.icon, Some(p(&new_exe)));
    assert!(!is_gone(&pin, &w));
}

#[test]
fn store_apps_start_through_the_apps_folder() {
    let id = "Agilebits.1Password_amwd9z03whsfe!Agilebits.OnePassword";
    let exe = r"C:\Program Files\WindowsApps\Agilebits.1Password_8.10.0.0_x64__amwd9z03whsfe\1Password.exe";
    let w = Fake::default().file(exe).app(id, "1Password");
    let pin = resolve(&Pin::new(id, Some(p(exe))), None, &w);
    assert_eq!(pin.launch, Some(Launch::AppsFolder { id: id.into() }));
    assert_eq!(pin.name.as_deref(), Some("1Password"));
    assert_eq!(pin.icon, Some(p(exe)));
    // A UWP app seen through its frame host: its icon is its own, not the host's.
    let calc = "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App";
    let hosted = resolve(&Pin::new(calc, Some(p(r"C:\Windows\System32\ApplicationFrameHost.exe"))), None, &w);
    assert_eq!(hosted.icon, Some(p(&format!(r"shell:AppsFolder\{calc}"))));
    assert_eq!(hosted.launch, Some(Launch::AppsFolder { id: calc.into() }));
}

#[test]
fn apps_folder_ids_and_web_apps() {
    let chrome = r"C:\Program Files\Google\Chrome\Application\chrome.exe";
    let w = Fake::default()
        .file(chrome)
        .file(r"C:\Program Files\Google\Chrome\Application\chrome_proxy.exe")
        .file(r"C:\Program Files\Mozilla Firefox\firefox.exe")
        .app("308046B0AF4A39CB", "Firefox")
        .app("Chrome._crx_abc", "YouTube Music");
    let ff = resolve(&Pin::new("308046B0AF4A39CB", Some(p(r"C:\Program Files\Mozilla Firefox\firefox.exe"))), None, &w);
    assert_eq!(ff.launch, Some(Launch::AppsFolder { id: "308046B0AF4A39CB".into() }));
    assert_eq!(ff.name.as_deref(), Some("Firefox"));
    // A web app's window: the browser's program, its own id and relaunch command.
    let relaunch = Relaunch {
        command: r#""C:\Program Files\Google\Chrome\Application\chrome_proxy.exe" --profile-directory=Default --app-id=abc"#.into(),
        name: Some("YouTube Music".into()),
        icon: Some(r"C:\Users\X\AppData\Local\Google\Chrome\User Data\Default\Web Applications\abc\icon.ico,0".into()),
    };
    let pwa = resolve(&Pin::new("Chrome._crx_abc", Some(p(chrome))), Some(&relaunch), &w);
    assert_eq!(
        pwa.launch,
        Some(Launch::Exe { path: p(r"C:\Program Files\Google\Chrome\Application\chrome_proxy.exe"), args: "--profile-directory=Default --app-id=abc".into() })
    );
    assert_eq!(pwa.name.as_deref(), Some("YouTube Music"));
    // Its icon file is not there: the program's.
    assert_eq!(pwa.icon, Some(p(r"C:\Program Files\Google\Chrome\Application\chrome_proxy.exe")));
    // Without the window: its Apps folder entry.
    let lazy = resolve(&Pin::new("Chrome._crx_abc", Some(p(chrome))), None, &w);
    assert_eq!(lazy.launch, Some(Launch::AppsFolder { id: "Chrome._crx_abc".into() }));
}

#[test]
fn relaunch_commands_are_only_run_when_they_are_the_apps_own() {
    let w = steam_machine();
    let places = w.places();
    let helper = Some(Path::new(HELPER));
    let ok = |prog: &str, exe: Option<&Path>, id: &str| relaunch_allowed(Path::new(prog), exe, id, &w.shortcuts(), &places);
    assert!(ok(STEAM, helper, "Valve.Steam.Client"));
    assert!(ok(HELPER, helper, "Valve.Steam.Client"));
    // Windows' own programs, and programs anywhere else, never.
    assert!(!ok(r"C:\Windows\System32\cmd.exe", helper, "x"));
    assert!(!ok(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe", Some(Path::new(r"C:\Windows\explorer.exe")), "x"));
    assert!(!ok(r"C:\Users\X\Downloads\evil.exe", helper, "Valve.Steam.Client"));
    // A sibling app in Program Files is not the app's own folder.
    assert!(!ok(r"C:\Program Files (x86)\Other\other.exe", helper, "x"));
    assert!(!ok(r"C:\Program Files (x86)\evil.exe", helper, "x"));
    assert!(!ok(r"C:\evil.exe", Some(Path::new(r"C:\a\b.exe")), "x"));
    assert!(!ok(r"C:\Users\X\AppData\Local\evil.exe", Some(Path::new(r"C:\Users\X\AppData\Local\Discord\app-1\Discord.exe")), "x"));
    // Not a program, or not a full path.
    assert!(!ok(r"C:\Program Files (x86)\Steam\run.bat", helper, "x"));
    assert!(!ok("steam.exe", helper, "x"));
    // Its own Start menu shortcut's program, wherever that is.
    let lnk = Shortcut { path: p(r"C:\s.lnk"), aumid: Some("Some.App".into()), target: Some(p(r"D:\Apps\some.exe")), ..Default::default() };
    assert!(relaunch_allowed(Path::new(r"D:\Apps\some.exe"), None, "Some.App", &[lnk.clone()], &places));
    assert!(!relaunch_allowed(Path::new(r"D:\Apps\some.exe"), None, "Other.App", &[lnk], &places));
    // Its own Store package.
    let id = "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App";
    let own = r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.21.0.0_x64__8wekyb3d8bbwe\wt.exe";
    let other = r"C:\Program Files\WindowsApps\Evil.App_1.0.0.0_x64__8wekyb3d8bbwe\wt.exe";
    assert!(relaunch_allowed(Path::new(own), None, id, &[], &places));
    assert!(!relaunch_allowed(Path::new(other), None, id, &[], &places));
    // A planted command is ignored: the pin starts the app another way.
    let planted = Relaunch { command: r"C:\Windows\System32\cmd.exe /c del everything".into(), name: Some("Steam".into()), icon: None };
    let pin = resolve(&Pin::new("Valve.Steam.Client", Some(p(HELPER))), Some(&planted), &w);
    assert_eq!(pin.launch, Some(Launch::Open { path: p(&format!(r"{START}\Steam\Steam.lnk")) }));
}

#[test]
fn commands_split_into_program_and_arguments() {
    assert_eq!(split_command(STEAM), Some((p(STEAM), String::new())));
    assert_eq!(split_command(&format!("{STEAM} -silent")), Some((p(STEAM), "-silent".into())));
    assert_eq!(split_command(r#""C:\A B\x.exe" --a "b c""#), Some((p(r"C:\A B\x.exe"), r#"--a "b c""#.into())));
    assert_eq!(split_command(r"C:\a.exec\b.exe x"), Some((p(r"C:\a.exec\b.exe"), "x".into())));
    assert_eq!(split_command("   "), None);
}

#[test]
fn dropped_files_open_as_themselves() {
    let w = Fake::default()
        .file(r"C:\Work\v1.2\notes.txt")
        .file(r"C:\Users\X\Desktop\Report.docx")
        .file(r"C:\Users\X\AppData\Local\Discord\Update.exe")
        .shortcut(Shortcut {
            path: p(r"C:\Users\X\Desktop\Discord.lnk"),
            name: "Discord".into(),
            target: Some(p(r"C:\Users\X\AppData\Local\Discord\Update.exe")),
            args: "--processStart Discord.exe".into(),
            ..Default::default()
        });
    let folder = resolve(&Pin::new(r"c:\work\v1.2", Some(p(r"C:\Work\v1.2"))), None, &w);
    assert_eq!(folder.name.as_deref(), Some("v1.2"));
    assert_eq!(folder.launch, Some(Launch::Open { path: p(r"C:\Work\v1.2") }));
    assert_eq!(folder.icon, Some(p(r"C:\Work\v1.2")));
    let doc = resolve(&Pin::new(r"c:\users\x\desktop\report.docx", Some(p(r"C:\Users\X\Desktop\Report.docx"))), None, &w);
    assert_eq!(doc.name.as_deref(), Some("Report.docx"));
    // A shortcut with arguments is kept as the shortcut, and finds its Squirrel app's windows.
    let lnk = p(r"C:\Users\X\Desktop\Discord.lnk");
    let pin = Pin { launch: Some(Launch::Open { path: lnk.clone() }), ..Pin::new(r"c:\users\x\desktop\discord.lnk", Some(lnk.clone())) };
    let pin = resolve(&pin, None, &w);
    assert_eq!(pin.launch, Some(Launch::Open { path: lnk.clone() }));
    assert_eq!(pin.icon, Some(lnk));
    assert_eq!(pin.name.as_deref(), Some("Discord"));
    assert_eq!(pin.home, Some(p(r"C:\Users\X\AppData\Local\Discord")));
}

#[test]
fn programs_and_gone_programs() {
    let term = r"C:\Users\X\AppData\Local\Programs\shellman\shellman-terminal.exe";
    let w = Fake::default().file(term).file(r"D:\Apps\coolwave.exe").desc(r"D:\Apps\coolwave.exe", "Coolwave");
    let own = resolve(&Pin::new(term.to_lowercase(), Some(p(term))), None, &w);
    assert_eq!(own.launch, Some(Launch::Exe { path: p(term), args: String::new() }));
    // shellman's own apps are named by the bar.
    assert_eq!(own.name, None);
    // Pinned while an update had it renamed aside.
    let aside = r"C:\Users\X\AppData\Local\Programs\shellman\shellman-terminal.old205309.exe";
    let fixed = resolve(&Pin::new(aside.to_lowercase(), Some(p(aside))), None, &w);
    assert_eq!(fixed.exe, Some(p(term)));
    assert_eq!(fixed.app_id, term.to_lowercase());
    let cool = resolve(&Pin::new(r"d:\apps\coolwave.exe", Some(p(r"D:\Apps\coolwave.exe"))), None, &w);
    assert_eq!((cool.name.as_deref(), cool.icon.clone()), (Some("Coolwave"), Some(p(r"D:\Apps\coolwave.exe"))));
    // Uninstalled: nothing to start, and it says so.
    let gone = resolve(&Pin::new(r"d:\gone\x.exe", Some(p(r"D:\Gone\x.exe"))), None, &w);
    assert_eq!(gone.launch, None);
    assert!(is_gone(&gone, &w));
    // A saved launch whose program went away is worked out again.
    let moved = Pin { launch: Some(Launch::Exe { path: p(r"D:\Old\coolwave.exe"), args: String::new() }), icon: Some(p(r"D:\Old\coolwave.exe")), ..Pin::new(r"d:\apps\coolwave.exe", Some(p(r"D:\Apps\coolwave.exe"))) };
    let again = resolve(&moved, None, &w);
    assert_eq!(again.launch, Some(Launch::Exe { path: p(r"D:\Apps\coolwave.exe"), args: String::new() }));
    assert_eq!(again.icon, Some(p(r"D:\Apps\coolwave.exe")));
}

/// Seen on this machine: plain `notepad.exe` was taken for the Windows SDK, whose Start menu
/// shortcut starts a program in `C:\Windows`, a folder above `System32`.
#[test]
fn programs_are_never_taken_for_a_shortcut_in_a_folder_above_them() {
    let notepad = r"C:\Windows\System32\notepad.exe";
    let sdk = Shortcut { path: p(&format!(r"{START}\Windows Kits\SDK.lnk")), name: "Windows Software Development Kit".into(), target: Some(p(r"C:\Windows\explorer.exe")), ..Default::default() };
    let tools = Shortcut { path: p(&format!(r"{START}\Tools.lnk")), name: "Tools".into(), target: Some(p(r"D:\Tools\menu.exe")), ..Default::default() };
    let w = Fake::default().file(notepad).file(r"C:\Windows\explorer.exe").file(r"D:\Tools\menu.exe").file(r"D:\Tools\sub\cut.exe").desc(notepad, "Notepad").shortcut(sdk).shortcut(tools);
    let n = resolve(&Pin::new(notepad.to_lowercase(), Some(p(notepad))), None, &w);
    assert_eq!(n.launch, Some(Launch::Exe { path: p(notepad), args: String::new() }));
    assert_eq!(n.name.as_deref(), Some("Notepad"));
    // A program pinned by its path is itself, whatever shortcut sits in a folder above.
    let cut = resolve(&Pin::new(r"d:\tools\sub\cut.exe", Some(p(r"D:\Tools\sub\cut.exe"))), None, &w);
    assert_eq!(cut.launch, Some(Launch::Exe { path: p(r"D:\Tools\sub\cut.exe"), args: String::new() }));
    // An app id declared by a window of a program in Windows' folder: no guess either.
    let odd = resolve(&Pin::new("Some.App", Some(p(notepad))), None, &w);
    assert_ne!(odd.launch, Some(Launch::Open { path: p(&format!(r"{START}\Windows Kits\SDK.lnk")) }));
}

/// Seen on this machine: `explorer.exe` was taken for the Windows SDK, whose shortcut starts
/// `explorer.exe "<the SDK's folder>"`. A shortcut with arguments is not the program.
#[test]
fn a_shortcut_with_arguments_is_not_its_program() {
    let explorer = r"C:\Windows\explorer.exe";
    let tool = r"D:\Apps\tool.exe";
    let with_args = |name: &str, target: &str, args: &str| Shortcut {
        path: p(&format!(r"{START}\{name}.lnk")),
        name: name.into(),
        target: Some(p(target)),
        args: args.into(),
        ..Default::default()
    };
    let w = Fake::default()
        .file(explorer)
        .file(tool)
        .desc(explorer, "Windows Explorer")
        .shortcut(with_args("SDK", explorer, r#""C:\Program Files (x86)\Windows Kits\10\""#))
        .shortcut(with_args("Tool safe mode", tool, "--safe"))
        .shortcut(with_args("Tool", tool, ""));
    let e = resolve(&Pin::new(explorer.to_lowercase(), Some(p(explorer))), None, &w);
    assert_eq!(e.launch, Some(Launch::Exe { path: p(explorer), args: String::new() }));
    // The plain shortcut, whichever comes first.
    let t = resolve(&Pin::new(tool.to_lowercase(), Some(p(tool))), None, &w);
    assert_eq!(t.launch, Some(Launch::Open { path: p(&format!(r"{START}\Tool.lnk")) }));
}

/// The bar saves a pin whenever working it out changes it, so working out a worked-out pin must
/// change nothing.
#[test]
fn working_a_pin_out_twice_changes_nothing() {
    let w = steam_machine();
    let pins = [
        (Pin::new("Valve.Steam.Client", Some(p(HELPER))), Some(steam_relaunch())),
        (Pin::new("Valve.Steam.Client", Some(p(HELPER))), None),
        (Pin::new(COD.to_lowercase(), Some(p(COD))), None),
        (Pin::new(STEAM.to_lowercase(), Some(p(STEAM))), None),
        (Pin::new(r"c:\gone\x.exe", Some(p(r"C:\Gone\x.exe"))), None),
    ];
    for (pin, relaunch) in pins {
        let once = resolve(&pin, relaunch.as_ref(), &w);
        assert_eq!(resolve(&once, relaunch.as_ref(), &w), once, "{}", pin.app_id);
        assert_eq!(resolve(&once, None, &w), once, "{} with its window gone", pin.app_id);
    }
}

/// Steam pinned by an older bar (its id and helper program only), with Steam's window closed:
/// the Start menu's Steam shortcut, a folder above the helper.
#[test]
fn an_old_steam_pin_with_no_window_finds_steams_shortcut() {
    let old = resolve(&Pin::new("Valve.Steam.Client", Some(p(HELPER))), None, &steam_machine());
    assert_eq!(old.launch, Some(Launch::Open { path: p(&format!(r"{START}\Steam\Steam.lnk")) }));
    assert_eq!(old.name.as_deref(), Some("Steam"));
}

#[test]
fn a_resolved_pin_is_left_as_it_is() {
    let w = steam_machine();
    let pin = resolve(&Pin::new("Valve.Steam.Client", Some(p(HELPER))), Some(&steam_relaunch()), &w);
    assert_eq!(resolve(&pin, None, &w), pin);
}

#[test]
fn steam_ids_from_urls() {
    assert_eq!(steam_id("steam://rungameid/1938090"), Some(1938090));
    assert_eq!(steam_id("STEAM://RUN/570/"), Some(570));
    assert_eq!(steam_id("https://x"), None);
}
