//! Doing what was clicked, off the UI thread: removing toasts from the store, and handing a click
//! to the toast's app (which may have to start first). Each runs on a short-lived thread.

use super::window;
use shellman_core::notify::{Activation, Invoke, Outcome, Source, Toast};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

fn com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

/// Remove these toasts from the store (gone for good, as Windows' own ✕ does).
pub fn dismiss(source: Arc<dyn Source>, ids: Vec<u32>) {
    std::thread::spawn(move || {
        com();
        for id in ids {
            if let Err(e) = source.dismiss(id) {
                eprintln!("notifications: dismiss {id}: {e}");
            }
        }
    });
}

/// The apps with windows: (app id, program, front-most window), as the dock groups them.
pub struct Running(pub Vec<(String, Option<PathBuf>, isize)>);

impl Running {
    /// The window of the app a toast names: the one grouped under that id, else (a window that
    /// declares no id is grouped by its program's path) one whose program lives where the app's
    /// Start menu shortcut points. The second walks the Start menu: worker threads only.
    fn window_of(&self, app_id: &str) -> Option<isize> {
        if let Some((_, _, hwnd)) = self.0.iter().find(|(id, _, _)| id.eq_ignore_ascii_case(app_id)) {
            return Some(*hwnd);
        }
        let dir = shellman_core::notify::activate::program_dir(app_id)?;
        self.0
            .iter()
            .find(|(_, exe, _)| exe.as_deref().is_some_and(|e| shellman_core::notify::activate::in_program_dir(e, &dir)))
            .map(|(_, _, hwnd)| *hwnd)
    }
}

/// How long an app that was handed a click gets to come forward by itself before its window
/// is brought forward for it.
const SETTLE: Duration = Duration::from_millis(250);

/// Hand `invoke` to the toast's app, then remove the toast, as Windows does once one is acted
/// on. If the app has no activation handler shellman can reach, its window `front` is brought
/// forward, or the app is started. An app that took the click but did not come forward (many
/// only open the right page, and Windows would have raised them) has `front` raised too.
pub fn activate(source: Arc<dyn Source>, toast: Toast, invoke: Invoke, running: Running) {
    std::thread::spawn(move || {
        com();
        let front = running.window_of(&toast.app_id);
        match source.activate(&toast, &invoke) {
            Ok(Outcome::NoHandler) => bring_forward(&toast.app_id, front),
            Ok(Outcome::Delivered) if invoke.kind == Activation::Foreground => {
                if let Some(hwnd) = front {
                    std::thread::sleep(SETTLE);
                    if !window::in_front_is_of(hwnd) {
                        window::raise(hwnd);
                    }
                }
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("notifications: activate {}: {e}", toast.app_id);
                bring_forward(&toast.app_id, front);
            }
        }
        if let Err(e) = source.dismiss(toast.id) {
            eprintln!("notifications: dismiss {}: {e}", toast.id);
        }
    });
}

fn bring_forward(app_id: &str, front: Option<isize>) {
    match front {
        Some(hwnd) => window::raise(hwnd),
        // An AppUserModelID (not a program path) opens through the Apps folder, as the dock does.
        None if !app_id.contains('\\') && !app_id.is_empty() => {
            shellman_core::notify::activate::open(&format!("shell:AppsFolder\\{app_id}"));
        }
        None => {}
    }
}
