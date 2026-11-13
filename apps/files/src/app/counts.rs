//! How many items each folder card holds ("24 items"), counted by one worker as the cards come
//! into view, so the UI never waits on a slow or remote folder. Forgotten when a folder is
//! listed again. Leaving a folder cancels the counts still waiting, and only so many are kept.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Past this many counts kept, they are dropped and counted again as needed.
const MAX_KNOWN: usize = 4000;

pub(super) struct Counts {
    known: HashMap<PathBuf, Option<usize>>,
    /// A folder to count, and the round it was asked in.
    ask: Sender<(PathBuf, u64)>,
    done: Receiver<(PathBuf, Option<usize>)>,
    /// The current round: asks from an earlier one are skipped by the worker.
    round: Arc<AtomicU64>,
}

/// Whether an ask from round `asked` is still wanted in round `now`.
fn wanted(asked: u64, now: u64) -> bool {
    asked == now
}

impl Counts {
    pub(super) fn new(ctx: &egui::Context) -> Self {
        let (ask, asks) = channel::<(PathBuf, u64)>();
        let (reply, done) = channel();
        let ctx = ctx.clone();
        let round = Arc::new(AtomicU64::new(0));
        let now = round.clone();
        std::thread::spawn(move || {
            for (dir, asked) in asks {
                if !wanted(asked, now.load(Ordering::Relaxed)) {
                    continue;
                }
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
            round,
        }
    }

    /// Take in what the worker counted.
    pub(super) fn poll(&mut self) {
        while let Ok((dir, n)) = self.done.try_recv() {
            if self.known.len() >= MAX_KNOWN {
                self.known.clear();
            }
            self.known.insert(dir, n);
        }
    }

    /// The count, once known; asks for it the first time.
    pub(super) fn get(&mut self, dir: &Path) -> Option<usize> {
        match self.known.get(dir) {
            Some(n) => *n,
            None => {
                if self.known.len() >= MAX_KNOWN {
                    self.known.clear();
                }
                self.known.insert(dir.to_path_buf(), None);
                let _ = self.ask.send((dir.to_path_buf(), self.round.load(Ordering::Relaxed)));
                None
            }
        }
    }

    /// The tab went elsewhere: the counts not made yet are not wanted any more. They are
    /// forgotten as asked, so coming back asks again.
    pub(super) fn cancel(&mut self) {
        self.round.fetch_add(1, Ordering::Relaxed);
        self.known.retain(|_, n| n.is_some());
    }

    /// Forget the counts of the folders in `parent`, which was just listed again.
    pub(super) fn forget_in(&mut self, parent: &Path) {
        self.known.retain(|p, _| p.parent() != Some(parent));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaving_a_folder_cancels_its_counts() {
        assert!(wanted(3, 3));
        assert!(!wanted(2, 3), "asked before the tab left: skipped");
        let mut c = Counts::new(&egui::Context::default());
        let dir = std::env::temp_dir();
        assert_eq!(c.get(&dir), None);
        c.cancel();
        // Forgotten, so coming back asks again, in the new round.
        assert!(!c.known.contains_key(&dir));
        assert_eq!(c.get(&dir), None);
        assert!(c.known.contains_key(&dir));
    }

    #[test]
    fn only_so_many_counts_are_kept() {
        let mut c = Counts::new(&egui::Context::default());
        let nowhere = std::env::temp_dir().join(format!("shellman-counts-nowhere-{}", std::process::id()));
        for i in 0..MAX_KNOWN * 2 + 10 {
            c.get(&nowhere.join(i.to_string()));
        }
        assert!(c.known.len() <= MAX_KNOWN, "{}", c.known.len());
        c.cancel();
    }
}
