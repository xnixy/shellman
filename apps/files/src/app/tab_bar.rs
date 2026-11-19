//! The tabs, in their row under the header with no pill of their own, as wide as they need and
//! drawn by `shellman_ui::tab_strip` (the look Terminal's tabs share). Many tabs shrink down to a
//! smallest width; past that they scroll sideways with the wheel, and the active tab is brought
//! into view whenever it changes.

use super::actions::Action;
use super::menu::item;
use super::tab::Loc;
use egui::{Id, Rect, Ui};
use shellman_ui::chips::round_icon;
use shellman_ui::menu::popup::Entry;
use shellman_ui::tab_strip::{self as ts, Mark, Tab};
use shellman_ui::theme::{glyph, shape};

/// A tab's right-click menu: a copy of it, and closing it while there are others.
fn tab_entries(loc: &Loc, i: usize, n: usize) -> Vec<Entry<Action>> {
    let mut e = vec![item(Action::NewTab(loc.clone()), glyph::ADD, "Duplicate tab")];
    if n > 1 {
        e.push(item(Action::CloseTab(i), glyph::CLOSE, "Close tab").hint("Ctrl+W"));
    }
    e
}

impl super::FileMan {
    /// Draw the tabs and the + button at the left of `row` (`Plan::tabs_row`). Returns the
    /// strip's extent.
    pub(super) fn tab_strip(&mut self, ui: &mut Ui, row: Rect, actions: &mut Vec<Action>) -> Rect {
        let n = self.tabs.len();
        let mut s = ts::strip(n, 1, row.width(), row.height(), self.tab_scroll);
        if self.tab_seen != (self.active, n) {
            self.tab_seen = (self.active, n);
            s.offset = ts::reveal(&s, self.active);
        }
        let pill = super::plan::from_left(row, s.pill_w);
        let view = ts::view(&s, pill);
        // The wheel scrolls the tabs sideways while the pointer is over them.
        if s.max_offset > 0.0 && ui.rect_contains_pointer(pill) {
            let d = ui.input(|i| i.smooth_scroll_delta);
            let step = if d.x.abs() > d.y.abs() { d.x } else { d.y };
            if step != 0.0 {
                s.offset = (s.offset - step).clamp(0.0, s.max_offset);
            }
        }
        self.tab_scroll = s.offset;
        let clip = ui.clip_rect().intersect(view);
        for i in 0..n {
            let rect = ts::tab_rect(&s, view, i);
            if rect.right() < clip.left() || rect.left() > clip.right() {
                continue;
            }
            let active = i == self.active;
            let loc = self.tabs[i].loc.clone();
            let title = loc.title();
            super::dnd::zone(ui, rect.intersect(clip), super::dnd::Dest::Tab(i), shape::TAB.min(rect.height() / 2.0));
            // A tag place's tab has the tag's coloured dot where the others have a glyph.
            let icon = match loc {
                Loc::Tag(_) => None,
                Loc::Pc => Some(glyph::PC),
                Loc::Bin => Some(glyph::DELETE),
                Loc::Dir(_) => Some(glyph::FOLDER),
            };
            let t = Tab { id: Id::new(("tab", self.tabs[i].id)), title: &title, icon, active, closable: n > 1, mark: Mark::None };
            let r = ts::tab(ui, rect, clip, &t);
            if let Loc::Tag(tag) = loc {
                let at = egui::pos2(rect.left() + 16.0, rect.center().y);
                ui.painter().with_clip_rect(clip).circle_filled(at, 5.0, super::tag_ui::colour(tag as usize));
            }
            if r.close || r.resp.middle_clicked() {
                actions.push(Action::CloseTab(i));
            } else if r.resp.clicked() {
                actions.push(Action::SwitchTab(i));
            }
            let resp = r.resp.on_hover_text(loc.dir().map_or_else(|| loc.title(), |d| d.display().to_string()));
            super::menu::on_right_click(&resp, "tab", || tab_entries(&loc, i, n));
        }
        ts::edges(ui, &s, view);
        if round_icon(ui, ts::button_rect(&s, pill, 0), Id::new("new-tab"), glyph::ADD, true, false, "New tab")
            .on_hover_text("New tab (Ctrl+T)")
            .clicked()
        {
            actions.push(Action::NewTab(self.tabs[self.active].loc.clone()));
        }
        pill
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tabs_menu() {
        let loc = Loc::Dir(std::path::PathBuf::from(r"C:\x"));
        let e = tab_entries(&loc, 2, 3);
        let ids: Vec<String> = e.iter().map(|e| match e { Entry::Item(i) => format!("{:?} {} {}", i.id, i.label, i.hint), _ => "-".into() }).collect();
        assert!(ids[0].starts_with("NewTab") && ids[0].ends_with("Duplicate tab "));
        assert_eq!(ids[1], "CloseTab(2) Close tab Ctrl+W");
        // The last tab cannot be closed from its menu.
        assert_eq!(tab_entries(&loc, 0, 1).len(), 1);
    }
}
