//! The bar under the tabs: back, forward, up and refresh, the path as chips (the current folder
//! lit with the accent), the search field, and the view switch (or the bin's buttons).
//!
//! Its empty stretch is the window's to drag; clicking just after the path, or Ctrl+L, types a
//! path instead.

use super::actions::Action;
use super::tab::Loc;
use crate::settings::ViewMode;
use egui::{Align, Align2, CornerRadius, FontId, Id, Key, Rect, RichText, Sense, Stroke, Ui, Vec2};
use shellman_core::fsops::Drive;
use shellman_ui::chips::{chip, chip_width, round_icon, soft_colors, Chip, CHIP_H};
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::select_all;
use std::path::{Path, PathBuf};

/// Inside the bar, at each end.
const PAD: f32 = 9.0;
const BUTTON: f32 = 34.0;
const SEARCH_W: f32 = 230.0;
const TOGGLE_W: f32 = 76.0;
/// The room after the last crumb where a click starts typing a path.
const TYPE_ZONE: f32 = 56.0;
const SEP_W: f32 = 16.0;

impl super::FileMan {
    pub(super) fn toolbar(&mut self, ui: &mut Ui, bar: Rect, actions: &mut Vec<Action>) {
        let tab = &self.tabs[self.active];
        let enabled = [
            !tab.back.is_empty(),
            !tab.fwd.is_empty(),
            tab.loc.parent().is_some(),
            true,
        ];
        let nav = [
            (glyph::BACK, "Back (Alt+Left)", Action::Back),
            (glyph::FORWARD, "Forward (Alt+Right)", Action::Forward),
            (glyph::UP, "Up (Alt+Up)", Action::Up),
            (glyph::REFRESH, "Refresh (F5)", Action::Refresh),
        ];
        let mut x = bar.left() + PAD;
        for ((g, tip, action), on) in nav.into_iter().zip(enabled) {
            let r = Rect::from_center_size(
                egui::pos2(x + BUTTON / 2.0, bar.center().y),
                Vec2::splat(BUTTON),
            );
            if round_icon(ui, r, Id::new(("nav", tip)), g, on, false)
                .on_hover_text(tip)
                .clicked()
            {
                actions.push(action);
            }
            x += BUTTON;
        }
        x += 8.0;

        // From the right: the view switch (or the bin's buttons), then search.
        let bin = self.tabs[self.active].loc == Loc::Bin;
        let toggle_w = if bin { super::bin::BUTTONS_W } else { TOGGLE_W };
        let right = bar.right() - PAD;
        let toggle = Rect::from_min_max(
            egui::pos2(right - toggle_w, bar.center().y - 16.0),
            egui::pos2(right, bar.center().y + 16.0),
        );
        if bin {
            self.bin_buttons(ui, toggle, actions);
        } else {
            self.view_toggle(ui, toggle, actions);
        }
        // The details island's switch, lit while it shows.
        let info = Rect::from_center_size(
            egui::pos2(toggle.left() - 8.0 - BUTTON / 2.0, bar.center().y),
            Vec2::splat(BUTTON),
        );
        let on = self.details_on();
        if on {
            let c = colors();
            ui.painter().circle_filled(info.center(), BUTTON / 2.0, soft_colors(c.light).1);
        }
        let tip = if on { "Hide details" } else { "Show details" };
        if round_icon(ui, info, Id::new("details-toggle"), glyph::INFO, true, false)
            .on_hover_text(tip)
            .clicked()
        {
            actions.push(Action::ToggleDetails);
        }
        let toggle = Rect::from_min_max(egui::pos2(info.left(), toggle.top()), toggle.max);
        let room = (toggle.left() - 10.0 - x).max(0.0);
        let search_w = (room * 0.4).clamp(140.0, SEARCH_W);
        let search = Rect::from_min_max(
            egui::pos2(toggle.left() - 10.0 - search_w, bar.center().y - 16.0),
            egui::pos2(toggle.left() - 10.0, bar.center().y + 16.0),
        );
        self.search_box(ui, search);
        let path = Rect::from_min_max(
            egui::pos2(x, bar.center().y - 16.0),
            egui::pos2((search.left() - 12.0).max(x), bar.center().y + 16.0),
        );
        self.path_bar(ui, path, actions);
    }

