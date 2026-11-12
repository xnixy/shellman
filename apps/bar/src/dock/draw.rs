//! Painting the dock: the glass pill, the icons with their running marks, and the dividers. It
//! only reports what the pointer did; `Dock` decides what that means.

use super::items::{Item, Section};
use crate::shape::SYSTEM_RADIUS as RADIUS;
use shellman_core::thumbs::Thumbs;
use shellman_ui::theme::{colors, glyph, icon_font};

pub const HEIGHT: f32 = 56.0;
const CELL: f32 = 52.0;
const ICON: f32 = 40.0;
const PAD: f32 = 8.0;
/// Extra room a divider takes between two sections.
const DIVIDER: f32 = 13.0;

/// A divider goes before the first running app (after Files and the pins), and before the bin.
fn divider_before(items: &[Item], i: usize) -> bool {
    match items[i].section {
        Section::Running => i == 0 || items[i - 1].section != Section::Running,
        Section::Bin => true,
        _ => false,
    }
}

/// Left edge of each icon's cell, and the dock's total width, in points.
pub fn layout(items: &[Item]) -> (Vec<f32>, f32) {
    let mut x = PAD;
    let mut xs = Vec::with_capacity(items.len());
    for i in 0..items.len() {
        if divider_before(items, i) {
            x += DIVIDER;
        }
        xs.push(x);
        x += CELL;
    }
    (xs, x + PAD)
}

/// What the pointer did over the icons this frame; each is an index into the items.
#[derive(Default)]
pub struct Events {
    pub clicked: Option<usize>,
    pub middle: Option<usize>,
    pub menu: Option<usize>,
    /// Scrolled over an icon: its index and the wheel's vertical delta.
    pub scrolled: Option<(usize, f32)>,
}

pub fn paint(ui: &mut egui::Ui, glass: bool, thumbs: &mut Thumbs, items: &[Item], front: Option<isize>) -> Events {
    let c = colors();
    let pill = ui.max_rect();
    // The same surfaces as Files: its glass tint, and the lighter sheet its files sit on.
    let radius = egui::CornerRadius::same(RADIUS as u8);
    if glass {
        ui.painter().rect_filled(pill, radius, c.shell);
        ui.painter().rect_filled(pill, radius, c.sheet);
    } else {
        ui.painter().rect_filled(pill, radius, c.bg);
    }
    ui.painter().rect_stroke(
        pill.shrink(0.5),
        radius,
        egui::Stroke::new(1.0_f32, c.hairline),
        egui::StrokeKind::Inside,
    );
    let (xs, _) = layout(items);
    let mut events = Events::default();
    for (i, item) in items.iter().enumerate() {
        if divider_before(items, i) {
            let x = pill.min.x + xs[i] - DIVIDER / 2.0;
            let mid = pill.center().y;
            ui.painter().line_segment(
                [egui::pos2(x, mid - 12.0), egui::pos2(x, mid + 12.0)],
                egui::Stroke::new(1.0_f32, c.hairline),
            );
        }
        let cell = egui::Rect::from_min_size(
            pill.min + egui::vec2(xs[i], (HEIGHT - CELL) / 2.0),
            egui::vec2(CELL, CELL),
        );
        let resp = ui.interact(cell, egui::Id::new(("dock", &item.app_id)), egui::Sense::click());
        let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
        if hover > 0.0 {
            ui.painter().rect_filled(cell.shrink(3.0), egui::CornerRadius::same(10), c.control_hover.gamma_multiply(hover));
        }
        let icon = egui::Rect::from_center_size(
            cell.center() - egui::vec2(0.0, 2.0 + hover * 2.0),
            egui::Vec2::splat(ICON),
        );
        match item.icon_source().and_then(|p| thumbs.get(&p, true).map(|t| (t.id(), t.size_vec2()))) {
            Some((id, size)) => {
                let scale = (ICON / size.x).min(ICON / size.y);
                egui::Image::from_texture((id, size))
                    .paint_at(ui, egui::Rect::from_center_size(icon.center(), size * scale));
            }
            None => {
                ui.painter().rect_filled(icon, egui::CornerRadius::same(10), c.control);
                ui.painter().text(icon.center(), egui::Align2::CENTER_CENTER, glyph::APP, icon_font(18.0), c.muted);
            }
        }
        // Running dots; the app in front gets an accent bar instead. Pins that are not running
        // have neither.
        let y = cell.bottom() - 4.0;
        if front.is_some_and(|f| item.windows.contains(&f)) {
            let bar = egui::Rect::from_center_size(egui::pos2(cell.center().x, y), egui::vec2(16.0, 3.0));
            ui.painter().rect_filled(bar, egui::CornerRadius::same(2), c.accent);
        } else {
            let n = item.windows.len().min(3);
            for d in 0..n {
                let x = cell.center().x + (d as f32 - (n - 1) as f32 / 2.0) * 7.0;
                ui.painter().circle_filled(egui::pos2(x, y), 2.0, c.text.gamma_multiply(0.55));
            }
        }
        if resp.hovered() {
            let delta = ui.input(|i| i.raw_scroll_delta.y);
            if delta != 0.0 {
                events.scrolled = Some((i, delta));
            }
        }
        let resp = resp.on_hover_text(item.name());
        if resp.clicked() {
            events.clicked = Some(i);
        }
        if resp.middle_clicked() {
            events.middle = Some(i);
        }
        if resp.secondary_clicked() {
            events.menu = Some(i);
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(section: Section) -> Item {
        Item { section, app_id: String::new(), exe: None, windows: Vec::new() }
    }

    #[test]
    fn dividers_sit_between_sections() {
        let items = [
            item(Section::Files),
            item(Section::Pinned),
            item(Section::Running),
            item(Section::Running),
            item(Section::Bin),
        ];
        let (xs, total) = layout(&items);
        assert_eq!(xs[0], PAD);
        assert_eq!(xs[1], PAD + CELL);
        // A divider before the first running app and before the bin, none inside a section.
        assert_eq!(xs[2], PAD + 2.0 * CELL + DIVIDER);
        assert_eq!(xs[3], xs[2] + CELL);
        assert_eq!(xs[4], xs[3] + CELL + DIVIDER);
        assert_eq!(total, xs[4] + CELL + PAD);
        // Without running apps, only the one before the bin.
        let (_, total) = layout(&[item(Section::Files), item(Section::Bin)]);
        assert_eq!(total, PAD * 2.0 + CELL * 2.0 + DIVIDER);
    }
}
