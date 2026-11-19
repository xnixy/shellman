//! The graphs: a thin accent line over a soft fill, newest at the right, a minute wide. The same
//! weight as the Islands sliders, so they read as one family.

use egui::{pos2, Color32, Mesh, Painter, Pos2, Rect, Shape, Stroke};
use shellman_core::procs::history::{Ring, LEN};
use shellman_ui::glass::islands::shows_as;
use shellman_ui::theme::{self, colors, shape};

/// Where each value goes in `rect`, `max` at the top. The newest value is at the right edge and
/// each older one a step left, so a graph that is not full yet grows in from the right.
pub fn points(ring: &Ring, rect: Rect, max: f32) -> Vec<Pos2> {
    let step = rect.width() / (LEN - 1) as f32;
    let n = ring.len();
    let max = if max > 0.0 { max } else { 1.0 };
    ring.iter()
        .enumerate()
        .map(|(i, v)| {
            let x = rect.right() - (n - 1 - i) as f32 * step;
            let y = rect.bottom() - (v / max).clamp(0.0, 1.0) * rect.height();
            pos2(x, y)
        })
        .collect()
}

/// The scale for a rate graph: the largest value held, rounded up so the line rarely touches the
/// top, never less than `floor` (an idle disk does not fill the graph with noise).
pub fn scale(ring: &Ring, floor: f32) -> f32 {
    let m = ring.max().max(floor);
    let magnitude = 10f32.powf(m.log10().floor());
    for step in [1.0, 2.0, 5.0, 10.0] {
        if m <= step * magnitude {
            return step * magnitude;
        }
    }
    m
}

/// A graph in `rect`: a faint well, a few faint guide lines when `grid`, the soft fill under the
/// line, and the line.
pub fn draw(painter: &Painter, rect: Rect, ring: &Ring, max: f32, color: Color32, grid: bool) {
    let c = colors();
    if c.high_contrast {
        // No tints: the well is outlined, the line drawn alone.
        painter.rect_stroke(rect, shape::TILE, Stroke::new(1.0_f32, c.text), egui::StrokeKind::Inside);
        let pts = points(ring, rect.shrink2(egui::vec2(1.0, 3.0)), max);
        if pts.len() > 1 {
            painter.add(Shape::line(pts, Stroke::new(2.0_f32, color)));
        }
        return;
    }
    painter.rect_filled(rect, shape::TILE, c.well);
    if grid {
        let line = Stroke::new(1.0_f32, shows_as(theme::with_alpha(c.text, 12)));
        for i in 1..4 {
            let y = rect.top() + rect.height() * i as f32 / 4.0;
            painter.line_segment([pos2(rect.left() + 8.0, y), pos2(rect.right() - 8.0, y)], line);
        }
    }
    let inner = rect.shrink2(egui::vec2(1.0, 3.0));
    let pts = points(ring, inner, max);
    if pts.len() < 2 {
        return;
    }
    painter.add(fill(&pts, inner.bottom(), shows_as(theme::with_alpha(color, if c.light { 46 } else { 56 }))));
    painter.add(Shape::line(pts, Stroke::new(1.5_f32, color)));
}

/// The area under the line, as one strip of quads (a polygon fill would need it convex).
fn fill(pts: &[Pos2], bottom: f32, color: Color32) -> Shape {
    let mut mesh = Mesh::default();
    for p in pts {
        mesh.colored_vertex(*p, color);
        mesh.colored_vertex(pos2(p.x, bottom), color);
    }
    for i in 0..pts.len() as u32 - 1 {
        let (a, b, c, d) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
        mesh.add_triangle(a, b, c);
        mesh.add_triangle(c, b, d);
    }
    Shape::mesh(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_value_is_at_the_right() {
        let mut r = Ring::default();
        for v in [0.0, 50.0, 100.0] {
            r.push(v);
        }
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(590.0, 100.0));
        let p = points(&r, rect, 100.0);
        assert_eq!(p.len(), 3);
        assert_eq!(p[2], pos2(590.0, 0.0));
        assert_eq!(p[1], pos2(580.0, 50.0));
        assert_eq!(p[0], pos2(570.0, 100.0));
    }

    #[test]
    fn values_stay_in_the_graph() {
        let mut r = Ring::default();
        r.push(500.0);
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(10.0, 10.0));
        assert_eq!(points(&r, rect, 100.0)[0].y, 0.0);
        assert_eq!(points(&r, rect, 0.0)[0].y, 0.0, "no scale reads as 1");
    }

    #[test]
    fn scales_round_up() {
        let mut r = Ring::default();
        r.push(3_400.0);
        assert_eq!(scale(&r, 1000.0), 5000.0);
        r.push(9_000.0);
        assert_eq!(scale(&r, 1000.0), 10_000.0);
        assert_eq!(scale(&Ring::default(), 1000.0), 1000.0);
    }
}
