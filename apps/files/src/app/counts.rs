//! How many items each folder card holds ("24 items"), counted by one worker as the cards come
//! into view, so the UI never waits on a slow or remote folder. Forgotten when a folder is
//! listed again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

pub(super) struct Counts {
    known: HashMap<PathBuf, Option<usize>>,
    ask: Sender<PathBuf>,
    done: Receiver<(PathBuf, Option<usize>)>,
}

impl Counts {
    pub(super) fn new(ctx: &egui::Context) -> Self {
        let (ask, asks) = channel::<PathBuf>();
        let (reply, done) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for dir in asks {
                // Hidden items count, as Explorer's "items" do in its details. A cloud folder
                // whose content is not on this disk is not listed: that would fetch it.
                let n = if shellman_core::fsops::not_local(&dir) {
                    None
                } else {
                    std::fs::read_dir(&dir).ok().map(|d| d.count())
                };
                if reply.send((dir, n)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
        Self {
            known: HashMap::new(),
            ask,
            done,
        }
    }

    /// Take in what the worker counted.
    pub(super) fn poll(&mut self) {
        while let Ok((dir, n)) = self.done.try_recv() {
            self.known.insert(dir, n);
        }
    }

    /// The count, once known; asks for it the first time.
    pub(super) fn get(&mut self, dir: &Path) -> Option<usize> {
        match self.known.get(dir) {
            Some(n) => *n,
            None => {
                self.known.insert(dir.to_path_buf(), None);
                let _ = self.ask.send(dir.to_path_buf());
                None
            }
        }
    }

    /// Forget the counts of the folders in `parent`, which was just listed again.
    pub(super) fn forget_in(&mut self, parent: &Path) {
        self.known.retain(|p, _| p.parent() != Some(parent));
    }
}
