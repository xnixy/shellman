//! A few lines about what the hook decided (the picker, or the real dialog and why), for when
//! something needs explaining. Off unless `SHELLMAN_PICKHOOK_LOG` names a file or a
//! `pickhook.log` already exists in shellman's program folder; never fails.

use std::io::Write;

pub const VAR: &str = "SHELLMAN_PICKHOOK_LOG";

/// Append `text` to the log, if there is one.
pub fn line(text: &str) {
    // Without the variable: a `pickhook.log` the user put next to shellman's programs turns the
    // log on for every app, with no need to start each one in a special way.
    let (path, create) = match std::env::var_os(VAR) {
        Some(p) => (std::path::PathBuf::from(p), true),
        None => match std::env::var_os("LOCALAPPDATA") {
            Some(d) => (std::path::Path::new(&d).join(r"Programs\shellman\pickhook.log"), false),
            None => return,
        },
    };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(create).append(true).open(path) {
        let _ = writeln!(f, "[{}] {text}", std::process::id());
    }
}

/// Note how a getter the app called turned out (the calls an app makes from `OnFileOk` are what
/// decide whether it takes the choice).
pub fn trace<T>(name: &str, r: windows_core::Result<T>) -> windows_core::Result<T> {
    match &r {
        Ok(_) => line(&format!("  {name}: ok")),
        Err(e) => line(&format!("  {name}: {:?}", e.code())),
    }
    r
}
