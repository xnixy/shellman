//! The apps the session runs, in start order.

use std::path::{Path, PathBuf};

/// Replaces the app list, for tests: `exe|arg|arg;exe2|arg`. Never set in real use.
pub const ENV: &str = "SHELLMAN_SESSION_APPS";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    /// Short name for the log: the exe's name without `shellman-` and `.exe`.
    pub name: String,
    pub exe: PathBuf,
    pub args: Vec<String>,
}

impl App {
    fn new(exe: PathBuf, args: Vec<String>) -> App {
        let stem = exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let name = stem.strip_prefix("shellman-").unwrap_or(&stem).to_string();
        App { name, exe, args }
    }

    pub fn is_bar(&self) -> bool {
        self.name == "bar"
    }
}

/// The real list: desktop first (wallpaper, and Windows' modern-app host), then the bar, the shelf
/// (what sits on the wallpaper) and the switcher, then the residents, which stay hidden until summoned. The session owns the
/// residents; the bar starts them itself only when run without a session.
pub fn defaults(dir: &Path) -> Vec<App> {
    let one = |name: &str, args: &[&str]| {
        App::new(dir.join(format!("shellman-{name}.exe")), args.iter().map(|a| a.to_string()).collect())
    };
    vec![
        one("desktop", &[]),
        one("bar", &[]),
        one("shelf", &[]),
        one("switcher", &[]),
        one("launcher", &["--background"]),
        one("control", &["--background"]),
        one("taskview", &["--background"]),
    ]
}

pub fn parse(spec: &str) -> Vec<App> {
    spec.split(';')
        .filter(|e| !e.trim().is_empty())
        .map(|entry| {
            let mut parts = entry.split('|').map(str::to_string);
            let exe = PathBuf::from(parts.next().unwrap_or_default());
            App::new(exe, parts.collect())
        })
        .collect()
}

/// The list to run and whether it is the test override.
pub fn load() -> (Vec<App>, bool) {
    if let Some(spec) = std::env::var(ENV).ok().filter(|s| !s.trim().is_empty()) {
        return (parse(&spec), true);
    }
    let dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)).unwrap_or_default();
    (defaults(&dir), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_in_start_order_beside_the_exe() {
        let apps = defaults(Path::new("C:\\s"));
        let names: Vec<_> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["desktop", "bar", "shelf", "switcher", "launcher", "control", "taskview"]);
        assert_eq!(apps[4].args, ["--background"]);
        assert!(apps[1].is_bar());
        assert_eq!(apps[0].exe, Path::new("C:\\s\\shellman-desktop.exe"));
    }

    #[test]
    fn parses_the_test_override() {
        let apps = parse("C:\\t\\a.exe|1|2;C:\\t\\shellman-bar.exe;");
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].name, "a");
        assert_eq!(apps[0].args, ["1", "2"]);
        assert!(apps[1].is_bar());
    }
}
