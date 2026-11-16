//! What the helper may be asked to do. A fixed list, parsed strictly from the command line:
//! no paths, commands or registry keys are ever taken from it. To add an operation: one variant
//! here, its words in `parse` and `to_args`, and one arm in the helper's handler.

use crate::startup::approved::Area;

/// One thing the helper will do as administrator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Turn a sound device on or off (`sound-device <enable|disable> <endpoint-id>`).
    SoundDevice { enable: bool, id: String },
    /// Record a machine-wide startup item as on or off, in
    /// `HKLM\...\Explorer\StartupApproved\<Run|Run32|StartupFolder>`
    /// (`startup-approve <enable|disable> <run|run32|folder> <name>`). Only that one value.
    StartupApprove { enable: bool, area: Area, name: String },
    /// Install the system font replacements built for one generation and point the Segoe UI
    /// entries at them (`sysfont-apply <generation>`). The helper reads the plan from its own
    /// fixed place and checks it; only the generation comes from here.
    SysfontApply { generation: String },
    /// Point the Segoe UI entries back at Windows' own files (`sysfont-restore`).
    SysfontRestore,
    /// Put a system font used before back, from its history folder (`sysfont-reuse <generation>`).
    SysfontReuse { generation: String },
    /// End processes, in this order, each named by PID and start time (`end-processes
    /// <pid>:<created> ...`); one whose start time differs is another process on a reused PID and
    /// is left alone. Never a name or a command line.
    EndProcesses { targets: Vec<(u32, i64)> },
    /// Start a Store app, which then runs as administrator when it is a desktop program in a
    /// package (`launch-app <Family>!<App>`). Only an app id of an installed package: never a
    /// path, a command line or arguments.
    LaunchApp { id: String },
}

/// The most processes one `end-processes` may name (a big browser's tree is well under this).
pub const MAX_TARGETS: usize = 256;

