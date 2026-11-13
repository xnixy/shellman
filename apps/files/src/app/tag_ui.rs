//! Tags in the window: the store kept by the app (saved whenever it changes), the small dots on
//! cards and rows, and the rail's tag places, which list every item with a tag (`Loc::Tag`).

use super::actions::Action;
use super::tab::Loc;
use crate::tags::{Store, COLOURS, COUNT};
use egui::{Color32, Painter, Pos2, Ui};
use std::path::{Path, PathBuf};

/// The tags' names as last saved, for titles drawn far from the store (a tab's, the window's).
static NAMES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Tag `i`'s name, as `set_names` last left it.
pub(super) fn name_of(i: usize) -> String {
    NAMES
        .lock()
        .ok()
        .and_then(|n| n.get(i).cloned())
        .unwrap_or_else(|| Store::default().name(i).to_owned())
}

fn set_names(store: &Store) {
    if let Ok(mut n) = NAMES.lock() {
        n.clone_from(&store.names);
    }
}

/// One dot's size and the room between dots.
const DOT: f32 = 8.0;
const DOT_GAP: f32 = 3.0;

/// A tag's colour.
pub(super) fn colour(i: usize) -> Color32 {
    let [r, g, b] = COLOURS[i.min(COUNT - 1)];
    Color32::from_rgb(r, g, b)
}

/// How wide the dots for `mask` are, with room before them; 0 for none.
pub(super) fn dots_width(mask: u8) -> f32 {
    let n = mask.count_ones() as f32;
    if n == 0.0 {
        0.0
    } else {
        n * DOT + (n - 1.0) * DOT_GAP + 6.0
    }
}

/// The dots for `mask`, ending at `right` (or starting there, when not `from_right`), centred on
/// its height.
pub(super) fn paint_dots(p: &Painter, at: Pos2, mask: u8, from_right: bool) {
    let n = mask.count_ones() as f32;
    if n == 0.0 {
        return;
    }
    let width = n * DOT + (n - 1.0) * DOT_GAP;
    let mut x = if from_right { at.x - width } else { at.x };
    for i in 0..COUNT {
        if mask & (1 << i) != 0 {
            p.circle_filled(egui::pos2(x + DOT / 2.0, at.y), DOT / 2.0, colour(i));
            x += DOT + DOT_GAP;
        }
    }
}

/// The store, as the app keeps it: saved at once whenever it changes.
pub(crate) struct Tags {
    pub(super) store: Store,
    file: Option<PathBuf>,
}

impl Tags {
    pub(super) fn load() -> Self {
        let file = Store::file();
        let store = file.as_deref().map(Store::load).unwrap_or_default();
        set_names(&store);
        Self { store, file }
    }

    pub(super) fn mask(&self, p: &Path) -> u8 {
        self.store.mask(p)
    }

    /// Save after a change; a failure is only said, never fatal.
    pub(super) fn save(&self) -> Result<(), String> {
        match &self.file {
            Some(f) => self.store.save(f).map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }
}

/// The background check of tagged items (see `tags::check`): asked for when something may have
/// moved them, run on a worker, at most every `CHECK_EVERY` seconds.
#[derive(Default)]
pub(super) struct Checking {
    pub(super) running: bool,
    wanted: bool,
    last: Option<f64>,
}

const CHECK_EVERY: f64 = 2.0;

impl super::FileMan {
    /// Ask for a check of the tagged items.
    pub(super) fn want_tag_check(&mut self) {
        self.tag_check.wanted = true;
    }

