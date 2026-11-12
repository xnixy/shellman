//! The toolbar: navigation buttons, the path bar, and the search box.

use super::actions::Action;
use super::tab::Loc;
use crate::settings::ViewMode;
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Id, Key, Layout, Rect, RichText, Sense, Stroke,
    Ui, Vec2,
};
use shellman_core::fsops::{self, Drive};
use shellman_ui::theme::{self, colors, glyph, icon, icon_font};
use shellman_ui::widgets::{round_button, select_all};
use std::path::{Path, PathBuf};

impl super::FileMan {
    pub(super) fn toolbar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let tab = &self.tabs[self.active];
        let (can_back, can_fwd, can_up) = (
            !tab.back.is_empty(),
            !tab.fwd.is_empty(),
            tab.loc.parent().is_some(),
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
            // The path pill gives way; the search box and the toggle keep their size.
            let bin = self.tabs[self.active].loc == Loc::Bin;
            let (search_w, toggle_w) = (220.0, if bin { super::bin::BUTTONS_W } else { 76.0 });
            let path_w = (ui.available_width() - search_w - toggle_w - 20.0).max(120.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(path_w, 32.0), Sense::hover());
            self.path_bar(ui, rect, actions);
            ui.add_space(10.0);
            let (trect, _) = ui.allocate_exact_size(Vec2::new(toggle_w, 32.0), Sense::hover());
            if bin {
                self.bin_buttons(ui, trect, actions);
            } else {
                self.view_toggle(ui, trect, actions);
            }
            ui.add_space(10.0);
            let (srect, _) = ui.allocate_exact_size(Vec2::new(search_w, 32.0), Sense::hover());
            self.search_box(ui, srect);
        });
    }

    /// List or grid, as a pill with two halves; the current one is lit.
    fn view_toggle(&self, ui: &mut Ui, rect: Rect, actions: &mut Vec<Action>) {
        let c = colors();
        ui.painter()
            .rect_filled(rect, CornerRadius::same(16), c.control);
        let halves = [
            (ViewMode::List, glyph::VIEW_LIST, "List (Ctrl+1)"),
            (ViewMode::Grid, glyph::VIEW_GRID, "Icons (Ctrl+2)"),
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
                    .rect_filled(r, CornerRadius::same(13), c.control_active);
            } else if resp.hovered() {
                ui.painter()
                    .rect_filled(r, CornerRadius::same(13), c.control_hover);
            }
            let color = if current { c.accent_text } else { c.muted };
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
        let fill = if focused || hovered {
            c.control_hover
        } else {
            c.control
        };
        ui.painter().rect_filled(rect, CornerRadius::same(16), fill);
        if focused {
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(16),
                Stroke::new(1.0_f32, theme::with_alpha(c.accent_fill, 170)),
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
        ui.painter().rect_filled(rect, CornerRadius::same(16), fill);
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
        // Keep the deepest crumbs that fit, by their measured widths: an estimate from the
        // character count dropped crumbs while there was still room.
        let budget = rect.width() - 40.0;
        let (name_font, chevron_font) = (FontId::proportional(14.0), icon_font(9.0));
        let chevron_w = ui
            .fonts(|f| f.layout_no_wrap(glyph::CHEVRON.to_string(), chevron_font, c.muted))
            .size()
            .x;
        let mut used = 0.0;
        let mut first = crumbs.len();
        while first > 0 {
            let name = crumbs[first - 1].0.clone();
            let text_w = ui
                .fonts(|f| f.layout_no_wrap(name, name_font.clone(), c.text))
                .size()
                .x;
            let w = text_w + 22.0 + chevron_w;
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
