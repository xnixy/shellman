//! What the main button (or Enter, or a double click) does, from the mode, what is typed in the
//! name field and what is selected. Pure: the file system is asked through a function, so every
//! case is tested without touching a disk.

use super::filter::has_wildcard;
use super::name;
use shellman_pick::Mode;
use std::path::{Component, Path, PathBuf};

/// What a path is on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Missing,
    File,
    Dir,
}

/// What pressing the main button comes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Done: hand these back to the app.
    Choose(Vec<PathBuf>),
    /// Go into this folder and keep picking.
    Enter(PathBuf),
    /// Show only names matching this typed filter (`*.log`).
    Filter(String),
    /// The file exists: ask "Replace it?" in the bottom bar before choosing it.
    Replace(PathBuf),
    /// Can't: say why, and keep picking.
    Problem(String),
    /// Nothing to act on.
    Nothing,
}

/// Everything `decide` looks at.
pub struct Input<'a> {
    pub mode: Mode,
    pub multiple: bool,
    pub must_exist: bool,
    pub overwrite_prompt: bool,
    /// The folder on show; None on This PC.
    pub here: Option<&'a Path>,
    /// The name field, with environment variables already expanded.
    pub typed: &'a str,
    /// Whether the field was typed in after the selection last changed: then it wins over a
    /// selected folder.
    pub typed_since_selection: bool,
    /// The selection, in order: each path and whether it is a folder.
    pub selected: &'a [(PathBuf, bool)],
    /// The app's default extension, and the chosen type's own (see `name::extension_to_add`).
    pub default_ext: &'a str,
    pub type_ext: Option<&'a str>,
}

/// The single selected folder, when the selection is exactly that.
pub fn lone_folder(selected: &[(PathBuf, bool)]) -> Option<&Path> {
    match selected {
        [(p, true)] => Some(p),
        _ => None,
    }
}

/// Whether the main button would go into the selected folder rather than finish: one folder
/// is selected, nothing was typed since, and the mode is not choosing folders.
pub fn enters_folder(mode: Mode, selected: &[(PathBuf, bool)], typed_since_selection: bool) -> bool {
    mode != Mode::Folder && !typed_since_selection && lone_folder(selected).is_some()
}

pub fn decide(input: &Input, kind: &dyn Fn(&Path) -> Kind) -> Outcome {
    if enters_folder(input.mode, input.selected, input.typed_since_selection) {
        return Outcome::Enter(input.selected[0].0.clone());
    }
    let typed = input.typed.trim();
    match input.mode {
        Mode::Folder => folder(input, typed, kind),
        Mode::Save => save(input, typed, kind),
        Mode::Open | Mode::OpenMany => open(input, typed, kind),
    }
}

fn folder(input: &Input, typed: &str, kind: &dyn Fn(&Path) -> Kind) -> Outcome {
    if !input.typed_since_selection {
        if let Some(dir) = lone_folder(input.selected) {
            return Outcome::Choose(vec![dir.to_path_buf()]);
        }
    }
    if typed.is_empty() {
        return match input.here {
            Some(here) => Outcome::Choose(vec![here.to_path_buf()]),
            None => Outcome::Nothing,
        };
    }
    let Some(path) = resolve(typed, input.here) else {
        return Outcome::Problem("Open a folder first, or type a full path.".into());
    };
    match kind(&path) {
        Kind::Dir => Outcome::Choose(vec![path]),
        Kind::File => Outcome::Problem(format!("{typed} is a file, not a folder.")),
        Kind::Missing => Outcome::Problem(format!("Can't find a folder named {typed}.")),
    }
}

