//! The right-click menu of an app (in the grid or the results) and of the "Run" row: shellman's
//! glass popup menu, a window of its own. Pure here: what it offers; `app` opens it and acts.
//!
//! What only the shell and the disk know about an app (whether it can run as administrator,
//! which dock pin it is, whether that is pinned, where its program is) is read on a worker
//! ([`facts`]); the menu opens at once with "Open" and gets the rest when the answer comes.

use shellman_core::apps_index::{admin, pin, App};
use shellman_core::settings::dock::{DockSettings, Pin};
use shellman_ui::menu::popup::Entry;
use shellman_ui::theme::glyph;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    Open,
    Admin,
    Pin,
    Unpin,
    /// Open the folder the program is in, in Files, with it selected.
    Location,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    /// It can be started as administrator (a program or a desktop Store app).
    pub admin: bool,
    /// Its dock pin, when the dock could hold it, and whether it is there now.
    pub pin: Option<Pin>,
    pub pinned: bool,
    /// The program (or shortcut) file, when it is one.
    pub location: Option<PathBuf>,
}

/// Read an app's facts. Asks the shell and reads the dock's file: a worker's job.
pub fn facts(app: &App) -> Facts {
    // Files is always in the dock, and the launcher is not an app to pin.
    let pin = if app.own && app.id.to_lowercase().ends_with("shellman-files.exe") { None } else { pin::pin_of(app) };
    let pinned = pin.as_ref().is_some_and(|p| DockSettings::load().contains(&p.app_id));
    let location = pin
        .as_ref()
        .and_then(|p| p.exe.clone())
        .or_else(|| app.direct.then(|| PathBuf::from(&app.id)))
        .filter(|p| p.is_file());
    Facts { admin: admin::can_elevate(&app.shell_path()), pin, pinned, location }
}

/// Pin or unpin in the dock's file, which the dock follows. Reads and writes: a worker's job.
/// A file that cannot be read is left alone.
pub fn set_pinned(pin: &Pin, pinned: bool) {
    let Ok(mut dock) = DockSettings::try_load() else { return };
    if pinned {
        dock.add(&pin.app_id, pin.exe.as_deref());
    } else {
        dock.remove(&pin.app_id);
    }
    dock.save();
}

/// An app's menu; `facts` is None until they are read.
pub fn app_entries(facts: Option<&Facts>) -> Vec<Entry<Pick>> {
    let mut e = vec![Entry::item(Pick::Open, "Open").icon(glyph::OPEN_EXTERNAL)];
    let Some(f) = facts else {
        e.push(Entry::note("Looking…"));
        return e;
    };
    if f.admin {
        e.push(Entry::item(Pick::Admin, "Run as administrator").icon(glyph::ADMIN).hint("Ctrl+Shift+Enter"));
    }
    if f.pin.is_some() || f.location.is_some() {
        e.push(Entry::separator());
    }
    if f.pin.is_some() {
        e.push(if f.pinned {
            Entry::item(Pick::Unpin, "Unpin from dock").icon(glyph::UNPIN)
        } else {
            Entry::item(Pick::Pin, "Pin to dock").icon(glyph::PIN)
        });
    }
    if f.location.is_some() {
        e.push(Entry::item(Pick::Location, "Open file location").icon(glyph::FOLDER));
    }
    e
}

/// The "Run" row's menu.
pub fn run_entries() -> Vec<Entry<Pick>> {
    vec![
        Entry::item(Pick::Open, "Run").icon(glyph::OPEN_EXTERNAL).hint("Enter"),
        Entry::item(Pick::Admin, "Run as administrator").icon(glyph::ADMIN).hint("Ctrl+Shift+Enter"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picks(e: &[Entry<Pick>]) -> Vec<Pick> {
        e.iter().filter_map(|e| if let Entry::Item(i) = e { Some(i.id) } else { None }).collect()
    }

    #[test]
    fn an_app_offers_what_it_can_do() {
        // Before the facts: Open alone.
        assert_eq!(picks(&app_entries(None)), [Pick::Open]);
        let pin = Pin { app_id: "c:\\a.exe".into(), exe: Some("C:\\a.exe".into()) };
        let program = Facts { admin: true, pin: Some(pin.clone()), pinned: false, location: Some("C:\\a.exe".into()) };
        assert_eq!(picks(&app_entries(Some(&program))), [Pick::Open, Pick::Admin, Pick::Pin, Pick::Location]);
        let pinned = Facts { pinned: true, ..program };
        assert_eq!(picks(&app_entries(Some(&pinned))), [Pick::Open, Pick::Admin, Pick::Unpin, Pick::Location]);
        // A sandboxed Store app: no administrator, no file.
        let store = Facts { admin: false, pin: Some(pin), pinned: false, location: None };
        assert_eq!(picks(&app_entries(Some(&store))), [Pick::Open, Pick::Pin]);
        assert_eq!(picks(&app_entries(Some(&Facts::default()))), [Pick::Open]);
        assert_eq!(picks(&run_entries()), [Pick::Open, Pick::Admin]);
    }
}
