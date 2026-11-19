//! The bar along the bottom of the pane, under a hairline: at its left what is selected (or how
//! many items there are), at its right what can be done to the selection (Open, Copy, Cut,
//! Rename, Move to…, Copy to…, Delete, Properties); with nothing selected, what can be done in
//! the folder (New folder, Paste, Select all, Sort, View). In the Recycle Bin, Restore and Delete
//! for good. When the window is narrow the buttons drop their labels, the least used first (see
//! `fit`).
//!
//! Messages ("Copy done") show in the bar's free stretch between the two, or in the free end of
//! the tabs row, never over a button (see `toast_slot`).

use super::actions::Action;
use super::plan::INSET;
use super::tab::Loc;
use crate::settings::ViewMode;
use egui::{Align2, Color32, FontId, Id, Rect, Ui, Vec2};
use shellman_core::fsops::{self, Op};
use shellman_ui::chips::{chip, chip_width, Chip, CHIP_H};
use shellman_ui::menu::popup::Anchor;
use shellman_ui::theme::{colors, glyph, semibold_font};

const TOAST_SECS: f64 = 3.0;
/// The least room between the text at the bar's left and its buttons.
const PAD: f32 = 16.0;
/// Between the bar's parts.
const SPACE: f32 = 6.0;
/// Between a message and what is beside it.
const TOAST_GAP: f32 = 10.0;
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

/// Which buttons keep their labels when the bar has `room`: `lead` for the text at its start and
/// `full`/`bare` for each button with and without its label, with at least `PAD` between the two.
/// Labels go from the end of the list backwards (the least used are last), but the first
/// button's goes last of all. Returns the labels kept, and the width it all takes.
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

/// Where a message of `w` goes: at the right of `free` (the bar's stretch between its text and
/// its buttons), else in `top` (the free end of the tabs row), else wherever there is most room,
/// cut to fit. Never over the bar's text or buttons.
pub(super) fn toast_slot(free: Rect, top: Rect, w: f32) -> Rect {
    let at = |area: Rect| {
        let h = TOAST_H.min(area.height());
        let w = w.min(area.width()).max(0.0);
        Rect::from_min_size(egui::pos2(area.right() - w, area.center().y - h / 2.0), Vec2::new(w, h))
    };
    let free = Rect::from_min_max(egui::pos2(free.left() + TOAST_GAP, free.top()), egui::pos2(free.right() - TOAST_GAP, free.bottom()));
    let top = Rect::from_min_max(egui::pos2(top.left() + TOAST_GAP, top.top()), top.max);
    if free.width() >= w {
        return at(free);
    }
    if top.width() >= w {
        return at(top);
    }
    at(if top.width() > free.width() { top } else { free })
}

fn text_width(ui: &Ui, text: &str, font: FontId) -> f32 {
    ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, Color32::WHITE))
        .size()
        .x
        .ceil()
}

