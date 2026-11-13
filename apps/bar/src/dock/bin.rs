//! The dock's Recycle Bin icon: empty or full, open it in Files, and empty it.
//!
//! Whether it is full is asked of the shell (`SHQueryRecycleBinW`, which can take a moment on a
//! sleeping disk) on a thread of its own, once at the start and again only when one of the bin's
//! folders changes: the folder watcher Files' bin page uses (`recycle_bin::watch_dirs`, one
//! folder per drive and user, plus that drive's `$Recycle.Bin`). No polling. The docks are woken
//! when the answer changes. (A fixed drive that has never had a bin is only watched from the next
//! change anywhere else on.)
//!
//! Files shows the bin itself, so nothing here hands off to `shell:RecycleBinFolder` (which would
//! start Explorer's folder window). Emptying is confirmed in the dock's own menu, in a second
//! step that says how much goes; then it runs on a thread of its own.


use shellman_core::recycle_bin::{self, BinState};
use shellman_core::watch::Watcher;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Whether the bin had something in it when last asked, for the icon pack's empty and full bins.
static FULL: AtomicBool = AtomicBool::new(false);

pub fn looks_full() -> bool {
    FULL.load(Ordering::Relaxed)
}

#[derive(Default)]
struct Shared {
    state: Option<BinState>,
    /// Folders to watch, found by the last query; taken by the next `poll`.
    dirs: Option<Vec<PathBuf>>,
    /// A query is running; `again` asks it to run once more when done.
    busy: bool,
    again: bool,
}

pub struct Bin {
    watcher: Watcher,
    shared: Arc<Mutex<Shared>>,

}

impl Bin {
    pub fn new(ctx: egui::Context) -> Self {
        let bin = Self { watcher: Watcher::new(ctx), shared: Arc::default() };
        bin.refresh();
        bin
    }

    /// The bin's item count and size as last asked; None until the first answer.
    pub fn state(&self) -> Option<BinState> {
        self.shared.lock().unwrap().state
    }

    /// In a frame: if a bin folder changed, ask again (off this thread). Free otherwise.
    pub fn poll(&self) {
        if let Some(dirs) = self.shared.lock().unwrap().dirs.take() {
            self.watcher.set_dirs(dirs);
        }
        if !self.watcher.poll().is_empty() {
            self.refresh();
        }
    }

    /// Ask the shell on a thread of its own; a change meanwhile asks once more after it.
    fn refresh(&self) {
        {
            let mut s = self.shared.lock().unwrap();
            if s.busy {
                s.again = true;
                return;
            }
            s.busy = true;
        }
        let shared = self.shared.clone();
        std::thread::spawn(move || loop {
            let state = recycle_bin::state();
            let mut dirs = recycle_bin::watch_dirs();
            // Each drive's bin root, where a user's folder appears at the drive's first delete.
            let roots: Vec<PathBuf> = dirs.iter().filter_map(|d| d.parent().map(PathBuf::from)).collect();
            dirs.extend(roots);
            dirs.sort();
            dirs.dedup();
            let mut s = shared.lock().unwrap();
            let changed = s.state != state;
            s.state = state;
            s.dirs = Some(dirs);
            if let Some(state) = state {
                FULL.store(!state.is_empty(), Ordering::Relaxed);
            }
            if s.again {
                s.again = false;
                continue;
            }
            s.busy = false;
            drop(s);
            if changed {
                super::wake_docks_from_thread();
            }
            // The watcher's new folders are set in the next frame; make sure there is one.
            crate::host::request_frame();
            break;
        });
    }
}

/// Start Files on the bin. False when Files is not next to the bar.
pub fn open() {
    std::thread::spawn(|| shellman_core::sibling::start_with("files", &["--recycle-bin"]));
}

/// Empty the bin for good: only after the menu's confirmation. On a thread of its own.
pub fn empty() {
    std::thread::spawn(|| {
        if recycle_bin::state().is_some_and(|s| !s.is_empty()) {
            let _ = recycle_bin::empty(0);
        }
    });
}
