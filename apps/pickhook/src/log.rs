//! A few lines about what the hook decided (the picker, or the real dialog and why), for when
//! something needs explaining. Off unless `SHELLMAN_PICKHOOK_LOG` names a file; never fails.

use std::io::Write;

pub const VAR: &str = "SHELLMAN_PICKHOOK_LOG";

/// Append `text` to the log, if there is one.
pub fn line(text: &str) {
    let Some(path) = std::env::var_os(VAR) else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "[{}] {text}", std::process::id());
    }
}
