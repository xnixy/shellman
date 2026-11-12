//! How often each app was launched from here, kept in `%APPDATA%\shellman\launcher.json`. It
//! only counts launches made through the launcher: nothing is read from Explorer or Windows.

use serde::{Deserialize, Serialize};
use shellman_core::apps_index::App;
use std::collections::HashMap;

const NAME: &str = "launcher";
/// Apps in the "most used" row.
pub const TOP: usize = 7;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Usage {
    /// Launch count by app id.
    counts: HashMap<String, u32>,
}

impl Usage {
    pub fn load() -> Self {
        shellman_core::config::load(NAME)
    }

    pub fn count(&self, id: &str) -> u32 {
        self.counts.get(id).copied().unwrap_or(0)
    }

    /// Count one launch and save, on a thread of its own so the UI never waits for the disk.
    pub fn record(&mut self, id: &str) {
        *self.counts.entry(id.to_owned()).or_insert(0) += 1;
        let snapshot = Usage {
            counts: self.counts.clone(),
        };
        std::thread::spawn(move || shellman_core::config::save(NAME, &snapshot));
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
        used.sort_by(|a, b| b.1.cmp(&a.1));
        used.into_iter().take(TOP).map(|(i, _)| i).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str) -> App {
        App {
            name: id.into(),
            id: id.into(),
        }
    }

    #[test]
    fn top_lists_only_launched_apps_by_count() {
        let apps = [app("a"), app("b"), app("c"), app("d")];
        let mut u = Usage::default();
        assert!(u.top(&apps).is_empty());
        u.counts.insert("c".into(), 5);
        u.counts.insert("a".into(), 2);
        u.counts.insert("d".into(), 2);
        u.counts.insert("gone".into(), 9);
        assert_eq!(u.top(&apps), [2, 0, 3]);
    }

    #[test]
    fn top_is_capped() {
        let apps: Vec<App> = (0..20).map(|i| app(&format!("a{i}"))).collect();
        let mut u = Usage::default();
        for a in &apps {
            u.counts.insert(a.id.clone(), 1);
        }
        assert_eq!(u.top(&apps).len(), TOP);
    }
}
