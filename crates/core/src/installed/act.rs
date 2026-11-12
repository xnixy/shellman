//! What the page may do to an app, and the seam it goes through. Starting a program or removing a
//! package is behind `Runner`: the real one is in `win`, a dry run (set `SHELLMAN_APPS_DRY_RUN=1`)
//! only says what it would have done, and tests use a fake that records. Nothing else in the page
//! ever starts an uninstaller.

use super::cmdline::Launch;
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Action {
    /// Start a program (an uninstaller or a changer) and wait for it to end.
    Launch(Launch),
    /// Remove a Store app for the current user, by package full name.
    RemovePackage(String),
}

/// The environment variable that turns every action into a description of it.
pub const DRY_RUN_VAR: &str = "SHELLMAN_APPS_DRY_RUN";

/// Told the percentage of a removal as it goes.
pub type Progress = Arc<dyn Fn(u32) + Send + Sync>;

pub trait Runner: Send + Sync {
    /// Start `launch` and wait until it ends. `Ok` carries a note for the page ("" for none).
    fn launch(&self, launch: &Launch) -> Result<String, String>;
    /// Remove a package; `progress` is told the percentage as it goes.
    fn remove_package(&self, full_name: &str, progress: Progress) -> Result<String, String>;
}

/// Do an action through a runner.
pub fn perform(runner: &dyn Runner, action: &Action, progress: Progress) -> Result<String, String> {
    match action {
        Action::Launch(l) => runner.launch(l),
        Action::RemovePackage(name) => runner.remove_package(name, progress),
    }
}

/// Says what would be done, and does nothing.
pub struct DryRun;

impl Runner for DryRun {
    fn launch(&self, launch: &Launch) -> Result<String, String> {
        let note = format!("Dry run: would start {}", launch.command_line());
        eprintln!("{note}");
        Ok(note)
    }

    fn remove_package(&self, full_name: &str, progress: Progress) -> Result<String, String> {
        let note = format!("Dry run: would remove package {full_name}");
        eprintln!("{note}");
        progress(50);
        Ok(note)
    }
}

/// The runner this process uses: the dry run when asked for, otherwise the real one.
pub fn runner() -> Arc<dyn Runner> {
    if std::env::var(DRY_RUN_VAR).is_ok_and(|v| v != "0" && !v.is_empty()) { Arc::new(DryRun) } else { Arc::new(super::win::Real) }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Records what it is asked and starts nothing.
    #[derive(Default)]
    pub struct Fake {
        pub log: Mutex<Vec<String>>,
        pub fail: bool,
    }

    impl Runner for Fake {
        fn launch(&self, l: &Launch) -> Result<String, String> {
            self.log.lock().unwrap().push(format!("launch {}", l.command_line()));
            if self.fail { Err("no".into()) } else { Ok(String::new()) }
        }

        fn remove_package(&self, name: &str, progress: Progress) -> Result<String, String> {
            self.log.lock().unwrap().push(format!("remove {name}"));
            progress(100);
            Ok(String::new())
        }
    }

    #[test]
    fn actions_reach_the_runner_and_only_it() {
        let fake = Fake::default();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let progress: Progress = Arc::new(move |p| sink.lock().unwrap().push(p));
        perform(&fake, &Action::Launch(Launch::new("a b.exe", "/x")), progress.clone()).unwrap();
        perform(&fake, &Action::RemovePackage("P_1".into()), progress).unwrap();
        assert_eq!(*fake.log.lock().unwrap(), vec![r#"launch "a b.exe" /x"#, "remove P_1"]);
        assert_eq!(*seen.lock().unwrap(), vec![100]);
    }

    #[test]
    fn a_failure_comes_back() {
        let fake = Fake { fail: true, ..Default::default() };
        assert!(perform(&fake, &Action::Launch(Launch::new("x", "")), Arc::new(|_| {})).is_err());
    }

    #[test]
    fn the_dry_run_succeeds_and_says_so() {
        let note = DryRun.launch(&Launch::new("u.exe", "/q")).unwrap();
        assert!(note.starts_with("Dry run") && note.contains("u.exe"));
    }
}
