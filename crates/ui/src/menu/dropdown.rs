//! A dropdown select: a box that shows the current choice and, clicked, drops shellman's glass
//! popup menu (`popup`) under it with the choices, the current one ticked. In place of egui's
//! `ComboBox`, whose list is an opaque egui popup inside the window.
//!
//! ```ignore
//! let rows: Vec<(String, bool)> = days.iter().map(|d| (d.name.clone(), d.id == now)).collect();
//! if let Some(i) = dropdown(ui, "first-day", 220.0, &shown, &rows) { pick = Some(days[i].id); }
//! ```
//!
//! The app's root viewport must call `popup::declare` every frame.

use super::popup::{Anchor, Entry, PopupMenu};
use crate::theme::colors;
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{pos2, vec2, CornerRadius, FontId, Id, Sense, Stroke, StrokeKind, Ui};
use std::hash::Hash;

/// Clicked again this soon after its menu closed (by that same click), the box closes it rather
/// than opening it again.
const TOGGLE: f64 = 0.4;

#[derive(Clone, Default)]
struct State {
    menu: PopupMenu<usize>,
    was_open: bool,
    closed_at: f64,
}

/// A dropdown `width` points wide showing `shown`, offering `rows` (label, ticked). Returns the
/// index of the row picked, in the frame the pick arrives.
pub fn dropdown(ui: &mut Ui, id_salt: impl Hash, width: f32, shown: &str, rows: &[(String, bool)]) -> Option<usize> {
    let id = ui.make_persistent_id(Id::new(id_salt).with("shellman-dropdown"));
    let h = ui.spacing().interact_size.y.max(30.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, h), Sense::click());
    let now = ui.input(|i| i.time);
    let mut st: State = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let open = st.menu.is_open();
    if st.was_open && !open {
        st.closed_at = now;
    }
    let picked = st.menu.take_pick();
    if resp.clicked() && ui.is_enabled() && now - st.closed_at > TOGGLE && !open {
        let entries = rows.iter().enumerate().map(|(i, (label, on))| Entry::item(i, label.clone()).checked(*on)).collect();
        st.menu.open(ui.ctx(), Anchor::under(ui.ctx(), rect), entries);
    }
    st.was_open = st.menu.is_open();
    ui.data_mut(|d| d.insert_temp(id, st.clone()));
    paint(ui, rect, &resp, shown, st.was_open);
    picked
}

/// The box: the widget frame egui gives a button, the text cut to fit, and a chevron.
fn paint(ui: &Ui, rect: egui::Rect, resp: &egui::Response, shown: &str, open: bool) {
    let c = colors();
    let visuals = if open { &ui.visuals().widgets.open } else { ui.style().interact(resp) };
    let radius = CornerRadius::same(8);
    ui.painter().rect(rect, radius, visuals.weak_bg_fill, visuals.bg_stroke, StrokeKind::Inside);
    let text_color = if ui.is_enabled() { c.text } else { c.muted };
    let chevron_x = rect.right() - 16.0;
    let mut job = LayoutJob::single_section(
        shown.to_owned(),
        TextFormat { font_id: FontId::proportional(14.0), color: text_color, ..Default::default() },
    );
    job.wrap = TextWrapping::truncate_at_width((chevron_x - 12.0 - rect.left() - 10.0).max(10.0));
    let galley = ui.fonts(|f| f.layout_job(job));
    let at = pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, text_color);
    // The chevron points down; the font's points right, so it is drawn as a small "v".
    let cy = rect.center().y;
    let stroke = Stroke::new(1.5_f32, c.muted);
    ui.painter().line_segment([pos2(chevron_x - 4.0, cy - 2.0), pos2(chevron_x, cy + 2.0)], stroke);
    ui.painter().line_segment([pos2(chevron_x, cy + 2.0), pos2(chevron_x + 4.0, cy - 2.0)], stroke);
}
