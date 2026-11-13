//! How often each app was launched from here, kept in `%APPDATA%\shellman\launcher.json`. It
//! only counts launches made through the launcher: nothing is read from Explorer or Windows.

use shellman_core::apps_index::App;
use shellman_core::settings::launcher::LauncherSettings;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Apps in the "most used" row.
pub const TOP: usize = 6;

#[derive(Default)]
pub struct Usage {
    data: LauncherSettings,
    /// Saves started and not yet finished. The launcher's own save also trips the folder watch;
    /// reading the file back before it is done would undo the newest launch.
    saving: Arc<AtomicUsize>,
}

impl Usage {
    pub fn load() -> Self {
        Self {
            data: LauncherSettings::load(),
            saving: Arc::default(),
        }
    }

    pub fn count(&self, id: &str) -> u32 {
        self.data.count(id)
    }

    /// Count one launch and save, on a thread of its own so the UI never waits for the disk.
    pub fn record(&mut self, id: &str) {
        *self.data.counts.entry(id.to_owned()).or_insert(0) += 1;
        let snapshot = self.data.clone();
        let saving = self.saving.clone();
        saving.fetch_add(1, Ordering::AcqRel);
        std::thread::spawn(move || {
            snapshot.save();
            saving.fetch_sub(1, Ordering::AcqRel);
        });
    }

    /// Read the file again, when the settings folder changed: the settings app clears the history
    /// from outside. True when the counts differ now. Skipped while our own save is under way.
    pub fn reload(&mut self) -> bool {
        if self.saving.load(Ordering::Acquire) > 0 {
            return false;
        }
        let fresh = LauncherSettings::load();
        let changed = fresh != self.data;
        self.data = fresh;
        changed
    }

    /// Indices into `apps` of the most launched ones, most first, at most `TOP`; apps never
    /// launched are not included, so a fresh install shows no row.
    pub fn top(&self, apps: &[App]) -> Vec<usize> {
        let mut used: Vec<(usize, u32)> = apps
            .iter()
            .enumerate()
            .map(|(i, a)| (i, self.count(&a.id)))
            .filter(|&(_, n)| n > 0)
            .collect();
        // Stable: ties stay alphabetical, as `apps` is.
        used.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
        used.into_iter().take(TOP).map(|(i, _)| i).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str) -> App {
        App::new(id, id)
    }

    #[test]
    fn top_lists_only_launched_apps_by_count() {
        let apps = [app("a"), app("b"), app("c"), app("d")];
        let mut u = Usage::default();
        assert!(u.top(&apps).is_empty());
        u.data.counts.insert("c".into(), 5);
        u.data.counts.insert("a".into(), 2);
        u.data.counts.insert("d".into(), 2);
        u.data.counts.insert("gone".into(), 9);
        assert_eq!(u.top(&apps), [2, 0, 3]);
    }

    #[test]
    fn reload_waits_for_our_own_save() {
        let mut u = Usage::default();
        u.saving.fetch_add(1, Ordering::AcqRel);
        u.data.counts.insert("a".into(), 1);
        // Whatever the file holds, a save in flight means memory is the newer truth.
        assert!(!u.reload());
        assert_eq!(u.count("a"), 1);
    }

    #[test]
    fn top_is_capped() {
        let apps: Vec<App> = (0..20).map(|i| app(&format!("a{i}"))).collect();
        let mut u = Usage::default();
        for a in &apps {
            u.data.counts.insert(a.id.clone(), 1);
        }
        assert_eq!(u.top(&apps).len(), TOP);
    }
}
