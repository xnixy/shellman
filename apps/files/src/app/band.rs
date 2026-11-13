//! The command bar along the bottom: a pill that says what is selected and offers what can be
//! done to it (Open, Copy, Cut, Rename, Move to…, Copy to…, Delete, Properties); with nothing
//! selected, what can be done in the folder (New folder, Paste, Select all, Sort, View). In the
//! Recycle Bin, Restore and Delete for good. When the window is narrow the buttons drop their
//! labels, the least used first (see `fit`).
//!
//! Messages ("Copy done") show in a small pill of their own beside it, or in the free end of
//! the top row, never over a button (see `toast_slot`).

use super::actions::Action;
use super::plan::GAP;
use super::tab::Loc;
use crate::settings::ViewMode;
use egui::{Align2, Color32, FontId, Id, Rect, Ui, Vec2};
use shellman_core::fsops::{self, Op};
use shellman_ui::chips::{chip, chip_width, Chip, CHIP_H};
use shellman_ui::theme::{colors, glyph, semibold_font};

const TOAST_SECS: f64 = 3.0;
/// Inside the pill, at each end, and between its parts.
const PAD: f32 = 8.0;
const LEAD: f32 = 20.0;
const SPACE: f32 = 6.0;
/// A button without its label.
const ICON_ONLY_W: f32 = 38.0;
const TOAST_H: f32 = 32.0;

/// What a button of the bar does when clicked.
#[derive(Clone, Debug)]
pub(super) enum Does {
    Act(Action),
    /// Opens the list of places to move or copy to (see `dest`).
    Dest(Op),
    /// Opens the sort menu.
    Sort,
}

/// One button of the command bar.
pub(super) struct Button {
    pub(super) label: &'static str,
    pub(super) icon: char,
    pub(super) kind: Chip,
    pub(super) does: Does,
    pub(super) enabled: bool,
}

/// Where the bar is, for what it offers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Place {
    Folder,
    Bin,
    /// A tag place: items from anywhere, with no folder of its own to make things in.
    Tag,
}

/// The buttons for `count` selected items in `place`, `clip` saying whether there is something
/// to paste, `view` the current view (the View button switches to the other).
pub(super) fn buttons(place: Place, count: usize, clip: bool, view: ViewMode) -> Vec<Button> {
    let b = |label, icon, kind, does| Button {
        label,
        icon,
        kind,
        does,
        enabled: true,
    };
    let act = Does::Act;
    if place == Place::Bin {
        if count == 0 {
            return Vec::new();
        }
        return vec![
            b("Restore", glyph::UNDO, Chip::Soft, act(Action::BinRestore)),
            b("Delete for good", glyph::DELETE, Chip::Danger, act(Action::BinDelete)),
        ];
    }
    if count == 0 {
        let (view_label, view_icon, other) = match view {
            ViewMode::Grid => ("List", glyph::VIEW_LIST, ViewMode::List),
            ViewMode::List => ("Cards", glyph::VIEW_GRID, ViewMode::Grid),
        };
        let mut list = Vec::new();
        if place == Place::Folder {
            list.push(b("New folder", glyph::NEW_FOLDER, Chip::Soft, act(Action::NewFolder)));
            let mut paste = b("Paste", glyph::PASTE, Chip::Soft, act(Action::Paste));
            paste.enabled = clip;
            list.push(paste);
        }
        list.push(b("Select all", glyph::CHECK, Chip::Soft, act(Action::SelectAll)));
        list.push(b("Sort", glyph::SORT_DOWN, Chip::Soft, Does::Sort));
        list.push(b(view_label, view_icon, Chip::Soft, act(Action::SetView(other))));
        return list;
    }
    let mut list = vec![
        b("Open", glyph::OPEN_EXTERNAL, Chip::Soft, act(Action::OpenSelected)),
        b("Copy", glyph::COPY, Chip::Soft, act(Action::Clip(Op::Copy))),
        b("Cut", glyph::CUT, Chip::Soft, act(Action::Clip(Op::Move))),
    ];
    if count == 1 {
        list.push(b("Rename", glyph::RENAME, Chip::Soft, act(Action::Rename)));
    }
    list.push(b("Move to…", glyph::FORWARD, Chip::Soft, Does::Dest(Op::Move)));
    list.push(b("Copy to…", glyph::COPY, Chip::Soft, Does::Dest(Op::Copy)));
    list.push(b("Delete", glyph::DELETE, Chip::Danger, act(Action::Recycle)));
    list.push(b("Properties", glyph::INFO, Chip::Soft, act(Action::Properties)));
    list
}

