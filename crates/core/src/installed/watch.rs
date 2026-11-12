//! Noticing that a program was installed or removed: the three Uninstall keys gain or lose a
//! subkey. It sleeps on Windows' own change notifications (the startup watcher's machinery), so
//! nothing runs while nothing changes.

use crate::startup::watch::{RunWatch, Targets};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

fn targets() -> Targets {
    let keys = super::classic::PLACES
        .iter()
        .map(|(_, hive, key)| {
            let root = match hive {
                crate::registry::Hive::Hkcu => HKEY_CURRENT_USER,
                crate::registry::Hive::Hklm => HKEY_LOCAL_MACHINE,
            };
            (root, (*key).to_string(), false)
        })
        .collect();
    Targets { keys, dirs: Vec::new() }
}

/// Calls `on_change` once the Uninstall keys have been quiet for a moment after a change.
pub fn start(on_change: impl Fn() + Send + 'static) -> RunWatch {
    RunWatch::start_with(targets, on_change)
}
