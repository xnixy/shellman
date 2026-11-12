//! Startup apps: the programs that start when you sign in, each with a switch. All reading and
//! writing is done by a worker (`StartupPage`), which also hears when the Run keys change; the
//! page draws what it last published and sends it what was switched.

use crate::page;
use crate::state::State;
use egui::{Align, CornerRadius, FontId, Layout, Sense, UiBuilder, Vec2};
use shellman_core::startup::rows::Row;
use shellman_core::startup::worker::{Page, StartupPage};
use shellman_core::thumbs::Thumbs;
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::{draw_text, switch};
use std::collections::HashMap;
use std::sync::Arc;

const ROW_H: f32 = 56.0;
const ICON: f32 = 28.0;

/// The page's live state. The worker starts when the page is first drawn and ends when another
/// page is shown, so nothing runs for it otherwise.
#[derive(Default)]
pub struct StartupView {
    handle: Option<StartupPage>,
    page: Page,
    loaded: bool,
    /// Switches pressed and not yet confirmed by a new reading (a UAC prompt may be open), so the
    /// switch follows the press. A new reading replaces them, which is also how a refused
    /// prompt puts the switch back.
    pending: HashMap<String, bool>,
}

impl StartupView {
    pub fn stop(&mut self) {
        if self.handle.take().is_some() {
            *self = Self::default();
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        if self.handle.is_none() {
            let ctx = ctx.clone();
            self.handle = Some(StartupPage::start(Arc::new(move || ctx.request_repaint())));
        }
    }

    fn poll(&mut self) {
        if let Some(page) = self.handle.as_ref().and_then(StartupPage::take) {
            self.page = page;
            self.loaded = true;
            self.pending.clear();
        }
    }
}

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Startup apps", "Programs that start when you sign in. Turn one off to stop it starting; it stays listed.");
    st.startup.start(ui.ctx());
    st.startup.poll();
    let StartupView { handle, page, loaded, pending } = &mut st.startup;
    let Some(handle) = handle.as_ref() else { return };
    if !*loaded {
        ui.label(theme::muted("Reading the startup apps..."));
        return;
    }
    if page.rows.is_empty() {
        ui.label(theme::muted("No app is set to start when you sign in."));
    }
    ui.spacing_mut().item_spacing.y = 4.0;
    for r in &page.rows {
        let key = r.key();
        let mut on = pending.get(&key).copied().unwrap_or(r.item.enabled);
        if row(ui, r, &mut on, &mut st.thumbs) {
            pending.insert(key, on);
            handle.set_enabled(&r.item, on);
        }
    }
    if let Some(why) = &page.notice {
        ui.add_space(8.0);
        ui.label(theme::muted(why).size(12.0));
    }
    page::gap(ui);
    ui.label(theme::muted("Startup tasks of Store apps are not listed here.").size(12.0));
}

/// One item: icon, name over command, and the switch. True when the switch was pressed.
fn row(ui: &mut egui::Ui, r: &Row, on: &mut bool, thumbs: &mut Thumbs) -> bool {
    let c = colors();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(10), c.control);
    // An item whose program is gone, and one that is off, are dimmer than one that will run.
    let (text, sub) = if r.missing || !*on { (c.muted, theme::with_alpha(c.muted, 170)) } else { (c.text, c.muted) };

    let icon = egui::Rect::from_center_size(egui::pos2(rect.left() + 14.0 + ICON / 2.0, rect.center().y), Vec2::splat(ICON));
    let image = r.exe.as_ref().filter(|_| !r.missing).and_then(|p| shellman_ui::app_icon::get(ui.ctx(), p, true, thumbs));
    match image {
        Some((id, size)) => {
            let scale = (ICON / size.x).min(ICON / size.y);
            let tint = if *on && !r.missing { egui::Color32::WHITE } else { theme::with_alpha(egui::Color32::WHITE, 120) };
            egui::Image::from_texture((id, size))
                .tint(tint)
                .paint_at(ui, egui::Rect::from_center_size(icon.center(), size * scale));
        }
        None => {
            ui.painter().text(icon.center(), egui::Align2::CENTER_CENTER, glyph::APP, icon_font(18.0), c.muted);
        }
    }

    let switch_w = 46.0;
    let text_left = icon.right() + 12.0;
    let text_right = rect.right() - switch_w - 16.0;
    let mut title = r.title.clone();
    if r.missing {
        title.push_str("  (Missing)");
    }
    let line = |top: f32, bottom: f32| egui::Rect::from_min_max(egui::pos2(text_left, top), egui::pos2(text_right, bottom));
    draw_text(ui, line(rect.top() + 7.0, rect.center().y), &title, FontId::proportional(14.0), text, false);
    draw_text(ui, line(rect.center().y, rect.bottom() - 7.0), &r.detail(), FontId::proportional(11.5), sub, false);

    let controls = egui::Rect::from_min_max(egui::pos2(rect.right() - switch_w - 8.0, rect.top()), egui::pos2(rect.right() - 8.0, rect.bottom()));
    let mut child = ui.new_child(UiBuilder::new().max_rect(controls).layout(Layout::left_to_right(Align::Center)));
    switch(&mut child, on, "").changed()
}