/// Which buttons keep their labels when the bar may be `room` wide: `lead` for the text at its
/// start and `full`/`bare` for each button with and without its label. Labels go from the end
/// of the list backwards (the least used are last), but the first button's goes last of all.
/// Returns the labels kept, and the bar's width.
pub(super) fn fit(lead: f32, full: &[f32], bare: f32, room: f32) -> (Vec<bool>, f32) {
    let gaps = SPACE * full.len().saturating_sub(1) as f32;
    let width = |labels: &[bool]| {
        lead + gaps
            + PAD
            + full
                .iter()
                .zip(labels)
                .map(|(w, &on)| if on { *w } else { bare })
                .sum::<f32>()
    };
    let mut labels = vec![true; full.len()];
    // Drop order: the last, then backwards, the first last of all.
    let mut order: Vec<usize> = (1..full.len()).rev().collect();
    if !full.is_empty() {
        order.push(0);
    }
    for i in order {
        if width(&labels) <= room {
            break;
        }
        labels[i] = false;
    }
    let w = width(&labels);
    (labels, w)
}

/// Where a message of `w` goes: beside the bottom `pill` in `band` (right, else left), else in
/// `top` (the free end of the top row), else wherever there is most room, cut to fit. Never over
/// the pill.
pub(super) fn toast_slot(band: Rect, pill: Rect, top: Rect, w: f32) -> Rect {
    let h = TOAST_H.min(band.height());
    let right = Rect::from_min_max(egui::pos2(pill.right() + GAP, band.top()), band.max);
    let left = Rect::from_min_max(band.min, egui::pos2(pill.left() - GAP, band.bottom()));
    let at = |area: Rect, from_right: bool| {
        let w = w.min(area.width()).max(0.0);
        let x = if from_right { area.right() - w } else { area.left() };
        Rect::from_min_size(egui::pos2(x, area.center().y - h / 2.0), Vec2::new(w, h))
    };
    if right.width() >= w {
        return at(right, true);
    }
    if left.width() >= w {
        return at(left, false);
    }
    if top.width() >= w {
        return at(top, true);
    }
    // The most room there is.
    let best = [right, left, top]
        .into_iter()
        .max_by(|a, b| a.width().total_cmp(&b.width()))
        .unwrap_or(right);
    at(best, best != left)
}

fn text_width(ui: &Ui, text: &str, font: FontId) -> f32 {
    ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, Color32::WHITE))
        .size()
        .x
        .ceil()
}

