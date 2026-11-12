//! Doing what was clicked, off the UI thread: removing toasts from the store, and handing a click
//! to the toast's app (which may have to start first). Each runs on a short-lived thread.

use shellman_core::notify::{Invoke, Outcome, Source, Toast};
use std::sync::Arc;
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

/// Hand `invoke` to the toast's app, then remove the toast, as Windows does once one is acted
/// on. If the app has no activation handler shellman can reach, its window `front` is brought
/// forward, or the app is started.
pub fn activate(source: Arc<dyn Source>, toast: Toast, invoke: Invoke, front: Option<isize>) {
    std::thread::spawn(move || {
        com();
        match source.activate(&toast, &invoke) {
            Ok(Outcome::NoHandler) => bring_forward(&toast.app_id, front),
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
        Some(hwnd) => crate::dock::Dock::show_window(hwnd),
        // An AppUserModelID (not a program path) opens through the Apps folder, as the dock does.
        None if !app_id.contains('\\') && !app_id.is_empty() => {
            shellman_core::notify::activate::open(&format!("shell:AppsFolder\\{app_id}"));
        }
        None => {}
    }
}