    /// List or grid, as a pill with two halves; the current one is lit.
    fn view_toggle(&self, ui: &mut Ui, rect: Rect, actions: &mut Vec<Action>) {
        let c = colors();
        let (soft, soft_hover, line) = soft_colors(c.light);
        ui.painter().rect(
            rect,
            CornerRadius::same(16),
            soft,
            line,
            egui::StrokeKind::Inside,
        );
        let halves = [
            (ViewMode::List, glyph::VIEW_LIST, "List (Ctrl+1)"),
            (ViewMode::Grid, glyph::VIEW_GRID, "Cards (Ctrl+2)"),
        ];
        let half_w = rect.width() / 2.0;
        for (i, (mode, g, tip)) in halves.into_iter().enumerate() {
            let r = Rect::from_min_size(
                rect.min + Vec2::new(i as f32 * half_w, 0.0),
                Vec2::new(half_w, rect.height()),
            )
            .shrink(3.0);
            let resp = ui
                .interact(r, Id::new(("view", i)), Sense::click())
                .on_hover_text(tip);
            let current = self.settings.view == mode;
            if current {
                ui.painter()
                    .rect_filled(r, CornerRadius::same(13), c.accent_fill);
            } else if resp.hovered() {
                ui.painter()
                    .rect_filled(r, CornerRadius::same(13), soft_hover);
            }
            let color = if current { c.on_accent } else { c.muted };
            ui.painter()
                .text(r.center(), Align2::CENTER_CENTER, g, icon_font(13.0), color);
            if resp.clicked() && !current {
                actions.push(Action::SetView(mode));
            }
        }
    }

