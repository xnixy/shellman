//! Saving settings off the UI thread, in order: one writer thread runs the saves one after the
//! other, so a slider that saves on every step never leaves an older value as the last write.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;

type Job = Box<dyn FnOnce() + Send>;

pub struct Saver {
    tx: Sender<Job>,
    /// Jobs queued or running.
    pending: Arc<AtomicUsize>,
}

impl Saver {
    pub fn new() -> Self {
        let (tx, rx) = channel::<Job>();
        let pending = Arc::new(AtomicUsize::new(0));
        let counter = pending.clone();
        std::thread::spawn(move || {
            // Ends when the `Saver` is dropped and the channel closes.
            while let Ok(job) = rx.recv() {
                job();
                counter.fetch_sub(1, Ordering::AcqRel);
            }
        });
        Self { tx, pending }
    }

    /// Run `job` on the writer thread, after the saves already queued.
    pub fn run(&self, job: impl FnOnce() + Send + 'static) {
        self.pending.fetch_add(1, Ordering::AcqRel);
        if self.tx.send(Box::new(job)).is_err() {
            self.pending.fetch_sub(1, Ordering::AcqRel);
        }
    }

    /// Whether every save has finished. The folder watch also reports our own writes, and reading
    /// the files back before they are done would undo the newest change.
    pub fn idle(&self) -> bool {
        self.pending.load(Ordering::Acquire) == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    #[test]
    fn runs_jobs_in_order_and_goes_idle() {
        let saver = Saver::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        for i in 0..50 {
            let seen = seen.clone();
            saver.run(move || seen.lock().unwrap().push(i));
        }
        let start = Instant::now();
        while !saver.idle() {
            assert!(start.elapsed() < Duration::from_secs(5), "never went idle");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(*seen.lock().unwrap(), (0..50).collect::<Vec<_>>());
    }
}
