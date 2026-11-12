//! The list of installed apps, from the shell's Apps folder (`shell:AppsFolder`): the same
//! entries the Start menu shows, Store apps and classic shortcuts alike.
//!
//! Enumerating takes a moment, so `AppIndex` does it on its own thread and the UI only ever reads
//! the finished list from memory.

mod enumerate;
mod launch;

pub use enumerate::enumerate;
pub use launch::{launch, run_text};

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One installed app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    /// What the user sees and searches, e.g. "Calculator".
    pub name: String,
    /// The shell's parsing name inside the Apps folder: an AUMID for Store apps, a path or GUID
    /// path for classic ones. `shell:AppsFolder\<id>` launches it and gives its icon.
    pub id: String,
}

impl App {
    /// The path the shell understands for both launching and fetching the app's icon.
    pub fn shell_path(&self) -> String {
        format!(r"shell:AppsFolder\{}", self.id)
    }
}

/// The apps list, loaded off the UI thread and kept in memory.
#[derive(Clone, Default)]
pub struct AppIndex {
    inner: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    apps: Arc<Vec<App>>,
    loaded_at: Option<Instant>,
    loading: bool,
}

impl AppIndex {
    /// Start loading in the background. `done` runs on that thread once the list is ready, so
    /// the UI can repaint.
    pub fn refresh(&self, done: impl FnOnce() + Send + 'static) {
        {
            let mut s = self.inner.lock().unwrap();
            if s.loading {
                return;
            }
            s.loading = true;
        }
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            let apps = enumerate();
            {
                let mut s = inner.lock().unwrap();
                // A failed enumeration keeps the older list rather than emptying it.
                if !apps.is_empty() || s.apps.is_empty() {
                    s.apps = Arc::new(apps);
                }
                s.loaded_at = Some(Instant::now());
                s.loading = false;
            }
            done();
        });
    }

    /// Load again if the list is older than `max_age` (or was never loaded).
    pub fn refresh_if_older(&self, max_age: Duration, done: impl FnOnce() + Send + 'static) {
        let stale = self
            .inner
            .lock()
            .unwrap()
            .loaded_at
            .is_none_or(|t| t.elapsed() > max_age);
        if stale {
            self.refresh(done);
        }
    }

    /// The current list, sorted by name. Cheap: a shared pointer.
    pub fn apps(&self) -> Arc<Vec<App>> {
        self.inner.lock().unwrap().apps.clone()
    }
}
