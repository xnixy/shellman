//! Folders the user pinned to the rail: the list operations, the rail's icons, and which pins
//! point at a folder that is no longer there.

use super::actions::Action;
use super::tab::Loc;
use egui::{Align2, Id, Ui};
use shellman_ui::theme::{glyph, semibold_font};
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
    /// The pinned folders, as icons on the rail; their names and paths show on hover.
    pub(super) fn rail_pins(&mut self, ui: &mut Ui, current: &Loc, actions: &mut Vec<Action>) {
        let pins = self.settings.pins.clone();
        ui.ctx()
            .data_mut(|d| d.insert_temp(Id::new("pinned_folders"), pins.clone()));
        self.pin_exists.refresh(ui.ctx(), &pins);
        for path in pins {
            let loc = Loc::Dir(path.clone());
            let there = self.pin_exists.there(&path);
            let tip = if there {
                format!("{}\n{}", loc.title(), path.display())
            } else {
                format!("{} (not there any more)\n{}", loc.title(), path.display())
            };
            let resp = super::rail::item(
                ui,
                ("pin", &path),
                *current == loc,
                !there,
                |p, square, fg| {
                    // The folder's initial tells pins apart; the name shows on hover.
                    p.text(
                        square.center(),
                        Align2::CENTER_CENTER,
                        initial(&loc.title()),
                        semibold_font(15.0),
                        fg,
                    );
                },
            );
            super::rail::label(ui, &resp, &tip);
            if there {
                super::dnd::zone(ui, resp.rect, super::dnd::Dest::Dir(path.clone()), 14.0);
            }
            if resp.clicked() {
                if there {
                    actions.push(Action::Navigate(loc.clone()));
                } else {
                    self.toast(format!("{} is not there any more", path.display()));
                }
            }
            if resp.middle_clicked() && there {
                actions.push(Action::NewTab(loc.clone()));
            }
            resp.context_menu(|ui| {
                super::controls::flat_items(ui);
                if there && super::controls::mi(ui, glyph::ADD, "Open in new tab", "") {
                    actions.push(Action::NewTab(loc.clone()));
                    ui.close_menu();
                }
                if super::controls::mi(ui, glyph::UNPIN, "Unpin from sidebar", "") {
                    actions.push(Action::TogglePin(path.clone()));
                    ui.close_menu();
                }
            });
        }
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