fn open(input: &Input, typed: &str, kind: &dyn Fn(&Path) -> Kind) -> Outcome {
    if typed.is_empty() {
        let files: Vec<PathBuf> = input
            .selected
            .iter()
            .filter(|(_, dir)| !dir)
            .map(|(p, _)| p.clone())
            .collect();
        return match files.len() {
            0 => Outcome::Nothing,
            1 => Outcome::Choose(files),
            _ if input.multiple => Outcome::Choose(files),
            _ => Outcome::Problem("Choose just one file.".into()),
        };
    }
    if has_wildcard(typed) {
        return Outcome::Filter(typed.to_owned());
    }
    let names = name::split(typed);
    if names.len() > 1 {
        if !input.multiple {
            return Outcome::Problem("Choose just one file.".into());
        }
        let mut paths = Vec::new();
        for n in &names {
            let Some(p) = resolve(n, input.here) else {
                return Outcome::Problem("Open a folder first, or type full paths.".into());
            };
            match kind(&p) {
                Kind::File => paths.push(p),
                Kind::Dir => return Outcome::Problem(format!("{n} is a folder.")),
                Kind::Missing if input.must_exist => {
                    return Outcome::Problem(format!("Can't find {n}."))
                }
                Kind::Missing => paths.push(p),
            }
        }
        return Outcome::Choose(paths);
    }
    let one = names.into_iter().next().unwrap_or_default();
    let Some(path) = resolve(&one, input.here) else {
        return Outcome::Problem("Open a folder first, or type a full path.".into());
    };
    match kind(&path) {
        Kind::Dir => Outcome::Enter(path),
        Kind::File => Outcome::Choose(vec![path]),
        Kind::Missing => {
            // "notes" may mean "notes.txt" when that is the app's kind of file.
            let ext = name::extension_to_add(input.default_ext, input.type_ext);
            let fuller = PathBuf::from(name::with_extension(&path.to_string_lossy(), ext));
            if fuller != path && kind(&fuller) == Kind::File {
                Outcome::Choose(vec![fuller])
            } else if input.must_exist {
                Outcome::Problem(format!("Can't find {one}."))
            } else {
                Outcome::Choose(vec![path])
            }
        }
    }
}

fn save(input: &Input, typed: &str, kind: &dyn Fn(&Path) -> Kind) -> Outcome {
    if typed.is_empty() {
        return Outcome::Nothing;
    }
    if has_wildcard(typed) {
        return Outcome::Filter(typed.to_owned());
    }
    let one = name::split(typed).into_iter().next().unwrap_or_default();
    let Some(path) = resolve(&one, input.here) else {
        return Outcome::Problem("Open a folder to save in first.".into());
    };
    if kind(&path) == Kind::Dir {
        return Outcome::Enter(path);
    }
    let (Some(parent), Some(file)) = (path.parent(), path.file_name()) else {
        return Outcome::Problem("Type a name to save under.".into());
    };
    let file = file.to_string_lossy();
    if let Err(why) = name::check(&file) {
        return Outcome::Problem(why);
    }
    if kind(parent) != Kind::Dir {
        return Outcome::Problem(format!("The folder {} doesn't exist.", parent.display()));
    }
    let ext = name::extension_to_add(input.default_ext, input.type_ext);
    let full = parent.join(name::with_extension(&file, ext));
    match kind(&full) {
        Kind::Dir => Outcome::Enter(full),
        Kind::File if input.overwrite_prompt => Outcome::Replace(full),
        _ => Outcome::Choose(vec![full]),
    }
}

/// A typed name or path as a full path: relative ones are taken from `here`. Quotes around it
/// are dropped, and `.` and `..` worked out.
pub fn resolve(typed: &str, here: Option<&Path>) -> Option<PathBuf> {
    let t = typed.trim().trim_matches('"').trim();
    if t.is_empty() {
        return None;
    }
    let p = Path::new(t);
    let full = if p.is_absolute() {
        p.to_path_buf()
    } else {
        // `join` keeps `here`'s drive for a rooted path like `\Temp`.
        here?.join(p)
    };
    Some(normalize(&full))
}