    fn search_box(&mut self, ui: &mut Ui, rect: Rect) {
        let c = colors();
        let id = Id::new("search");
        let focused = ui.memory(|m| m.has_focus(id));
        let hovered = ui.rect_contains_pointer(rect);
        let (soft, soft_hover, line) = soft_colors(c.light);
        let fill = if focused || hovered { soft_hover } else { soft };
        let stroke = if focused {
            Stroke::new(1.0_f32, theme::with_alpha(c.accent_fill, 190))
        } else {
            line
        };
        ui.painter().rect(
            rect,
            CornerRadius::same(16),
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            egui::pos2(rect.left() + 16.0, rect.center().y),
            Align2::LEFT_CENTER,
            glyph::SEARCH,
            icon_font(12.0),
            c.muted,
        );
        let tab = &mut self.tabs[self.active];
        let hint = format!("Search {}", tab.loc.title());
        let inner = Rect::from_min_max(
            egui::pos2(rect.left() + 34.0, rect.top()),
            egui::pos2(rect.right() - 12.0, rect.bottom()),
        );
        let resp = ui.put(
            inner,
            egui::TextEdit::singleline(&mut tab.filter)
                .id(id)
                .frame(false)
                .hint_text(RichText::new(hint).color(c.muted))
                .vertical_align(Align::Center)
                .margin(Vec2::new(2.0, 0.0))
                .desired_width(inner.width()),
        );
        if std::mem::take(&mut self.focus_search) {
            resp.request_focus();
        }
        if resp.changed() {
            tab.dirty = true;
        }
        if resp.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            tab.filter.clear();
            tab.dirty = true;
        }
    }

    /// The path, as chips in `rect`; or the box to type one.
    fn path_bar(&mut self, ui: &mut Ui, rect: Rect, actions: &mut Vec<Action>) {
        let c = colors();
        let loc = self.tabs[self.active].loc.clone();
        if std::mem::take(&mut self.focus_path) && self.editing_path.is_none() {
            self.editing_path = Some(
                loc.dir()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default(),
            );
        }
        if let Some(text) = &mut self.editing_path {
            let (_, soft_hover, _) = soft_colors(c.light);
            ui.painter().rect(
                rect,
                CornerRadius::same(16),
                soft_hover,
                Stroke::new(1.0_f32, theme::with_alpha(c.accent_fill, 190)),
                egui::StrokeKind::Inside,
            );
            let resp = ui.put(
                rect.shrink2(Vec2::new(4.0, 2.0)),
                egui::TextEdit::singleline(text)
                    .id(Id::new("path-edit"))
                    .frame(false)
                    .vertical_align(Align::Center)
                    .margin(Vec2::new(10.0, 4.0))
                    .desired_width(rect.width()),
            );
            if !resp.has_focus() && !resp.lost_focus() {
                resp.request_focus();
                select_all(ui.ctx(), resp.id, text.chars().count());
            }
            if resp.lost_focus() {
                let entered = ui.input(|i| i.key_pressed(Key::Enter));
                let typed = std::mem::take(text);
                self.editing_path = None;
                if entered {
                    self.go_to_typed(typed.trim().trim_matches('"'), actions);
                }
            }
            return;
        }
        let crumbs = self.crumbs(&loc);
        let last = crumbs.len() - 1;
        let widths: Vec<f32> = crumbs
            .iter()
            .enumerate()
            .map(|(i, (name, _))| {
                let kind = if i == last { Chip::Accent } else { Chip::Soft };
                chip_width(ui, name, false, kind).min(220.0)
            })
            .collect();
        // Keep the deepest crumbs that fit, by their measured widths.
        let budget = rect.width() - TYPE_ZONE;
        let ellipsis_w = chip_width(ui, "…", false, Chip::Soft) + SEP_W;
        let mut used = 0.0;
        let mut first = crumbs.len();
        while first > 0 {
            let w = widths[first - 1] + if first - 1 < last { SEP_W } else { 0.0 };
            let more = if first > 1 { ellipsis_w } else { 0.0 };
            if used + w + more > budget && first < crumbs.len() {
                break;
            }
            used += w;
            first -= 1;
        }
        if first > 0 {
            used += ellipsis_w;
        }
        // Just after the path, a click types one; the field shows faintly while hovered.
        let zone = Rect::from_min_max(
            rect.min,
            egui::pos2(
                (rect.left() + used + TYPE_ZONE).min(rect.right()),
                rect.bottom(),
            ),
        );
        let bg = ui.interact(zone, Id::new("path-bg"), Sense::click());
        if bg.hovered() {
            let (soft, _, line) = soft_colors(c.light);
            ui.painter().rect(
                zone,
                CornerRadius::same(16),
                soft,
                line,
                egui::StrokeKind::Inside,
            );
        }
        let bg = bg.on_hover_text("Click to type a path (Ctrl+L)");
        if bg.clicked() {
            self.focus_path = true;
        }
        let y = rect.center().y - CHIP_H / 2.0;
        let mut x = rect.left() + 2.0;
        let sep = |ui: &Ui, x: &mut f32| {
            ui.painter().text(
                egui::pos2(*x + SEP_W / 2.0, rect.center().y),
                Align2::CENTER_CENTER,
                "/",
                FontId::proportional(13.0),
                c.muted,
            );
            *x += SEP_W;
        };
        if first > 0 {
            let w = ellipsis_w - SEP_W;
            let r = Rect::from_min_size(egui::pos2(x, y), Vec2::new(w, CHIP_H));
            let hidden: Vec<String> = crumbs[..first].iter().map(|(n, _)| n.clone()).collect();
            let _ = chip(ui, r, Id::new("crumb-more"), "…", None, Chip::Soft)
                .on_hover_text(hidden.join(" / "));
            x += w;
            sep(ui, &mut x);
        }
        for (i, (name, target)) in crumbs.into_iter().enumerate().skip(first) {
            let kind = if i == last { Chip::Accent } else { Chip::Soft };
            let w = widths[i].min(rect.right() - x).max(0.0);
            let r = Rect::from_min_size(egui::pos2(x, y), Vec2::new(w, CHIP_H));
            let resp = chip(ui, r, Id::new(("crumb", i)), &name, None, kind);
            if let Loc::Dir(p) = &target {
                super::dnd::zone(ui, r, super::dnd::Dest::Dir(p.clone()), CHIP_H / 2.0);
            }
            if resp.clicked() {
                actions.push(Action::Navigate(target.clone()));
            }
            if resp.middle_clicked() {
                actions.push(Action::NewTab(target));
            }
            x += w;
            if i < last {
                sep(ui, &mut x);
            }
        }
    }

    /// The steps of the path to `loc`, each with where it leads.
    fn crumbs(&self, loc: &Loc) -> Vec<(String, Loc)> {
        let mut crumbs: Vec<(String, Loc)> = match loc {
            Loc::Bin => vec![(loc.title(), Loc::Bin)],
            _ => vec![("This PC".into(), Loc::Pc)],
        };
        if let Some(dir) = loc.dir() {
            let mut parts: Vec<&Path> = dir.ancestors().collect();
            parts.reverse();
            for p in parts {
                let name = match p.file_name() {
                    Some(n) => n.to_string_lossy().into_owned(),
                    None => self
                        .drives
                        .iter()
                        .find(|d| d.root.as_path() == p)
                        .map_or_else(|| p.display().to_string(), Drive::title),
                };
                crumbs.push((name, Loc::Dir(p.to_path_buf())));
            }
        }
        crumbs
    }

    /// Go where a typed path leads: a folder, a file (opened), or This PC.
    fn go_to_typed(&mut self, typed: &str, actions: &mut Vec<Action>) {
        let p = PathBuf::from(expand_env(typed));
        if typed.eq_ignore_ascii_case("this pc") || typed.is_empty() {
            actions.push(Action::Navigate(Loc::Pc));
        } else {
            // A folder, a file, or nothing: looked at on a worker (see `probed`).
            self.probe(super::Probed::Typed(typed.to_owned(), p));
        }
    }
}

/// Expands %VARS% in a typed path, like Explorer's address bar.
pub(super) fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}
