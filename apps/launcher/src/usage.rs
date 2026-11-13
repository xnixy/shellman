//! How often each app was launched from here, kept in `%APPDATA%\shellman\launcher.json`. It
//! only counts launches made through the launcher: nothing is read from Explorer or Windows.

use shellman_core::apps_index::App;
use shellman_core::settings::launcher::LauncherSettings;
use std::sync::{Arc, Mutex};

/// Apps in the "most used" row.
pub const TOP: usize = 6;

#[derive(Default)]
pub struct Usage {
    data: LauncherSettings,
    /// Writes the file off the UI thread. The launcher's own save also trips the folder watch;
    /// reading the file back before it is done would undo the newest launch.
    saver: Saver<LauncherSettings>,
}

impl Usage {
    pub fn load() -> Self {
        Self {
            data: LauncherSettings::load(),
            saver: Saver::default(),
        }
    }

    pub fn count(&self, id: &str) -> u32 {
        self.data.count(id)
    }

    /// Count one launch and save, off the UI thread so it never waits for the disk.
    pub fn record(&mut self, id: &str) {
        *self.data.counts.entry(id.to_owned()).or_insert(0) += 1;
        self.saver.save(self.data.clone());
    }

    /// Read the file again, when the settings folder changed: the settings app clears the history
    /// from outside. True when the counts differ now. Skipped while our own save is under way.
    pub fn reload(&mut self) -> bool {
        if self.saver.busy() {
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

/// One serial writer: at most one save runs at a time, and a save asked for while one runs
/// only replaces the value waiting to be written, so the newest always lands last. Quick
/// launches each saved on a thread of their own before, to the same temp file: they could
/// land out of order (an older count winning) or collide on the file.
pub struct Saver<T> {
    state: Arc<Mutex<Pending<T>>>,
    write: Arc<dyn Fn(&T) + Send + Sync>,
}

struct Pending<T> {
    /// The newest value not yet written.
    latest: Option<T>,
    /// A writer thread is at work.
    running: bool,
}

impl Default for Saver<LauncherSettings> {
    fn default() -> Self {
        Saver::new(|s: &LauncherSettings| s.save())
    }
}

impl<T: Send + 'static> Saver<T> {
    pub fn new(write: impl Fn(&T) + Send + Sync + 'static) -> Self {
        Saver { state: Arc::new(Mutex::new(Pending { latest: None, running: false })), write: Arc::new(write) }
    }

    pub fn save(&self, value: T) {
        let Ok(mut st) = self.state.lock() else { return };
        st.latest = Some(value);
        if st.running {
            return;
        }
        st.running = true;
        drop(st);
        let (state, write) = (self.state.clone(), self.write.clone());
        let work = move || loop {
            let next = {
                let Ok(mut st) = state.lock() else { return };
                match st.latest.take() {
                    Some(v) => v,
                    None => {
                        st.running = false;
                        return;
                    }
                }
            };
            write(&next);
        };
        let (state, write) = (self.state.clone(), self.write.clone());
        if std::thread::Builder::new().name("launcher save".into()).spawn(work).is_err() {
            // No thread: write here rather than lose the launch.
            let value = state.lock().ok().and_then(|mut st| {
                st.running = false;
                st.latest.take()
            });
            if let Some(v) = value {
                write(&v);
            }
        }
    }

    /// A save is waiting or under way.
    pub fn busy(&self) -> bool {
        self.state.lock().is_ok_and(|st| st.running || st.latest.is_some())
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
        u.saver.state.lock().unwrap().running = true;
        u.data.counts.insert("a".into(), 1);
        // Whatever the file holds, a save in flight means memory is the newer truth.
        assert!(!u.reload());
        assert_eq!(u.count("a"), 1);
    }

    #[test]
    fn quick_saves_are_written_one_at_a_time_and_the_newest_lands_last() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let written = Arc::new(Mutex::new(Vec::new()));
        let (inside, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (w, i, m) = (written.clone(), inside.clone(), most.clone());
        // A fake disk: slow, and counting writers at work at the same time.
        let saver = Saver::new(move |v: &u32| {
            let now = i.fetch_add(1, Ordering::SeqCst) + 1;
            m.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(20));
            w.lock().unwrap().push(*v);
            i.fetch_sub(1, Ordering::SeqCst);
        });
        for n in 1..=10 {
            saver.save(n);
        }
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while saver.busy() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!saver.busy());
        let written = written.lock().unwrap();
        assert_eq!(written.last(), Some(&10), "the newest count is the one on disk");
        assert!(written.windows(2).all(|p| p[0] < p[1]), "never an older value after a newer: {written:?}");
        assert_eq!(most.load(Ordering::SeqCst), 1, "one writer at a time");
        assert!(written.len() < 10, "saves asked for meanwhile were coalesced");
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
