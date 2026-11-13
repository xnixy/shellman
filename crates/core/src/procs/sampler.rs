//! The sampling thread: one [`Source`] reading per interval, handed to the window, which is woken
//! to show it. While the window is minimised or hidden (`paused` says so) nothing is read; the
//! thread only looks twice a second whether that is still so.

use super::{Sample, Source};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// How often a paused sampler looks whether it may run again.
const PAUSED_CHECK: Duration = Duration::from_millis(500);

struct Shared {
    latest: Mutex<Option<Sample>>,
    interval_ms: AtomicU64,
    stop: AtomicBool,
    /// Set to wake the thread before its wait is over (a new interval, a stop).
    poke: Mutex<bool>,
    woken: Condvar,
}

pub struct Sampler {
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Sampler {
    /// Start sampling every `interval`. `make` builds the source on the sampling thread (the
    /// counters' handles stay on the thread that opened them); `on_sample` is called after each
    /// reading (to wake the window).
    pub fn start<S, M, P, W>(make: M, interval: Duration, paused: P, on_sample: W) -> Self
    where
        S: Source + 'static,
        M: FnOnce() -> S + Send + 'static,
        P: Fn() -> bool + Send + 'static,
        W: Fn() + Send + 'static,
    {
        let shared = Arc::new(Shared {
            latest: Mutex::new(None),
            interval_ms: AtomicU64::new(interval.as_millis() as u64),
            stop: AtomicBool::new(false),
            poke: Mutex::new(false),
            woken: Condvar::new(),
        });
        let s = shared.clone();
        let thread = std::thread::Builder::new()
            .name("procs sampler".into())
            .spawn(move || {
                let making = crate::timing::span("procs: open the source");
                let mut source = make();
                drop(making);
                let mut first = true;
                while !s.stop.load(Ordering::Acquire) {
                    if paused() {
                        s.wait(PAUSED_CHECK);
                        continue;
                    }
                    let timing = first.then(|| crate::timing::span("procs: first reading"));
                    first = false;
                    let sample = source.sample();
                    drop(timing);
                    if let Ok(mut l) = s.latest.lock() {
                        *l = Some(sample);
                    }
                    on_sample();
                    s.wait(Duration::from_millis(s.interval_ms.load(Ordering::Acquire)));
                }
            })
            .ok();
        Self { shared, thread }
    }

    /// The newest reading, if one came since the last call.
    pub fn take(&self) -> Option<Sample> {
        self.shared.latest.lock().ok()?.take()
    }

    pub fn set_interval(&self, interval: Duration) {
        let ms = interval.as_millis() as u64;
        if self.shared.interval_ms.swap(ms, Ordering::AcqRel) != ms {
            self.shared.poke();
        }
    }

    pub fn interval(&self) -> Duration {
        Duration::from_millis(self.shared.interval_ms.load(Ordering::Acquire))
    }
}

impl Shared {
    fn wait(&self, timeout: Duration) {
        let Ok(mut poked) = self.poke.lock() else {
            return;
        };
        if !*poked {
            poked = match self.woken.wait_timeout(poked, timeout) {
                Ok((g, _)) => g,
                Err(_) => return,
            };
        }
        *poked = false;
    }

    fn poke(&self) {
        if let Ok(mut p) = self.poke.lock() {
            *p = true;
            self.woken.notify_all();
        }
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.poke();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct Counting(Arc<AtomicUsize>);

    impl Source for Counting {
        fn sample(&mut self) -> Sample {
            self.0.fetch_add(1, Ordering::SeqCst);
            Sample::default()
        }
    }

    #[test]
    fn samples_until_dropped() {
        let n = Arc::new(AtomicUsize::new(0));
        let woke = Arc::new(AtomicUsize::new(0));
        let (n2, w2) = (n.clone(), woke.clone());
        let s = Sampler::start(
            move || Counting(n2),
            Duration::from_millis(10),
            || false,
            move || {
                w2.fetch_add(1, Ordering::SeqCst);
            },
        );
        std::thread::sleep(Duration::from_millis(150));
        assert!(s.take().is_some());
        drop(s);
        let after = n.load(Ordering::SeqCst);
        assert!(after >= 3, "{after}");
        assert_eq!(woke.load(Ordering::SeqCst), after);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(n.load(Ordering::SeqCst), after, "nothing after the drop");
    }

    #[test]
    fn a_paused_sampler_reads_nothing() {
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        let s = Sampler::start(move || Counting(n2), Duration::from_millis(10), || true, || {});
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(n.load(Ordering::SeqCst), 0);
        assert!(s.take().is_none());
    }

    #[test]
    fn a_new_interval_takes_effect_at_once() {
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        let s = Sampler::start(move || Counting(n2), Duration::from_secs(60), || false, || {});
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(n.load(Ordering::SeqCst), 1);
        s.set_interval(Duration::from_millis(10));
        std::thread::sleep(Duration::from_millis(100));
        assert!(n.load(Ordering::SeqCst) >= 3);
        assert_eq!(s.interval(), Duration::from_millis(10));
    }
}
