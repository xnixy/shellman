//! The header bar across the pane: back and forward, the path as chips (the current folder lit
//! with the accent) in a field, the search field, the details switch, the view switch (or the
//! bin's buttons) and the sort button, then Windows' own window buttons flush in the corner.
//! Where they go is `plan::tools`.
//!
//! Its empty stretch is the window's to drag; clicking the path's field after the crumbs, or
//! Ctrl+L, types a path instead.

use super::actions::Action;
use super::plan::{self, Plan, BUTTON};
use super::tab::Loc;
use crate::settings::ViewMode;
use egui::{Align, Align2, FontId, Id, Key, Rect, RichText, Sense, Stroke, Ui, Vec2};
use shellman_core::fsops::Drive;
use shellman_ui::chips::{chip, chip_width, round_icon, soft_colors, window_buttons_flush, Chip, CHIP_H};
use shellman_ui::theme::{self, colors, glyph, icon_font, shape};
use shellman_ui::widgets::{select_all, window_drag};
use std::path::{Path, PathBuf};

const TOGGLE_W: f32 = 76.0;
/// The room the path's crumbs leave at the end of its field, where a click starts typing one.
const TYPE_ZONE: f32 = 56.0;
const SEP_W: f32 = 16.0;

/// A field: Glasshouse's 12 pt rounded square in the field fill (see `sheet::field`).
fn field_fill(ui: &Ui, rect: Rect) {
    shellman_ui::sheet::field(ui, rect);
}

/// A field with the keyboard in it: the accent round its edge.
fn field_focus(ui: &Ui, rect: Rect) {
    let c = colors();
    ui.painter().rect_stroke(rect, shape::corner(shape::FIELD), Stroke::new(2.0_f32, c.accent_fill), egui::StrokeKind::Inside);
}

impl super::FileMan {
    /// The header bar of `plan`; `radius` is the window's corner, for the close button's.
    pub(super) fn toolbar(&mut self, ui: &mut Ui, plan: &Plan, radius: f32, actions: &mut Vec<Action>) {
        // Its empty space moves the window, and a double click maximizes it. Made first, so the
        // tools on it win.
        let drag = ui.interact(plan.header, Id::new("header-drag"), Sense::click_and_drag());
        window_drag(ui, &drag);
        let bin = self.tabs[self.active].loc == Loc::Bin;
        let t = if bin {
            plan::tools(plan.tools, super::bin::BUTTONS_W, super::bin::BUTTONS_LEAST_W)
        } else {
            plan::tools(plan.tools, TOGGLE_W, TOGGLE_W)
        };
        let tab = &self.tabs[self.active];
        let nav = [
            (t.back, glyph::BACK, "Back (Alt+Left)", Action::Back, !tab.back.is_empty()),
            (t.forward, glyph::FORWARD, "Forward (Alt+Right)", Action::Forward, !tab.fwd.is_empty()),
        ];
        for (r, g, tip, action, on) in nav {
            if round_icon(ui, r, Id::new(("nav", tip)), g, on, false, tip).on_hover_text(tip).clicked() {
                actions.push(action);
            }
        }
        self.path_bar(ui, t.path, actions);
        self.search_box(ui, t.search);
        // The details box's switch, lit while it shows.
        // In a narrow window the details show only alone, in the folder's place.
        let on = self.details_on() && (!self.narrow || self.details_alone);
        if on {
            let c = colors();
            ui.painter().circle_filled(t.info.center(), BUTTON / 2.0, soft_colors(c.light).1);
        }
        let tip = if on { "Hide details" } else { "Show details" };
        if round_icon(ui, t.info, Id::new("details-toggle"), glyph::INFO, true, false, tip)
            .on_hover_text(tip)
            .clicked()
        {
            actions.push(Action::ToggleDetails);
        }
        if let Some(view) = t.view {
            if bin {
                self.bin_buttons(ui, view, actions);
            } else {
                self.view_toggle(ui, view, actions);
            }
        }
        if let Some(sort) = t.sort {
            self.sort_button(ui, sort);
        }
        window_buttons_flush(ui, plan.captions, radius);
    }

    /// List or grid, as a pill with two halves; the current one is raised (Glasshouse's chosen
    /// toggle: the active tab's fill, not the accent; the highlight in high contrast).
    fn view_toggle(&self, ui: &mut Ui, rect: Rect, actions: &mut Vec<Action>) {
        let c = colors();
        let (soft, soft_hover, line) = soft_colors(c.light);
        ui.painter().rect(
            rect,
            shape::pill(rect),
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
            let name = if mode == ViewMode::List { "List view" } else { "Cards view" };
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, current, name));
            theme::focus_ring(ui, &resp, r, r.height() / 2.0);
            if current {
                ui.painter()
                    .rect_filled(r, shape::pill(r), c.control_active);
            } else if resp.hovered() && !c.high_contrast {
                ui.painter()
                    .rect_filled(r, shape::pill(r), soft_hover);
            }
            // On the high contrast highlight, only highlight text.
            let color = match (current, c.high_contrast) {
                (true, true) => c.on_accent,
                (true, false) => c.text,
                (false, _) => c.muted,
            };
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
        field_fill(ui, rect);
        if !c.high_contrast && (focused || hovered) {
            ui.painter().rect_filled(rect, shape::corner(shape::FIELD), c.row_hover);
        }
        if focused {
            field_focus(ui, rect);
        }
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
        shellman_ui::widgets::name_field(&resp, &format!("Search {}", tab.loc.title()), Some(&tab.filter));
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

    /// The path, as chips in a field at `rect`; or the field to type one.
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
        field_fill(ui, rect);
        if let Some(text) = &mut self.editing_path {
            field_focus(ui, rect);
            let resp = ui.put(
                rect.shrink2(Vec2::new(4.0, 2.0)),
                egui::TextEdit::singleline(text)
                    .id(Id::new("path-edit"))
                    .frame(false)
                    .vertical_align(Align::Center)
                    .margin(Vec2::new(10.0, 4.0))
                    .desired_width(rect.width()),
            );
            shellman_ui::widgets::name_field(&resp, "Path", Some(text.as_str()));
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
        // A click on the field around the crumbs types a path; the field lights while hovered.
        let bg = ui.interact(rect, Id::new("path-bg"), Sense::click());
        bg.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Type a path"));
        theme::focus_ring(ui, &bg, rect, shape::FIELD);
        if bg.hovered() && !c.high_contrast {
            ui.painter().rect_filled(rect, shape::corner(shape::FIELD), c.row_hover);
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
            let more = chip(ui, r, Id::new("crumb-more"), "…", None, Chip::Soft);
            let spoken = format!("More of the path: {}", hidden.join(" / "));
            more.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &spoken));
            let _ = more.on_hover_text(hidden.join(" / "));
            x += w;
            sep(ui, &mut x);
        }
        for (i, (name, target)) in crumbs.into_iter().enumerate().skip(first) {
            let kind = if i == last { Chip::Accent } else { Chip::Soft };
            let w = widths[i].min(rect.right() - x).max(0.0);
            let r = Rect::from_min_size(egui::pos2(x, y), Vec2::new(w, CHIP_H));
            let resp = chip(ui, r, Id::new(("crumb", i)), &name, None, kind);
            if i == last {
                resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, true, format!("{name}, current folder")));
            }
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
                        .map(Drive::title)
                        // A Linux distribution's root, listed or typed (`\\wsl$\Debian`).
                        .or_else(|| shellman_core::wsl::distro_of(p).map(|name| format!("{name} (Linux)")))
                        .unwrap_or_else(|| p.display().to_string()),
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