impl super::FileMan {
    /// The message pill ("Copy done"), while there is a message; it is added to `extra`. `band`
    /// is the bottom band, `pill` the command bar in it, `top` the free end of the top row.
    pub(super) fn toast_pill(&mut self, ui: &mut Ui, band: Rect, pill: Rect, top: Rect, extra: &mut Vec<Rect>) {
        let c = colors();
        let now = ui.input(|i| i.time);
        let Some((msg, at)) = &self.toast else { return };
        if now - at >= TOAST_SECS {
            return;
        }
        let font = FontId::proportional(12.5);
        let w = text_width(ui, msg, font.clone()) + 36.0;
        let r = toast_slot(band, pill, top, w);
        shellman_ui::widgets::draw_text(ui, r.shrink2(Vec2::new(16.0, 0.0)), msg, font, c.accent_text, false);
        extra.push(r);
        let left = TOAST_SECS - (now - at);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(left.max(0.05)));
    }

    /// Draw the command bar in `band`; returns its pill, if one shows.
    pub(super) fn band(&mut self, ui: &mut Ui, band: Rect, actions: &mut Vec<Action>) -> Option<Rect> {
        let c = colors();
        let tab = &self.tabs[self.active];
        let place = match tab.loc {
            Loc::Pc => {
                let text = format!("{} drives", self.drives.len());
                return Some(self.muted_pill(ui, band, &text));
            }
            Loc::Bin => Place::Bin,
            Loc::Tag(_) => Place::Tag,
            Loc::Dir(_) => Place::Folder,
        };
        let (count, bytes) = tab.selection_stats();
        let list = buttons(place, count, self.clip.can_paste(), self.settings.view);
        // What the bar says first: the selection and its size, or the count of items.
        let (lead_text, lead_font, lead_color) = if count > 0 {
            (format!("{count} selected"), semibold_font(13.5), c.text)
        } else {
            let n = tab.view.len();
            let mut t = super::text::count(n, "item");
            if tab.loading {
                t += " · Loading…";
            }
            if let Some((n, cut)) = self.clip.summary() {
                let verb = if cut { "cut" } else { "copied" };
                t += &format!(" · {n} {verb}");
            }
            (t, FontId::proportional(12.5), c.muted)
        };
        let size = (count > 0 && bytes > 0).then(|| fsops::format_size(bytes));
        if list.is_empty() {
            return Some(self.muted_pill(ui, band, &lead_text));
        }
        let lead_w = text_width(ui, &lead_text, lead_font.clone());
        let size_w = size
            .as_ref()
            .map_or(0.0, |s| text_width(ui, s, FontId::proportional(13.0)) + SPACE * 2.0);
        let full: Vec<f32> = list
            .iter()
            .map(|b| chip_width(ui, b.label, true, b.kind))
            .collect();
        let lead = LEAD + lead_w + size_w + SPACE * 2.0;
        let (labels, wanted) = fit(lead, &full, ICON_ONLY_W, band.width());
        let pill = super::plan::centred(band, wanted);
        let y = pill.center().y;
        let mut x = pill.left() + LEAD;
        let p = ui.painter();
        p.text(egui::pos2(x, y), Align2::LEFT_CENTER, &lead_text, lead_font, lead_color);
        x += lead_w + SPACE * 2.0;
        if let Some(s) = size {
            p.text(
                egui::pos2(x, y),
                Align2::LEFT_CENTER,
                s,
                FontId::proportional(13.0),
                c.muted,
            );
            x += size_w;
        }
        for ((b, w), label) in list.into_iter().zip(full).zip(labels) {
            let w = if label { w } else { ICON_ONLY_W };
            let r = Rect::from_min_size(egui::pos2(x, y - CHIP_H / 2.0), Vec2::new(w, CHIP_H));
            x += w + SPACE;
            let resp = ui
                .scope(|ui| {
                    if !b.enabled {
                        ui.disable();
                        ui.multiply_opacity(0.4);
                    }
                    let text = if label { b.label } else { "" };
                    chip(ui, r, Id::new(("band", b.label)), text, Some(b.icon), b.kind)
                })
                .inner;
            let resp = if label { resp } else { resp.on_hover_text(b.label) };
            let clicked = b.enabled && resp.clicked();
            match b.does {
                Does::Sort => {
                    if clicked {
                        ui.memory_mut(|m| m.toggle_popup(Id::new("band-sort")));
                    }
                    self.sort_popup(ui, &resp, actions);
                }
                Does::Act(a) if clicked => actions.push(a),
                Does::Dest(op) if clicked => self.open_dest(op, r),
                _ => {}
            }
        }
        Some(pill)
    }

    /// A small muted pill with `text` in the middle of `band`.
    fn muted_pill(&self, ui: &Ui, band: Rect, text: &str) -> Rect {
        let c = colors();
        let font = FontId::proportional(12.5);
        let w = text_width(ui, text, font.clone()) + LEAD * 2.0;
        let pill = Rect::from_center_size(band.center(), Vec2::new(w.min(band.width()), 32.0));
        ui.painter()
            .text(pill.center(), Align2::CENTER_CENTER, text, font, c.muted);
        pill
    }

    /// The sort menu, opening above the bar's Sort button.
    fn sort_popup(&mut self, ui: &mut Ui, resp: &egui::Response, actions: &mut Vec<Action>) {
        egui::popup::popup_above_or_below_widget(
            ui,
            Id::new("band-sort"),
            resp,
            egui::AboveOrBelow::Above,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| self.sort_menu(ui, actions),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(place: Place, count: usize) -> Vec<&'static str> {
        buttons(place, count, false, ViewMode::Grid)
            .iter()
            .map(|b| b.label)
            .collect()
    }

    #[test]
    fn a_selection_offers_what_can_be_done_to_it() {
        assert_eq!(
            labels(Place::Folder, 1),
            ["Open", "Copy", "Cut", "Rename", "Move to…", "Copy to…", "Delete", "Properties"]
        );
        assert!(!labels(Place::Folder, 3).contains(&"Rename"));
        assert_eq!(labels(Place::Bin, 2), ["Restore", "Delete for good"]);
        assert!(labels(Place::Bin, 0).is_empty());
        let delete = buttons(Place::Folder, 2, false, ViewMode::Grid);
        assert!(delete.iter().any(|b| b.label == "Delete" && b.kind == Chip::Danger));
    }

    #[test]
    fn nothing_selected_offers_what_can_be_done_in_the_folder() {
        assert_eq!(labels(Place::Folder, 0), ["New folder", "Paste", "Select all", "Sort", "List"]);
        // Paste only with something to paste.
        let off = buttons(Place::Folder, 0, false, ViewMode::Grid);
        assert!(!off.iter().find(|b| b.label == "Paste").unwrap().enabled);
        let on = buttons(Place::Folder, 0, true, ViewMode::List);
        assert!(on.iter().find(|b| b.label == "Paste").unwrap().enabled);
        assert_eq!(on.last().unwrap().label, "Cards");
        // A tag place has no folder to make things in.
        assert_eq!(labels(Place::Tag, 0), ["Select all", "Sort", "List"]);
    }

    #[test]
    fn labels_drop_from_the_end_when_narrow() {
        let full = [60.0, 70.0, 80.0];
        let (all, w) = fit(100.0, &full, ICON_ONLY_W, 1000.0);
        assert_eq!(all, [true, true, true]);
        assert!(w <= 1000.0);
        // Room for all but one label: the last goes.
        let (some, w) = fit(100.0, &full, ICON_ONLY_W, w - 1.0);
        assert_eq!(some, [true, true, false]);
        // Very narrow: none, the first last.
        let (none, _) = fit(100.0, &full, ICON_ONLY_W, 10.0);
        assert_eq!(none, [false, false, false]);
        let (first_only, w2) = fit(100.0, &full, ICON_ONLY_W, 100.0 + PAD + 2.0 * SPACE + 60.0 + 2.0 * ICON_ONLY_W);
        assert_eq!(first_only, [true, false, false]);
        assert!(w2 <= 100.0 + PAD + 2.0 * SPACE + 60.0 + 2.0 * ICON_ONLY_W + 0.01);
    }

    #[test]
    fn a_message_never_covers_the_bar() {
        let band = Rect::from_min_size(egui::pos2(80.0, 600.0), Vec2::new(1000.0, 44.0));
        let top = Rect::from_min_size(egui::pos2(500.0, 12.0), Vec2::new(300.0, 40.0));
        // A narrow bar: beside it, on the right.
        let pill = Rect::from_center_size(band.center(), Vec2::new(300.0, 44.0));
        let r = toast_slot(band, pill, top, 200.0);
        assert!(!r.intersects(pill) && band.contains_rect(r));
        assert_eq!(r.right(), band.right());
        // A bar as wide as the band: up in the top row.
        let wide = band;
        let r = toast_slot(band, wide, top, 200.0);
        assert!(top.contains_rect(r));
        // Too long for anywhere: cut to the most room, still clear of the bar.
        let r = toast_slot(band, pill, top, 5000.0);
        assert!(!r.intersects(pill));
    }
}