/// `pid:created` as written on the command line: decimal digits only, a PID above 4 and a start
/// time above zero.
fn parse_target(word: &str) -> Option<(u32, i64)> {
    let (pid, created) = word.split_once(':')?;
    let digits = |s: &str, max: usize| !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(pid, 10) || !digits(created, 19) {
        return None;
    }
    let (pid, created) = (pid.parse::<u32>().ok()?, created.parse::<i64>().ok()?);
    (pid > 4 && created > 0).then_some((pid, created))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadArgs(pub &'static str);

/// Whether `s` is exactly an MMDevice endpoint id: `{0.0.N.00000000}.{guid}` with N 0 (output)
/// or 1 (input) and the guid as 8-4-4-4-12 hex digits.
pub fn is_endpoint_id(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("{0.0.") else { return false };
    let mut chars = rest.chars();
    if !matches!(chars.next(), Some('0' | '1')) {
        return false;
    }
    let Some(rest) = chars.as_str().strip_prefix(".00000000}.{") else { return false };
    let Some(guid) = rest.strip_suffix('}') else { return false };
    let groups: Vec<&str> = guid.split('-').collect();
    groups.len() == 5
        && groups.iter().zip([8, 4, 4, 4, 12]).all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Whether `s` can be the name of a startup item: a Run value's name or a Startup folder file's
/// name. Not empty, at most 256 characters, no leading or trailing space, and no backslash,
/// slash, quote or control character, so it is one plain registry value name and never a path.
pub fn is_item_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars().count() <= 256
        && s.trim() == s
        && !s.chars().any(|c| c.is_control() || matches!(c, '\\' | '/' | '"'))
}

/// Whether `s` is a system font generation id: base-36 time, lowercase letters and digits, at
/// most 13 of them. Never a path.
pub fn is_generation(s: &str) -> bool {
    !s.is_empty() && s.len() <= 13 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Whether `s` is spelled like a Store app's id, `<Name>_<PublisherId>!<App>`: a package name of
/// letters, digits, dots and dashes, the 13-character publisher id, and an application id of
/// letters, digits and dots. Never a path or anything with a space.
pub fn is_app_id(s: &str) -> bool {
    let Some((family, app)) = s.split_once('!') else { return false };
    let Some((name, publisher)) = family.split_once('_') else { return false };
    let word = |w: &str, max: usize, extra: &[u8]| {
        !w.is_empty() && w.len() <= max && w.bytes().all(|b| b.is_ascii_alphanumeric() || extra.contains(&b))
    };
    word(name, 50, b".-") && publisher.len() == 13 && word(publisher, 13, b"") && word(app, 64, b".")
}

fn state_word(state: &str) -> Result<bool, BadArgs> {
    match state {
        "enable" => Ok(true),
        "disable" => Ok(false),
        _ => Err(BadArgs("expected enable or disable")),
    }
}

/// The operation a command line (without the program name) asks for.
pub fn parse(args: &[String]) -> Result<Op, BadArgs> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["sound-device", state, id] => {
            let enable = state_word(state)?;
            if !is_endpoint_id(id) {
                return Err(BadArgs("not an audio endpoint id"));
            }
            Ok(Op::SoundDevice { enable, id: (*id).to_owned() })
        }
        ["startup-approve", state, area, name] => {
            let enable = state_word(state)?;
            let area = Area::from_word(area).ok_or(BadArgs("area must be run, run32 or folder"))?;
            if !is_item_name(name) {
                return Err(BadArgs("not a startup item name"));
            }
            Ok(Op::StartupApprove { enable, area, name: (*name).to_owned() })
        }
        ["sysfont-apply", generation] if is_generation(generation) => Ok(Op::SysfontApply { generation: (*generation).to_owned() }),
        ["sysfont-reuse", generation] if is_generation(generation) => Ok(Op::SysfontReuse { generation: (*generation).to_owned() }),
        ["sysfont-restore"] => Ok(Op::SysfontRestore),
        ["end-processes", targets @ ..] if (1..=MAX_TARGETS).contains(&targets.len()) => {
            let parsed: Option<Vec<_>> = targets.iter().map(|t| parse_target(t)).collect();
            parsed.map(|targets| Op::EndProcesses { targets }).ok_or(BadArgs("a target is <pid>:<start time>"))
        }
        ["launch-app", id] if is_app_id(id) => Ok(Op::LaunchApp { id: (*id).to_owned() }),
        ["launch-app", ..] => Err(BadArgs("launch-app takes exactly one Store app id")),
        ["end-processes", ..] => Err(BadArgs("end-processes takes 1 to 256 targets")),
        ["sysfont-apply" | "sysfont-reuse", ..] => Err(BadArgs("takes exactly one generation id")),
        ["sysfont-restore", ..] => Err(BadArgs("sysfont-restore takes nothing")),
        ["startup-approve", ..] => Err(BadArgs("startup-approve takes exactly a state, an area and a name")),
        ["sound-device", ..] => Err(BadArgs("sound-device takes exactly a state and an endpoint id")),
        [] => Err(BadArgs("no operation")),
        _ => Err(BadArgs("unknown operation")),
    }
}

impl Op {
    /// The command-line words for this operation; `parse` of them gives it back. Every word is
    /// validated, so none needs quoting.
    pub fn to_args(&self) -> Vec<String> {
        match self {
            Op::SoundDevice { enable, id } => {
                vec!["sound-device".into(), if *enable { "enable" } else { "disable" }.into(), id.clone()]
            }
            Op::StartupApprove { enable, area, name } => {
                vec!["startup-approve".into(), if *enable { "enable" } else { "disable" }.into(), area.word().into(), name.clone()]
            }
            Op::SysfontApply { generation } => vec!["sysfont-apply".into(), generation.clone()],
            Op::SysfontRestore => vec!["sysfont-restore".into()],
            Op::SysfontReuse { generation } => vec!["sysfont-reuse".into(), generation.clone()],
            Op::EndProcesses { targets } => {
                let mut v = vec!["end-processes".to_owned()];
                v.extend(targets.iter().map(|(pid, created)| format!("{pid}:{created}")));
                v
            }
            Op::LaunchApp { id } => vec!["launch-app".into(), id.clone()],
        }
    }

    /// `to_args` as one command line. A word with a space (an item name) is quoted; no word can
    /// contain a quote or a backslash, so that is all the quoting there is.
    pub fn command_line(&self) -> String {
        let quote = |w: String| if w.contains(' ') { format!("\"{w}\"") } else { w };
        self.to_args().into_iter().map(quote).collect::<Vec<_>>().join(" ")
    }

    /// The operation for a startup item, or None when `name` is not a plain item name.
    pub fn startup_approve(enable: bool, area: Area, name: &str) -> Option<Op> {
        is_item_name(name).then(|| Op::StartupApprove { enable, area, name: name.to_owned() })
    }

    /// The operation that installs a built system font, or None when `generation` is not an id.
    pub fn sysfont_apply(generation: &str) -> Option<Op> {
        is_generation(generation).then(|| Op::SysfontApply { generation: generation.to_owned() })
    }

    /// The operation that puts a system font used before back, or None when not an id.
    pub fn sysfont_reuse(generation: &str) -> Option<Op> {
        is_generation(generation).then(|| Op::SysfontReuse { generation: generation.to_owned() })
    }

    /// The operation that ends these (PID, start time) processes in order, or None when there are
    /// none, too many, or one is not an ordinary process (PID 4 or below, no start time).
    pub fn end_processes(targets: &[(u32, i64)]) -> Option<Op> {
        let ok = (1..=MAX_TARGETS).contains(&targets.len()) && targets.iter().all(|&(pid, created)| pid > 4 && created > 0);
        ok.then(|| Op::EndProcesses { targets: targets.to_vec() })
    }

    /// The operation that starts a Store app, or None when `id` is not spelled like one.
    pub fn launch_app(id: &str) -> Option<Op> {
        is_app_id(id).then(|| Op::LaunchApp { id: id.to_owned() })
    }

    /// The operation for a sound device, or None when `id` is not an endpoint id.
    pub fn sound_device(enable: bool, id: &str) -> Option<Op> {
        is_endpoint_id(id).then(|| Op::SoundDevice { enable, id: id.to_owned() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "{0.0.0.00000000}.{8a4f1c2e-0b3d-4e5f-9a6b-7c8d9e0f1a2b}";

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_known_shapes_are_endpoint_ids() {
        assert!(is_endpoint_id(ID));
        assert!(is_endpoint_id("{0.0.1.00000000}.{8A4F1C2E-0B3D-4E5F-9A6B-7C8D9E0F1A2B}"));
    }

    #[test]
    fn anything_else_is_not() {
        for bad in [
            "",
            "{0.0.2.00000000}.{8a4f1c2e-0b3d-4e5f-9a6b-7c8d9e0f1a2b}",
            "{0.0.0.00000001}.{8a4f1c2e-0b3d-4e5f-9a6b-7c8d9e0f1a2b}",
            "{0.0.0.00000000}.{8a4f1c2e-0b3d-4e5f-9a6b-7c8d9e0f1a2}",
            "{0.0.0.00000000}.{8a4f1c2e-0b3d-4e5f-9a6b-7c8d9e0f1a2g}",
            "{0.0.0.00000000}.{8a4f1c2e0b3d4e5f9a6b7c8d9e0f1a2b}",
            "{0.0.0.00000000}.{8a4f1c2e-0b3d-4e5f-9a6b-7c8d9e0f1a2b} ",
            "{0.0.0.00000000}.{8a4f1c2e-0b3d-4e5f-9a6b-7c8d9e0f1a2b}x",
            "{0.0.00.00000000}.{8a4f1c2e-0b3d-4e5f-9a6b-7c8d9e0f1a2b}",
            "C:\\Windows\\notepad.exe",
            "HKLM\\SOFTWARE\\x",
        ] {
            assert!(!is_endpoint_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn sound_device_parses_strictly() {
        assert_eq!(parse(&args(&["sound-device", "enable", ID])), Ok(Op::SoundDevice { enable: true, id: ID.into() }));
        assert_eq!(parse(&args(&["sound-device", "disable", ID])), Ok(Op::SoundDevice { enable: false, id: ID.into() }));
        assert!(parse(&args(&["sound-device", "toggle", ID])).is_err());
        assert!(parse(&args(&["sound-device", "Enable", ID])).is_err());
        assert!(parse(&args(&["sound-device", "enable", "x"])).is_err());
        assert!(parse(&args(&["sound-device", "enable"])).is_err());
        assert!(parse(&args(&["sound-device", "enable", ID, "extra"])).is_err());
        assert!(parse(&args(&["sound-device"])).is_err());
        assert!(parse(&args(&[])).is_err());
        assert!(parse(&args(&["run", "cmd.exe"])).is_err());
        assert!(parse(&args(&["--sound-device", "enable", ID])).is_err());
    }

    #[test]
    fn startup_approve_parses_strictly() {
        let ok = |a: &[&str]| parse(&args(a));
        assert_eq!(
            ok(&["startup-approve", "disable", "run32", "Microsoft Teams"]),
            Ok(Op::StartupApprove { enable: false, area: Area::Run32, name: "Microsoft Teams".into() })
        );
        assert!(ok(&["startup-approve", "enable", "folder", "a.lnk"]).is_ok());
        for bad in [
            &["startup-approve", "enable", "hklm", "x"][..],
            &["startup-approve", "enable", "Run", "x"],
            &["startup-approve", "toggle", "run", "x"],
            &["startup-approve", "enable", "run"],
            &["startup-approve", "enable", "run", "x", "y"],
            &["startup-approve", "enable", "run", ""],
            &["startup-approve", "enable", "run", "a\\b"],
            &["startup-approve", "enable", "run", "..\\x"],
            &["startup-approve", "enable", "run", "a/b"],
            &["startup-approve", "enable", "run", "a\"b"],
            &["startup-approve", "enable", "run", "a\nb"],
            &["startup-approve", "enable", "run", " lead"],
            &["startup-approve", "enable", "run", "trail "],
        ] {
            assert!(ok(bad).is_err(), "{bad:?}");
        }
        assert!(ok(&["startup-approve", "enable", "run", &"x".repeat(256)]).is_ok());
        assert!(ok(&["startup-approve", "enable", "run", &"x".repeat(257)]).is_err());
    }

    #[test]
    fn startup_approve_round_trips_and_quotes_spaces() {
        for area in Area::ALL {
            for enable in [true, false] {
                let op = Op::StartupApprove { enable, area, name: "OneDrive Sync".into() };
                assert_eq!(parse(&op.to_args()), Ok(op.clone()));
            }
        }
        let op = Op::startup_approve(false, Area::Run, "Microsoft Teams").unwrap();
        assert_eq!(op.command_line(), "startup-approve disable run \"Microsoft Teams\"");
        assert_eq!(Op::SoundDevice { enable: true, id: ID.into() }.command_line(), format!("sound-device enable {ID}"));
        assert!(Op::startup_approve(true, Area::Run, "a\\b").is_none());
    }

    #[test]
    fn sysfont_ops_parse_strictly() {
        let ok = |a: &[&str]| parse(&args(a));
        assert_eq!(ok(&["sysfont-apply", "tob9y0"]), Ok(Op::SysfontApply { generation: "tob9y0".into() }));
        assert_eq!(ok(&["sysfont-reuse", "t0b9y0"]), Ok(Op::SysfontReuse { generation: "t0b9y0".into() }));
        assert_eq!(ok(&["sysfont-restore"]), Ok(Op::SysfontRestore));
        for bad in [
            &["sysfont-apply"][..],
            &["sysfont-apply", ""],
            &["sysfont-apply", "TOB9Y0"],
            &["sysfont-apply", "..\\plan"],
            &["sysfont-apply", "C:\\Temp\\plan.json"],
            &["sysfont-apply", "a b"],
            &["sysfont-apply", "12345678901234"],
            &["sysfont-apply", "tob9y0", "extra"],
            &["sysfont-reuse", "a/b"],
            &["sysfont-restore", "now"],
            &["sysfont-install", "tob9y0"],
        ] {
            assert!(ok(bad).is_err(), "{bad:?}");
        }
        for op in [Op::sysfont_apply("abc123").unwrap(), Op::SysfontRestore, Op::sysfont_reuse("zz9").unwrap()] {
            assert_eq!(parse(&op.to_args()), Ok(op.clone()));
        }
        assert_eq!(Op::sysfont_apply("tob9y0").unwrap().command_line(), "sysfont-apply tob9y0");
        assert!(Op::sysfont_apply("x/y").is_none());
    }

    #[test]
    fn end_processes_parses_strictly() {
        let ok = |a: &[&str]| parse(&args(a));
        assert_eq!(
            ok(&["end-processes", "4312:133800000000000000", "99:1"]),
            Ok(Op::EndProcesses { targets: vec![(4312, 133800000000000000), (99, 1)] })
        );
        let many: Vec<String> = std::iter::once("end-processes".to_owned()).chain((0..257).map(|i| format!("{}:1", i + 10))).collect();
        assert!(parse(&many[..257]).is_ok(), "256 targets");
        assert!(parse(&many).is_err(), "257 targets");
        for bad in [
            &["end-processes"][..],
            &["end-processes", "notepad.exe"],
            &["end-processes", "4312"],
            &["end-processes", "4312:"],
            &["end-processes", ":5"],
            &["end-processes", "4:5"],
            &["end-processes", "0:5"],
            &["end-processes", "100:0"],
            &["end-processes", "-5:1"],
            &["end-processes", "100:-1"],
            &["end-processes", "+100:1"],
            &["end-processes", "100:1:2"],
            &["end-processes", "99999999999:1"],
            &["end-processes", "100:99999999999999999999"],
            &["end-processes", "100:1", "cmd.exe"],
            &["end-processes", "100:1 200:2"],
            &["end-process", "100:1"],
        ] {
            assert!(ok(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn end_processes_round_trips() {
        let op = Op::end_processes(&[(4312, 133800000000000000), (77, 5)]).unwrap();
        assert_eq!(parse(&op.to_args()), Ok(op.clone()));
        assert_eq!(op.command_line(), "end-processes 4312:133800000000000000 77:5");
        assert!(Op::end_processes(&[]).is_none());
        assert!(Op::end_processes(&[(4, 1)]).is_none());
        assert!(Op::end_processes(&[(100, 0)]).is_none());
        assert!(Op::end_processes(&vec![(100, 1); MAX_TARGETS + 1]).is_none());
    }

    #[test]
    fn launch_app_takes_only_a_store_app_id() {
        let ok = |a: &[&str]| parse(&args(a));
        const NOTEPAD: &str = "Microsoft.WindowsNotepad_8wekyb3d8bbwe!App";
        assert_eq!(ok(&["launch-app", NOTEPAD]), Ok(Op::LaunchApp { id: NOTEPAD.into() }));
        assert!(ok(&["launch-app", "Agilebits.1Password_amwd9z03whsfe!Agilebits.OnePassword"]).is_ok());
        for bad in [
            &["launch-app"][..],
            &["launch-app", NOTEPAD, "--flag"],
            &["launch-app", "cmd.exe"],
            &["launch-app", "C:\\Windows\\System32\\cmd.exe"],
            &["launch-app", "Microsoft.WindowsNotepad!App"],
            &["launch-app", "Microsoft.WindowsNotepad_8wekyb3d8bbwe"],
            &["launch-app", "Microsoft.WindowsNotepad_8wekyb3d8bbwe!"],
            &["launch-app", "Microsoft.WindowsNotepad_short!App"],
            &["launch-app", "..\\x_8wekyb3d8bbwe!App"],
            &["launch-app", "Name_8wekyb3d8bbwe!App x"],
            &["launch-app", "Name_8wekyb3d8bbwe!App!B"],
            &["launch-app", "Name_8wekyb3d8bbwe!a/b"],
        ] {
            assert!(ok(bad).is_err(), "{bad:?}");
        }
        let op = Op::launch_app(NOTEPAD).unwrap();
        assert_eq!(parse(&op.to_args()), Ok(op.clone()));
        assert_eq!(op.command_line(), format!("launch-app {NOTEPAD}"));
        assert!(Op::launch_app("shell:AppsFolder\\x").is_none());
    }

    #[test]
    fn to_args_round_trips() {
        for enable in [true, false] {
            let op = Op::SoundDevice { enable, id: ID.into() };
            assert_eq!(parse(&op.to_args()), Ok(op));
        }
        assert!(Op::sound_device(true, "nope").is_none());
    }
}
