//! Runs the apps that are started on demand (Files, Settings, Task Manager, the terminal) once,
//! invisibly, so the first real start is not the one Windows Defender scans (`core::warm`:
//! 1.2 to 2.3 s the first time a new exe runs, 0.2 s after). After the fleet is up and the
//! sign-in rush is over, one at a time, on a thread the session never waits on.

use crate::log;
use std::time::Duration;

/// The residents start first; this waits for them to settle.
const AFTER: Duration = Duration::from_secs(4);

pub fn start() {
    let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(std::path::Path::to_path_buf)) else { return };
    let _ = std::thread::Builder::new().name("warm".into()).spawn(move || {
        std::thread::sleep(AFTER);
        let said: Vec<String> = shellman_core::warm::run_all(&dir)
            .into_iter()
            .map(|(name, ran)| match ran {
                Ok(took) => format!("{name} {} ms", took.as_millis()),
                Err(e) => format!("{name} not run ({e})"),
            })
            .collect();
        log::line(&format!("warm: {}", said.join(", ")));
    });
}
