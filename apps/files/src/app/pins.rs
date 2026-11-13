//! Folders the user pinned to the rail: the list operations, the rail's icons, and which pins
//! point at a folder that is no longer there.

use super::tab::Loc;
use super::rail::{Kind, Spot};
use egui::Id;
use shellman_ui::rail::{Icon, Row};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A path as a key: Windows paths ignore case and a trailing slash.
fn key(p: &Path) -> String {
    p.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase()
}

/// The first letter of a name, as a capital, for a pin's icon.
fn initial(name: &str) -> String {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map_or_else(|| "?".into(), |c| c.to_uppercase().collect())
}

pub(super) fn is_pinned(list: &[PathBuf], p: &Path) -> bool {
    list.iter().any(|x| key(x) == key(p))
}

/// Pin `p` at the end. False when it already was.
pub(super) fn pin(list: &mut Vec<PathBuf>, p: &Path) -> bool {
    if is_pinned(list, p) {
        return false;
    }
    list.push(p.to_path_buf());
    true
}

/// Unpin `p`. False when it was not pinned.
pub(super) fn unpin(list: &mut Vec<PathBuf>, p: &Path) -> bool {
    let before = list.len();
    list.retain(|x| key(x) != key(p));
    list.len() != before
}

/// What the right-click menu of a folder says. The menu is drawn far from the sidebar's state, so
/// the pinned list is left in the context's memory each frame.
pub(super) fn menu_label(ctx: &egui::Context, p: &Path) -> &'static str {
    let pinned: Vec<PathBuf> = ctx
        .data(|d| d.get_temp(Id::new("pinned_folders")))
        .unwrap_or_default();
    if is_pinned(&pinned, p) {
        "Unpin from sidebar"
    } else {
        "Pin to sidebar"
    }
}

/// Which pinned folders exist. Looking is a disk (or network, or cloud) access, so a worker does
/// it, now and then; until an answer arrives a pin counts as there.
#[derive(Default)]
pub(super) struct Exists {
    known: Arc<Mutex<HashMap<String, bool>>>,
    asked_at: f64,
    asked_for: Vec<PathBuf>,
}

impl Exists {
    fn refresh(&mut self, ctx: &egui::Context, pins: &[PathBuf]) {
        let now = ctx.input(|i| i.time);
        if now - self.asked_at < 5.0 && self.asked_for == pins {
            return;
        }
        self.asked_at = now;
        self.asked_for = pins.to_vec();
        let (known, pins, ctx) = (self.known.clone(), pins.to_vec(), ctx.clone());
        std::thread::spawn(move || {
            let found: HashMap<String, bool> = pins.iter().map(|p| (key(p), p.is_dir())).collect();
            let changed = *known.lock().unwrap() != found;
            *known.lock().unwrap() = found;
            if changed {
                ctx.request_repaint();
            }
        });
    }

    fn there(&self, p: &Path) -> bool {
        self.known
            .lock()
            .unwrap()
            .get(&key(p))
            .copied()
            .unwrap_or(true)
    }
}

impl super::FileMan {
    /// The pinned folders' rows on the rail: the folder's initial for its icon (it tells pins
    /// apart when the rail is folded), the path beside it on hover, faded when it is gone.
    pub(super) fn rail_pins(&mut self, ctx: &egui::Context) -> Vec<(Row<'static>, Spot)> {
        let pins = self.settings.pins.clone();
        ctx.data_mut(|d| d.insert_temp(Id::new("pinned_folders"), pins.clone()));
        self.pin_exists.refresh(ctx, &pins);
        pins.into_iter()
            .map(|path| {
                let loc = Loc::Dir(path.clone());
                let there = self.pin_exists.there(&path);
                let title = loc.title();
                let tip = if there {
                    format!("{title}\n{}", path.display())
                } else {
                    format!("{title} (not there any more)\n{}", path.display())
                };
                let row = Row::new(format!("pin:{}", path.display()), Icon::Initial(initial(&title)), title).tip(tip).dim(!there);
                (row, Spot { loc, kind: Kind::Pin { there } })
            })
            .collect()
    }
    pub(super) fn toggle_pin(&mut self, path: PathBuf) {
        if !unpin(&mut self.settings.pins, &path) {
            pin(&mut self.settings.pins, &path);
        }
        self.save_pending = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_and_unpin_ignore_case_and_trailing_slash() {
        let mut list = Vec::new();
        assert!(pin(&mut list, Path::new(r"C:\Work\Proj")));
        assert!(!pin(&mut list, Path::new(r"c:\work\proj\")));
        assert!(pin(&mut list, Path::new(r"D:\Other")));
        assert_eq!(list.len(), 2);
        assert!(is_pinned(&list, Path::new(r"C:\WORK\PROJ")));
        assert!(unpin(&mut list, Path::new(r"c:\work\proj")));
        assert!(!unpin(&mut list, Path::new(r"c:\work\proj")));
        assert_eq!(list, [PathBuf::from(r"D:\Other")]);
    }

    #[test]
    fn a_pin_shows_its_first_letter() {
        assert_eq!(initial("projects"), "P");
        assert_eq!(initial("_build"), "B");
        assert_eq!(initial(""), "?");
    }

    #[test]
    fn order_is_the_order_pinned() {
        let mut list = Vec::new();
        for p in ["C:\\b", "C:\\a", "C:\\c"] {
            pin(&mut list, Path::new(p));
        }
        assert_eq!(list[0], PathBuf::from("C:\\b"));
        assert_eq!(list[2], PathBuf::from("C:\\c"));
    }
}