    /// Start a check that was asked for, when none is running and the last was long enough ago.
    pub(super) fn run_tag_check(&mut self, ctx: &egui::Context) {
        let c = &mut self.tag_check;
        if !c.wanted || c.running {
            return;
        }
        if self.tags.store.items.is_empty() {
            c.wanted = false;
            return;
        }
        let now = ctx.input(|i| i.time);
        if let Some(last) = c.last {
            if now - last < CHECK_EVERY {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(CHECK_EVERY - (now - last)));
                return;
            }
        }
        c.wanted = false;
        c.running = true;
        c.last = Some(now);
        let items: Vec<(String, crate::tags::Item)> =
            self.tags.store.items.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            use crate::tags::check;
            let results = check::run(items, &check::RealDisk);
            let _ = tx.send(super::Msg::TagCheck(results));
            ctx.request_repaint();
        });
    }

    /// Take in what a check found; `relist` lists the open tag places again when it changed
    /// anything (not when the check came from listing one).
    pub(super) fn tag_results(&mut self, results: &[(String, PathBuf, crate::tags::check::Verdict)], relist: bool) {
        if !self.tags.store.apply(results, crate::tags::now_secs()) {
            return;
        }
        set_names(&self.tags.store);
        if let Err(e) = self.tags.save() {
            self.toast(format!("Couldn't save the tags: {e}"));
        }
        if relist {
            for i in 0..self.tabs.len() {
                if matches!(self.tabs[i].loc, Loc::Tag(_)) {
                    self.load(i);
                }
            }
        }
    }

    /// Flip tag `i` on every selected item (all on when any lacks it, else all off).
    pub(super) fn toggle_tag(&mut self, i: usize) {
        let paths: Vec<PathBuf> = self.tabs[self.active]
            .selection()
            .into_iter()
            .map(|e| e.path)
            .collect();
        if paths.is_empty() || i >= COUNT {
            return;
        }
        let changed = if let [one] = paths.as_slice() {
            self.tags.store.toggle(one, i)
        } else {
            let on = paths.iter().any(|p| self.tags.mask(p) & (1 << i) == 0);
            let mut changed = false;
            for p in &paths {
                changed |= self.tags.store.set(p, i, on);
            }
            changed
        };
        if changed {
            self.tags_changed();
        }
    }

    /// Take every tag off the selection.
    pub(super) fn clear_tags(&mut self) {
        let mut changed = false;
        for p in self.selected_paths() {
            changed |= self.tags.store.clear(&p);
        }
        if changed {
            self.tags_changed();
        }
    }

    /// Leave the tags' names, and which the selection has, where the right-click menu reads them.
    pub(super) fn tag_menu_state(&self) {
        let paths = self.selected_paths();
        let state: Vec<(String, bool)> = (0..COUNT)
            .map(|i| {
                let on = !paths.is_empty() && paths.iter().all(|p| self.tags.mask(p) & (1 << i) != 0);
                (self.tags.store.name(i).to_owned(), on)
            })
            .collect();
        self.ctx.data_mut(|d| d.insert_temp(egui::Id::new("tag_menu_state"), state));
    }

    pub(super) fn tags_changed(&mut self) {
        // New items' identities are learned soon after.
        self.want_tag_check();
        set_names(&self.tags.store);
        if let Err(e) = self.tags.save() {
            self.toast(format!("Couldn't save the tags: {e}"));
        }
        // A tag place that is open lists again.
        for i in 0..self.tabs.len() {
            if matches!(self.tabs[i].loc, Loc::Tag(_)) {
                self.load(i);
            }
        }
    }

    /// The tag places on the rail: one dot per tag that is on anything.
    pub(super) fn rail_tags(&mut self, ui: &mut Ui, current: &Loc, actions: &mut Vec<Action>) {
        let used = self.tags.store.used();
        for i in 0..COUNT {
            if used & (1 << i) == 0 && *current != Loc::Tag(i as u8) {
                continue;
            }
            let loc = Loc::Tag(i as u8);
            let name = self.tags.store.name(i).to_owned();
            let resp = super::rail::item(ui, ("tag", i), *current == loc, false, |p, square, _| {
                p.circle_filled(square.center(), 6.0, colour(i));
            });
            super::rail::label(ui, &resp, &name);
            if resp.clicked() {
                actions.push(Action::Navigate(loc.clone()));
            }
            if resp.middle_clicked() {
                actions.push(Action::NewTab(loc));
            }
        }
    }

    /// How many tag places the rail shows.
    pub(super) fn rail_tag_count(&self) -> usize {
        let used = self.tags.store.used();
        (0..COUNT)
            .filter(|&i| used & (1 << i) != 0 || self.tabs[self.active].loc == Loc::Tag(i as u8))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_take_room_only_when_there_are_tags() {
        assert_eq!(dots_width(0), 0.0);
        assert_eq!(dots_width(0b1), DOT + 6.0);
        assert_eq!(dots_width(0b101), 2.0 * DOT + DOT_GAP + 6.0);
    }
}
