//! Default apps, read-only: which app opens each common file type and link, with its icon. There
//! is no change button because Windows protects the choice (a hash only its own Settings can
//! write), so a program that sets it is reset.

use crate::page;
use crate::state::State;
use egui::{CornerRadius, FontId, Sense, Vec2};
use shellman_core::system::default_apps::{self as da, Entry};
use shellman_core::thumbs::Thumbs;
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::draw_text;
use std::sync::{Arc, Mutex};

const ROW_H: f32 = 56.0;
const ICON: f32 = 28.0;

type Shared = Arc<Mutex<Option<Vec<Entry>>>>;

/// The page's live state; it reads again each time the page is shown.
#[derive(Default)]
pub struct DefaultAppsView {
    entries: Shared,
    started: bool,
}

impl DefaultAppsView {
    pub fn stop(&mut self) {
        *self = Self::default();
    }

    fn start(&mut self, ctx: &egui::Context) {
        if self.started {
            return;
        }
        self.started = true;
        let (shared, ctx) = (self.entries.clone(), ctx.clone());
        std::thread::spawn(move || {
            let v = da::read();
            if let Ok(mut s) = shared.lock() {
                *s = Some(v);
            }
            ctx.request_repaint();
        });
    }
}

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Default apps", "Which app opens each kind of file and link.");
    st.default_apps.start(ui.ctx());
    let entries = st.default_apps.entries.lock().ok().and_then(|g| g.clone());
    let Some(entries) = entries else {
        page::group(ui, "", |ui| {
            ui.label(theme::muted("Asking Windows..."));
        });
        return;
    };
    let thumbs = &mut st.thumbs;
    page::group(ui, "File types and links", |ui| {
        ui.label(theme::muted("Windows only lets its own Settings change these, so they are shown here and cannot be changed.").size(12.0));
        ui.add_space(10.0);
        ui.spacing_mut().item_spacing.y = 4.0;
        for e in &entries {
            row(ui, e, thumbs);
        }
    });
}

fn row(ui: &mut egui::Ui, e: &Entry, thumbs: &mut Thumbs) {
    let c = colors();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(10), c.control);
    let icon = egui::Rect::from_center_size(egui::pos2(rect.left() + 14.0 + ICON / 2.0, rect.center().y), Vec2::splat(ICON));
    let image = e.exe.as_ref().and_then(|p| shellman_ui::app_icon::get(ui.ctx(), p, true, thumbs));
    match image {
        Some((id, size)) => {
            let scale = (ICON / size.x).min(ICON / size.y);
            egui::Image::from_texture((id, size)).paint_at(ui, egui::Rect::from_center_size(icon.center(), size * scale));
        }
        None => {
            ui.painter().text(icon.center(), egui::Align2::CENTER_CENTER, glyph::APP, icon_font(18.0), c.muted);
        }
    }
    let text_left = icon.right() + 12.0;
    let key_w = 90.0;
    let line = |top: f32, bottom: f32, right: f32| egui::Rect::from_min_max(egui::pos2(text_left, top), egui::pos2(right, bottom));
    let left_edge = rect.right() - key_w - 16.0;
    draw_text(ui, line(rect.top() + 7.0, rect.center().y, left_edge), e.title, FontId::proportional(14.0), c.text, false);
    let (app, color) = match &e.app {
        Some(a) => (a.as_str(), c.muted),
        None => (da::NO_APP, theme::with_alpha(c.muted, 170)),
    };
    draw_text(ui, line(rect.center().y, rect.bottom() - 7.0, left_edge), app, FontId::proportional(12.0), color, false);
    let key = egui::Rect::from_min_max(egui::pos2(rect.right() - key_w - 14.0, rect.top()), egui::pos2(rect.right() - 14.0, rect.bottom()));
    draw_text(ui, key, &da::key_label(e.key, e.kind), FontId::proportional(12.0), c.muted, true);
}