/// `C:\a\b\..\c` as `C:\a\c`, without asking the disk.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                // Never above the root.
                if out.file_name().is_some() {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small disk: C:\docs with notes.txt, a.txt, b.txt and the folder sub.
    fn disk(p: &Path) -> Kind {
        let s = p.to_string_lossy().to_lowercase();
        match s.as_str() {
            r"c:\" | r"c:\docs" | r"c:\docs\sub" | r"c:\other" => Kind::Dir,
            r"c:\docs\notes.txt" | r"c:\docs\a.txt" | r"c:\docs\b.txt" | r"c:\other\x.md" => {
                Kind::File
            }
            _ => Kind::Missing,
        }
    }

    struct Case {
        mode: Mode,
        multiple: bool,
        typed: &'static str,
        typed_since: bool,
        selected: Vec<(PathBuf, bool)>,
        here: Option<&'static str>,
        default_ext: &'static str,
        type_ext: Option<&'static str>,
    }

    impl Case {
        fn new(mode: Mode, typed: &'static str) -> Self {
            Self {
                mode,
                multiple: mode == Mode::OpenMany,
                typed,
                typed_since: !typed.is_empty(),
                selected: Vec::new(),
                here: Some(r"C:\docs"),
                default_ext: "",
                type_ext: None,
            }
        }

        fn run(&self) -> Outcome {
            let input = Input {
                mode: self.mode,
                multiple: self.multiple,
                must_exist: matches!(self.mode, Mode::Open | Mode::OpenMany),
                overwrite_prompt: self.mode == Mode::Save,
                here: self.here.map(Path::new),
                typed: self.typed,
                typed_since_selection: self.typed_since,
                selected: &self.selected,
                default_ext: self.default_ext,
                type_ext: self.type_ext,
            };
            decide(&input, &disk)
        }
    }

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn choose(paths: &[&str]) -> Outcome {
        Outcome::Choose(paths.iter().map(|s| p(s)).collect())
    }

    #[test]
    fn open_takes_a_typed_name_from_the_folder_on_show() {
        assert_eq!(Case::new(Mode::Open, "notes.txt").run(), choose(&[r"C:\docs\notes.txt"]));
        assert_eq!(Case::new(Mode::Open, r"C:\other\x.md").run(), choose(&[r"C:\other\x.md"]));
        assert_eq!(Case::new(Mode::Open, r#""notes.txt""#).run(), choose(&[r"C:\docs\notes.txt"]));
    }

    #[test]
    fn open_goes_into_a_typed_folder_and_filters_on_wildcards() {
        assert_eq!(Case::new(Mode::Open, "sub").run(), Outcome::Enter(p(r"C:\docs\sub")));
        assert_eq!(Case::new(Mode::Open, "..").run(), Outcome::Enter(p(r"C:\")));
        assert_eq!(Case::new(Mode::Open, r"..\other").run(), Outcome::Enter(p(r"C:\other")));
        assert_eq!(Case::new(Mode::Open, "*.md").run(), Outcome::Filter("*.md".into()));
    }

    #[test]
    fn open_needs_the_file_to_exist_but_tries_the_default_extension() {
        assert!(matches!(Case::new(Mode::Open, "nope.txt").run(), Outcome::Problem(_)));
        let mut c = Case::new(Mode::Open, "notes");
        assert!(matches!(c.run(), Outcome::Problem(_)));
        c.default_ext = "txt";
        assert_eq!(c.run(), choose(&[r"C:\docs\notes.txt"]));
    }

    #[test]
    fn open_with_nothing_typed_takes_the_selected_files() {
        let mut c = Case::new(Mode::Open, "");
        assert_eq!(c.run(), Outcome::Nothing);
        c.selected = vec![(p(r"C:\docs\a.txt"), false)];
        assert_eq!(c.run(), choose(&[r"C:\docs\a.txt"]));
        c.selected.push((p(r"C:\docs\b.txt"), false));
        assert!(matches!(c.run(), Outcome::Problem(_)));
        c.multiple = true;
        assert_eq!(c.run(), choose(&[r"C:\docs\a.txt", r"C:\docs\b.txt"]));
    }

    #[test]
    fn several_quoted_names_need_multiple_and_must_all_exist() {
        let mut c = Case::new(Mode::OpenMany, r#""a.txt" "b.txt""#);
        assert_eq!(c.run(), choose(&[r"C:\docs\a.txt", r"C:\docs\b.txt"]));
        c.typed = r#""a.txt" "zz.txt""#;
        assert!(matches!(c.run(), Outcome::Problem(_)));
        c.typed = r#""a.txt" "sub""#;
        assert!(matches!(c.run(), Outcome::Problem(_)));
        let one = Case::new(Mode::Open, r#""a.txt" "b.txt""#);
        assert!(matches!(one.run(), Outcome::Problem(_)));
    }

    #[test]
    fn a_selected_folder_is_entered_unless_typed_over() {
        for mode in [Mode::Open, Mode::Save] {
            let mut c = Case::new(mode, "notes.txt");
            c.selected = vec![(p(r"C:\docs\sub"), true)];
            c.typed_since = false;
            assert_eq!(c.run(), Outcome::Enter(p(r"C:\docs\sub")), "{mode:?}");
            c.typed_since = true;
            assert_ne!(c.run(), Outcome::Enter(p(r"C:\docs\sub")), "{mode:?}");
        }
        assert!(enters_folder(Mode::Open, &[(p("x"), true)], false));
        assert!(!enters_folder(Mode::Folder, &[(p("x"), true)], false));
        assert!(!enters_folder(Mode::Open, &[(p("x"), true), (p("y"), true)], false));
    }

    #[test]
    fn save_adds_the_extension_and_asks_before_replacing() {
        let mut c = Case::new(Mode::Save, "report");
        assert_eq!(c.run(), choose(&[r"C:\docs\report"]));
        c.default_ext = "txt";
        assert_eq!(c.run(), choose(&[r"C:\docs\report.txt"]));
        c.type_ext = Some("md");
        assert_eq!(c.run(), choose(&[r"C:\docs\report.md"]));
        let mut c = Case::new(Mode::Save, "notes");
        c.default_ext = "txt";
        assert_eq!(c.run(), Outcome::Replace(p(r"C:\docs\notes.txt")));
        assert_eq!(Case::new(Mode::Save, "a.txt").run(), Outcome::Replace(p(r"C:\docs\a.txt")));
    }

    #[test]
    fn save_refuses_bad_names_and_missing_folders() {
        assert!(matches!(Case::new(Mode::Save, "bad|name").run(), Outcome::Problem(_)));
        assert!(matches!(Case::new(Mode::Save, "CON").run(), Outcome::Problem(_)));
        assert!(matches!(Case::new(Mode::Save, r"C:\nowhere\x.txt").run(), Outcome::Problem(_)));
        assert_eq!(Case::new(Mode::Save, "").run(), Outcome::Nothing);
        assert_eq!(Case::new(Mode::Save, "sub").run(), Outcome::Enter(p(r"C:\docs\sub")));
        assert_eq!(Case::new(Mode::Save, r"sub\new.txt").run(), choose(&[r"C:\docs\sub\new.txt"]));
        assert_eq!(Case::new(Mode::Save, "*.txt").run(), Outcome::Filter("*.txt".into()));
        let mut c = Case::new(Mode::Save, "x.txt");
        c.here = None;
        assert!(matches!(c.run(), Outcome::Problem(_)));
    }

    #[test]
    fn folder_mode_takes_the_selected_the_typed_or_the_current_folder() {
        let mut c = Case::new(Mode::Folder, "");
        assert_eq!(c.run(), choose(&[r"C:\docs"]));
        c.selected = vec![(p(r"C:\docs\sub"), true)];
        c.typed = "sub";
        c.typed_since = false;
        assert_eq!(c.run(), choose(&[r"C:\docs\sub"]));
        let mut c = Case::new(Mode::Folder, r"C:\other");
        assert_eq!(c.run(), choose(&[r"C:\other"]));
        c.typed = "notes.txt";
        assert!(matches!(c.run(), Outcome::Problem(_)));
        c.typed = "missing";
        assert!(matches!(c.run(), Outcome::Problem(_)));
        let mut c = Case::new(Mode::Folder, "");
        c.here = None;
        assert_eq!(c.run(), Outcome::Nothing);
    }

    #[test]
    fn resolving_works_out_dots_and_keeps_the_drive() {
        let here = Some(Path::new(r"C:\docs\sub"));
        assert_eq!(resolve(r"..\..\x", here), Some(p(r"C:\x")));
        assert_eq!(resolve(r"..\..\..\..", here), Some(p(r"C:\")));
        assert_eq!(resolve(r"\Temp", here), Some(p(r"C:\Temp")));
        assert_eq!(resolve(r"D:\y", here), Some(p(r"D:\y")));
        assert_eq!(resolve("x", None), None);
        assert_eq!(resolve("  ", here), None);
    }
}
