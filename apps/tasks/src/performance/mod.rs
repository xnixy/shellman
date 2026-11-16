//! The Performance page: one island per resource (CPU, memory, each disk, each network adapter
//! that is up, each GPU), each with its last minute as a larger graph and its key numbers,
//! scrolling under the title pill.

mod cards;

use crate::app::{State, MARGIN, RADIUS};
use egui::{pos2, vec2, Align2, FontId, Painter, Rect, Sense, UiBuilder};
use shellman_ui::glass::islands::Island;
use shellman_ui::theme::{colors, semibold_font};

/// Inside an island: room around the content.
pub const PAD: f32 = 20.0;
/// The numbers' column at the right of a graph.
pub const STATS_W: f32 = 300.0;

pub fn show(ui: &mut egui::Ui, area: Rect, st: &mut State, islands: &mut Vec<Island>) {
    let mut shown: Vec<Rect> = Vec::new();
    ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
        ui.spacing_mut().scroll = egui::style::ScrollStyle::floating();
        ui.spacing_mut().item_spacing.y = MARGIN;
        egui::ScrollArea::vertical()
            .id_salt("performance")
            .auto_shrink([false, false])
            .drag_to_scroll(false)
            .show(ui, |ui| {
                let mut island = |ui: &mut egui::Ui, height: f32, name: &str, draw: &mut dyn FnMut(&mut egui::Ui, Rect)| {
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
                    // The graph and its numbers, read out as one image.
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, name));
                    let visible = rect.intersect(ui.clip_rect());
                    if visible.height() > 1.0 {
                        shown.push(visible);
                        draw(ui, rect.shrink(PAD));
                    }
                };
                cards::all(ui, st, &mut island);
            });
    });
    islands.extend(shown.into_iter().map(|r| Island::new(r, RADIUS)));
}

/// An island's title and a muted line after it.
pub fn title(p: &Painter, rect: Rect, name: &str, about: &str) {
    let c = colors();
    let t = p.text(rect.min, Align2::LEFT_TOP, name, semibold_font(16.0), c.text);
    p.text(pos2(t.right() + 10.0, t.center().y + 1.0), Align2::LEFT_CENTER, about, FontId::proportional(12.5), c.muted);
}

/// An island as a screen reader says it: "CPU graph, Intel..., last minute. Utilisation 12%, ...".
pub fn spoken(name: &str, about: &str, pairs: &[(&str, String)]) -> String {
    let mut s = format!("{name} graph");
    if !about.is_empty() {
        s += &format!(", {about}");
    }
    s += ", last minute.";
    let numbers: Vec<String> = pairs.iter().filter(|(l, _)| !l.is_empty()).map(|(l, v)| format!("{l} {v}")).collect();
    if !numbers.is_empty() {
        s += &format!(" {}", numbers.join(", "));
    }
    s
}

/// Label and value pairs in two columns, from the top of `rect`.
pub fn stats(p: &Painter, rect: Rect, pairs: &[(&str, String)]) {
    let c = colors();
    let col = rect.width() / 2.0;
    for (i, (label, value)) in pairs.iter().enumerate() {
        let x = rect.left() + (i % 2) as f32 * col;
        let y = rect.top() + (i / 2) as f32 * 44.0;
        let clip = p.with_clip_rect(Rect::from_min_size(pos2(x, y), vec2(col - 10.0, 44.0)));
        clip.text(pos2(x, y), Align2::LEFT_TOP, *label, FontId::proportional(12.0), c.muted);
        clip.text(pos2(x, y + 16.0), Align2::LEFT_TOP, value, semibold_font(15.0), c.text);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn islands_read_their_numbers() {
        let s = super::spoken("Disk 0", "Active time", &[("Active time", "3%".into()), ("", String::new()), ("Read", "1 MB/s".into())]);
        assert_eq!(s, "Disk 0 graph, Active time, last minute. Active time 3%, Read 1 MB/s");
        assert_eq!(super::spoken("CPU", "", &[]), "CPU graph, last minute.");
    }
}
