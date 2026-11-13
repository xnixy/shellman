//! Reading the services off the window's thread: one reading at a time, started when asked, the
//! newest handed over when it is done (and the window woken to show it).

use super::{native, Service};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Descriptions already read, by service name: they do not change while the list is open, and
/// the indirect ones (`@%SystemRoot%\...`) cost a resource load each.
pub type Cache = HashMap<String, Option<String>>;

type ReadFn = dyn Fn(&mut Cache) -> Vec<Service> + Send + Sync;

pub struct Reader {
    latest: Arc<Mutex<Option<Vec<Service>>>>,
    busy: Arc<AtomicBool>,
    cache: Arc<Mutex<Cache>>,
    read: Arc<ReadFn>,
}

impl Default for Reader {
    fn default() -> Self {
        Self::with(native::read)
    }
}

impl Reader {
    /// A reader that reads with `read` (the real one is [`native::read`]).
    pub fn with(read: impl Fn(&mut Cache) -> Vec<Service> + Send + Sync + 'static) -> Self {
        Self {
            latest: Arc::default(),
            busy: Arc::default(),
            cache: Arc::default(),
            read: Arc::new(read),
        }
    }

    /// Start a reading on a thread of its own, unless one is still going. `done` is called from
    /// that thread when the reading is ready. True when one was started.
    pub fn request(&self, done: impl FnOnce() + Send + 'static) -> bool {
        if self.busy.swap(true, Ordering::AcqRel) {
            return false;
        }
        let (latest, busy, cache, read) = (self.latest.clone(), self.busy.clone(), self.cache.clone(), self.read.clone());
        let spawned = std::thread::Builder::new().name("services reader".into()).spawn(move || {
            let services = match cache.lock() {
                Ok(mut c) => read(&mut c),
                Err(_) => Vec::new(),
            };
            if let Ok(mut l) = latest.lock() {
                *l = Some(services);
            }
            busy.store(false, Ordering::Release);
            done();
        });
        if spawned.is_err() {
            self.busy.store(false, Ordering::Release);
        }
        spawned.is_ok()
    }

    /// The newest reading, if one came since the last call.
    pub fn take(&self) -> Option<Vec<Service>> {
        self.latest.lock().ok()?.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;
    use std::time::Duration;

    fn svc(name: &str) -> Service {
        Service { name: name.into(), display: name.into(), ..Default::default() }
    }

    #[test]
    fn a_reading_is_handed_over_and_the_window_woken() {
        let r = Reader::with(|_| vec![svc("a"), svc("b")]);
        let (tx, rx) = mpsc::channel();
        assert!(r.request(move || tx.send(()).unwrap()));
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(r.take().unwrap().len(), 2);
        assert!(r.take().is_none(), "taken once");
    }

    #[test]
    fn only_one_reading_runs_at_a_time() {
        let (release, wait) = mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let started = Arc::new(AtomicUsize::new(0));
        let s = started.clone();
        let r = Reader::with(move |_| {
            s.fetch_add(1, Ordering::SeqCst);
            let _ = wait.lock().unwrap().recv_timeout(Duration::from_secs(5));
            vec![svc("a")]
        });
        let (tx, rx) = mpsc::channel();
        assert!(r.request(move || tx.send(()).unwrap()));
        assert!(!r.request(|| {}), "one is still going");
        release.send(()).unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(started.load(Ordering::SeqCst), 1);
        assert!(r.request(|| {}), "free again once it is done");
    }

    #[test]
    fn the_cache_lives_across_readings() {
        let r = Reader::with(|cache| {
            let n = cache.len();
            cache.insert(format!("k{n}"), None);
            vec![svc("a")]
        });
        for expect in [1usize, 2] {
            let (tx, rx) = mpsc::channel();
            assert!(r.request(move || tx.send(()).unwrap()));
            rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(r.cache.lock().unwrap().len(), expect);
        }
    }
}
