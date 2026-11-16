//! Win+1 ... Win+9: the switcher's keyboard hook sets one of nine named events (`dockkey` there)
//! while shellman is the shell; a thread here waits on all of them, notes the number and asks the
//! host for a frame, which opens that pinned app (`Dock::win_key`). The thread sleeps in the
//! kernel until then.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use windows::core::HSTRING;
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{CreateEventW, WaitForMultipleObjects, INFINITE};

/// The event for number `n`; the switcher opens the same one.
pub fn event_name(instance: &str, n: u8) -> String {
    format!("Local\\shellman-dock-key{instance}-{n}")
}

/// Start waiting. The number pressed last (1 to 9) is left in the returned cell, 0 once taken;
/// `wake` runs after each.
pub fn start(wake: impl Fn() + Send + 'static) -> Arc<AtomicU8> {
    let pressed = Arc::new(AtomicU8::new(0));
    let set = pressed.clone();
    let _ = std::thread::Builder::new().name("dock-keys".into()).spawn(move || {
        let instance = std::env::var("SHELLMAN_INSTANCE").unwrap_or_default();
        let events: Vec<HANDLE> = (1..=9)
            .filter_map(|n| unsafe { CreateEventW(None, false, false, &HSTRING::from(event_name(&instance, n))) }.ok())
            .collect();
        if events.len() != 9 {
            return;
        }
        loop {
            let at = unsafe { WaitForMultipleObjects(&events, false, INFINITE) }.0.wrapping_sub(WAIT_OBJECT_0.0);
            let Some(n) = number_for(at) else { return };
            set.store(n, Ordering::Release);
            wake();
        }
    });
    pressed
}

/// The number whose event a wait answered with (its index among the nine), or None for a
/// failed wait.
fn number_for(index: u32) -> Option<u8> {
    (index < 9).then(|| index as u8 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_map_to_numbers() {
        assert_eq!(number_for(0), Some(1));
        assert_eq!(number_for(8), Some(9));
        assert_eq!(number_for(9), None);
        assert_eq!(number_for(u32::MAX), None);
    }

    #[test]
    fn names_match_the_switchers() {
        assert_eq!(event_name("", 1), "Local\\shellman-dock-key-1");
        assert_eq!(event_name("test", 9), "Local\\shellman-dock-keytest-9");
    }
}
