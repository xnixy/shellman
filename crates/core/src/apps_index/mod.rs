//! The list of installed apps, from the shell's Apps folder (`shell:AppsFolder`): the same
//! entries the Start menu shows, Store apps and classic shortcuts alike.
//!
//! Enumerating takes a moment, so `AppIndex` does it on its own thread and the UI only ever reads
//! the finished list from memory.

pub mod activate;
mod enumerate;
mod launch;
mod merge;
mod noise;
mod startmenu;

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
    /// `id` is a file to start directly (a shellman exe, or a Start menu shortcut the Apps folder
    /// has not caught up with) rather than an Apps folder name.
    pub direct: bool,
    /// One of shellman's own apps.
    pub own: bool,
    /// A Windows app that shares its name with one of shellman's: shown with a quiet "Windows"
    /// line so the two can be told apart.
    pub windows_twin: bool,
}

impl App {
    /// An entry of the Apps folder.
    pub fn new(name: impl Into<String>, id: impl Into<String>) -> App {
        App { name: name.into(), id: id.into(), direct: false, own: false, windows_twin: false }
    }

    /// The path the shell understands for both launching and fetching the app's icon.
    pub fn shell_path(&self) -> String {
        if self.direct {
            self.id.clone()
        } else {
            format!(r"shell:AppsFolder\{}", self.id)
        }
    }

    /// The name as the grid labels it: a Windows namesake of a shellman app says so.
    pub fn grid_label(&self) -> String {
        if self.windows_twin {
            format!("{} (Windows)", self.name)
        } else {
            self.name.clone()
        }
    }

    /// What is searched besides the name: shellman's own apps also answer to "shellman".
    pub fn alias(&self) -> Option<String> {
        self.own.then(|| format!("shellman {}", self.name))
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