impl super::FileMan {
    /// The message ("Copy done"), while there is one: in `free`, the bar's free stretch, else in
    /// `top`, the free end of the tabs row.
    pub(super) fn toast_text(&mut self, ui: &mut Ui, free: Rect, top: Rect) {
        let c = colors();
        let now = ui.input(|i| i.time);
        let Some((msg, at)) = &self.toast else { return };
        if now - at >= TOAST_SECS {
            return;
        }
        let font = FontId::proportional(12.5);
        let w = text_width(ui, msg, font.clone());
        let r = toast_slot(free, top, w);
        shellman_ui::widgets::draw_text(ui, r, msg, font, c.accent_text, false);
        let left = TOAST_SECS - (now - at);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(left.max(0.05)));
    }

    /// Draw the bar in `band`, the hairline above it; returns its free stretch, for messages.
    pub(super) fn band(&mut self, ui: &mut Ui, band: Rect, actions: &mut Vec<Action>) -> Rect {
        let c = colors();
        hairline(ui, band);
        let inner = band.shrink2(Vec2::new(INSET, 0.0));
        let tab = &self.tabs[self.active];
        let place = match tab.loc {
            Loc::Pc => {
                let text = match self.linux.len() {
                    0 => format!("{} drives", self.drives.len()),
                    n => format!("{} drives, {n} Linux", self.drives.len()),
                };
                return muted_text(ui, inner, &text);
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
            return muted_text(ui, inner, &lead_text);
        }
        let lead_w = text_width(ui, &lead_text, lead_font.clone());
        let size_w = size
            .as_ref()
            .map_or(0.0, |s| text_width(ui, s, FontId::proportional(13.0)) + SPACE * 2.0);
        let full: Vec<f32> = list
            .iter()
            .map(|b| chip_width(ui, b.label, true, b.kind))
            .collect();
        let lead = lead_w + SPACE * 2.0 + size_w;
        let (labels, _) = fit(lead, &full, ICON_ONLY_W, inner.width());
        let y = inner.center().y;
        let mut x = inner.left();
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
        let lead_end = x;
        // The buttons at the right end.
        let widths: Vec<f32> = full.iter().zip(&labels).map(|(w, &on)| if on { *w } else { ICON_ONLY_W }).collect();
        let buttons_w = widths.iter().sum::<f32>() + SPACE * widths.len().saturating_sub(1) as f32;
        let mut x = (inner.right() - buttons_w).max(lead_end + PAD);
        let free = Rect::from_min_max(egui::pos2(lead_end, band.top()), egui::pos2(x.max(lead_end), band.bottom()));
        for ((b, w), label) in list.into_iter().zip(widths).zip(labels) {
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
            // Icon only when narrow: the name is still the label.
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, b.enabled, b.label));
            let resp = if label { resp } else { resp.on_hover_text(b.label) };
            let clicked = b.enabled && resp.clicked();
            match b.does {
                Does::Sort => {
                    if clicked {
                        let tab = &self.tabs[self.active];
                        let entries = super::controls::sort_entries(tab.sort, tab.ascending, self.settings.show_hidden);
                        super::menu::request(Anchor::over(ui.ctx(), resp.rect), "sort", entries);
                    }
                }
                Does::Act(a) if clicked => actions.push(a),
                Does::Dest(op) if clicked => self.open_dest(op, r),
                _ => {}
            }
        }
        free
    }
}

/// The hairline along the top of the bottom bar `band`, between it and the folder.
pub(super) fn hairline(ui: &Ui, band: Rect) {
    let c = colors();
    let line = if c.high_contrast { c.text } else { c.hairline };
    ui.painter().hline(band.x_range(), band.top() + 0.5, egui::Stroke::new(1.0_f32, line));
}

/// Muted `text` at the left of `inner`; returns the stretch right of it.
fn muted_text(ui: &Ui, inner: Rect, text: &str) -> Rect {
    let c = colors();
    let font = FontId::proportional(12.5);
    let w = text_width(ui, text, font.clone()).min(inner.width());
    shellman_ui::widgets::draw_text(ui, Rect::from_min_size(inner.min, Vec2::new(w, inner.height())), text, font, c.muted, false);
    Rect::from_min_max(egui::pos2(inner.left() + w, inner.top()), inner.max)
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
        // Room between the text and the buttons: there, at its right.
        let free = Rect::from_min_max(egui::pos2(200.0, band.top()), egui::pos2(700.0, band.bottom()));
        let r = toast_slot(free, top, 200.0);
        assert!(free.contains_rect(r) && band.contains_rect(r));
        assert_eq!(r.right(), free.right() - TOAST_GAP);
        // No room there: up in the tabs row.
        let tight = Rect::from_min_max(egui::pos2(200.0, band.top()), egui::pos2(260.0, band.bottom()));
        let r = toast_slot(tight, top, 200.0);
        assert!(top.contains_rect(r));
        // Too long for anywhere: cut to the most room, still clear of the text and buttons.
        let r = toast_slot(free, top, 5000.0);
        assert!(free.contains_rect(r));
    }
}
