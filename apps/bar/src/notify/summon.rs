//! Win+N: the switcher's keyboard hook (which already takes the lone Win key) sets a named event
//! when Win+N is pressed while shellman is the shell; a thread here waits on it and asks the
//! host to open or close the list. The thread sleeps in the kernel until then.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows::core::HSTRING;
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};
use windows::Win32::Foundation::WAIT_OBJECT_0;

/// The event's name; the switcher opens the same one (`notifykey`).
pub fn event_name() -> String {
    let instance = std::env::var("SHELLMAN_INSTANCE").unwrap_or_default();
    format!("Local\\shellman-notifications-summon{instance}")
}

/// Start waiting; the flag turns true on each Win+N, and `wake` runs.
pub fn start(wake: impl Fn() + Send + 'static) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    let set = flag.clone();
    let _ = std::thread::Builder::new().name("notifications-summon".into()).spawn(move || {
        let Ok(event) = (unsafe { CreateEventW(None, false, false, &HSTRING::from(event_name())) }) else { return };
        while unsafe { WaitForSingleObject(event, INFINITE) } == WAIT_OBJECT_0 {
            set.store(true, Ordering::Release);
            wake();
        }
    });
    flag
}
