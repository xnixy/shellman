//! What the live system watchers (`audio`, `radios`, `brightness`, `media`) share: a mailbox
//! for the newest reading, a way to wake the UI, and a COM apartment for their threads.
//!
//! Each watcher runs on a thread of its own. It sleeps in a blocking receive until a command or
//! a Windows callback arrives, reads the state once, and puts it in its mailbox; the UI takes it
//! on its next frame. Nothing polls, so a watcher costs nothing while nothing changes, and
//! dropping it ends its thread and releases what it held.

use std::sync::{Arc, Mutex};

/// Called from a watcher's thread whenever there is something new to show. For an egui app,
/// `move || ctx.request_repaint()`.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// The newest reading a worker has published and the UI has not yet taken. Older readings are
/// simply replaced: the UI only ever wants the latest.
pub struct Mailbox<T> {
    slot: Mutex<Option<T>>,
}

impl<T> Default for Mailbox<T> {
    fn default() -> Self {
        Self { slot: Mutex::new(None) }
    }
}

impl<T> Mailbox<T> {
    pub fn put(&self, value: T) {
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(value);
        }
    }

    /// The reading put since the last `take`, if any.
    pub fn take(&self) -> Option<T> {
        self.slot.lock().ok().and_then(|mut slot| slot.take())
    }
}

/// Join the multithreaded COM apartment on this worker thread. WinRT objects and the audio
/// service's callbacks then reach it without needing a message loop.
pub fn init_mta() {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    // SAFETY: plain FFI call; an already initialised thread is fine.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

/// Spawn a named worker thread. A thread that cannot start leaves its watcher inert, which
/// reads as "nothing to show", never as a crash.
pub fn spawn(name: &str, body: impl FnOnce() + Send + 'static) {
    let _ = std::thread::Builder::new().name(name.into()).spawn(body);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_keeps_only_the_newest() {
        let m = Mailbox::default();
        assert_eq!(m.take(), None);
        m.put(1);
        m.put(2);
        assert_eq!(m.take(), Some(2));
        assert_eq!(m.take(), None);
    }
}
