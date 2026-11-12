//! The toolbar: navigation buttons, the path bar, and the search box.

use super::actions::Action;
use super::tab::Loc;
use super::widgets::{round_button, select_all};
use crate::fsops::{self, Drive};
use crate::theme::{self, colors, glyph, icon, icon_font};
use egui::{
    Align, Align2, Color32, CornerRadius, Id, Key, Layout, Rect, RichText, Sense, Stroke, Ui, Vec2,
};
use std::path::{Path, PathBuf};

impl super::FileMan {
    pub(super) fn toolbar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let tab = &self.tabs[self.active];
        let (can_back, can_fwd, can_up) = (
            !tab.back.is_empty(),
            !tab.fwd.is_empty(),
            tab.loc != Loc::Pc,
        );
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if round_button(ui, glyph::BACK, can_back, "Back (Alt+Left)").clicked() {
                actions.push(Action::Back);
            }
            if round_button(ui, glyph::FORWARD, can_fwd, "Forward (Alt+Right)").clicked() {
                actions.push(Action::Forward);
            }
            if round_button(ui, glyph::UP, can_up, "Up (Alt+Up)").clicked() {
                actions.push(Action::Up);
            }
            if round_button(ui, glyph::REFRESH, true, "Refresh (F5)").clicked() {
                actions.push(Action::Refresh);
            }
            ui.add_space(8.0);
            let search_w = 250.0;
            let path_w = (ui.available_width() - search_w - 10.0).max(120.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(path_w, 34.0), Sense::hover());
            self.path_bar(ui, rect, actions);
            ui.add_space(10.0);
            let (srect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::hover());
            self.search_box(ui, srect);
        });
    }

    fn search_box(&mut self, ui: &mut Ui, rect: Rect) {
        let c = colors();
        let id = Id::new("search");
        let focused = ui.memory(|m| m.has_focus(id));
        let hovered = ui.rect_contains_pointer(rect);
        let fill = if focused || hovered {
            c.control_hover
        } else {
            c.control
        };
        ui.painter().rect_filled(rect, CornerRadius::same(17), fill);
        if focused {
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(17),
                Stroke::new(1.0_f32, theme::with_alpha(c.accent, 170)),
                egui::StrokeKind::Inside,
            );
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

    fn path_bar(&mut self, ui: &mut Ui, rect: Rect, actions: &mut Vec<Action>) {
        let c = colors();
        let fill = if ui.rect_contains_pointer(rect) {
            c.control_hover
        } else {
            c.control
        };
        ui.painter().rect_filled(rect, CornerRadius::same(17), fill);
        let loc = self.tabs[self.active].loc.clone();
        if std::mem::take(&mut self.focus_path) && self.editing_path.is_none() {
            self.editing_path = Some(
                loc.dir()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default(),
            );
        }
        if let Some(text) = &mut self.editing_path {
            let resp = ui.put(
                rect.shrink2(Vec2::new(4.0, 2.0)),
                egui::TextEdit::singleline(text)
                    .id(Id::new("path-edit"))
                    .frame(false)
                    .margin(Vec2::new(8.0, 6.0))
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
                    let typed = typed.trim().trim_matches('"');
                    let p = PathBuf::from(expand_env(typed));
                    if typed.eq_ignore_ascii_case("this pc") || typed.is_empty() {
                        actions.push(Action::Navigate(Loc::Pc));
                    } else if p.is_dir() {
                        actions.push(Action::Navigate(Loc::Dir(p)));
                    } else if p.is_file() {
                        fsops::open(&p);
                    } else {
                        self.toast(format!("Can't find {typed}"));
                    }
                }
            }
            return;
        }
        // Clicking empty space in the bar switches to typing.
        let bg = ui.interact(rect, Id::new("path-bg"), Sense::click());
        if bg.clicked() {
            self.focus_path = true;
        }
        let mut crumbs: Vec<(String, Loc)> = vec![("This PC".into(), Loc::Pc)];
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
        // Keep the deepest crumbs that fit, roughly.
        let budget = rect.width() - 40.0;
        let mut used = 0.0;
        let mut first = crumbs.len();
        while first > 0 {
            let w = crumbs[first - 1].0.chars().count() as f32 * 7.5 + 30.0;
            if used + w > budget && first < crumbs.len() {
                break;
            }
            used += w;
            first -= 1;
        }
        let inner = rect.shrink2(Vec2::new(10.0, 1.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 0.0;
        if first > 0 {
            child.label(RichText::new("…").color(c.muted));
        }
        let last = crumbs.len() - 1;
        for (i, (name, target)) in crumbs.into_iter().enumerate().skip(first) {
            if i > first || first > 0 {
                child.label(icon(glyph::CHEVRON, 9.0).color(c.muted));
            }
            let text = RichText::new(name).color(if i == last { c.text } else { c.muted });
            let resp = child.add(
                egui::Button::new(text)
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::NONE)
                    .corner_radius(CornerRadius::same(12))
                    .min_size(Vec2::new(0.0, 26.0)),
            );
            if resp.clicked() {
                actions.push(Action::Navigate(target.clone()));
            }
            if resp.middle_clicked() {
                actions.push(Action::NewTab(target));
            }
        }
    }
}

/// Expands %VARS% in a typed path, like Explorer's address bar.
fn expand_env(s: &str) -> String {
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
