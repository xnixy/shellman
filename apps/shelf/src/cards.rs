//! Rounded, rotated cards: a stack's three overlapped pictures, and a fan item's round
//! picture. egui draws rounded rectangles only upright, so a card is a polygon (its corners as
//! arcs), turned, filled with a mesh that carries the picture, and edged with an anti-aliased
//! line so the turned edge is smooth.

use egui::epaint::{Mesh, Vertex};
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, TextureId, Vec2};

/// What fills a card.
#[derive(Clone, Copy)]
pub enum Fill {
    Color(Color32),
    /// A picture, the part of it in `uv` (0 to 1).
    Picture(TextureId, Rect),
}

/// The part of a `size` picture that covers a square without stretching: its middle.
pub fn cover_uv(size: Vec2) -> Rect {
    if size.x <= 0.0 || size.y <= 0.0 {
        return Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
    }
    let (w, h) = (size.x, size.y);
    if w > h {
        let cut = (1.0 - h / w) / 2.0;
        Rect::from_min_max(egui::pos2(cut, 0.0), egui::pos2(1.0 - cut, 1.0))
    } else {
        let cut = (1.0 - w / h) / 2.0;
        Rect::from_min_max(egui::pos2(0.0, cut), egui::pos2(1.0, 1.0 - cut))
    }
}

/// The outline of an upright rounded rectangle `size` centred on the origin, clockwise.
pub fn outline(size: Vec2, radius: f32) -> Vec<Vec2> {
    let r = radius.clamp(0.0, size.x.min(size.y) / 2.0);
    let (hx, hy) = (size.x / 2.0, size.y / 2.0);
    let steps = if r > 0.0 { 6 } else { 0 };
    let corners = [
        (Vec2::new(hx - r, -hy + r), -90.0_f32),
        (Vec2::new(hx - r, hy - r), 0.0),
        (Vec2::new(-hx + r, hy - r), 90.0),
        (Vec2::new(-hx + r, -hy + r), 180.0),
    ];
    let mut out = Vec::with_capacity(4 * (steps + 1));
    for (c, start) in corners {
        for s in 0..=steps {
            let a = (start + 90.0 * s as f32 / steps.max(1) as f32).to_radians();
            out.push(c + Vec2::new(a.cos(), a.sin()) * r);
        }
    }
    out
}

fn turn(v: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// One card at `center`, `size` points, turned by `angle` radians, with a soft shadow under it
/// and a faint rim. `opacity` dims it all.
#[allow(clippy::too_many_arguments)]
pub fn card(painter: &Painter, center: Pos2, size: Vec2, radius: f32, angle: f32, fill: Fill, rim: Color32, opacity: f32) {
    let local = outline(size, radius);
    let points: Vec<Pos2> = local.iter().map(|v| center + turn(*v, angle)).collect();
    let fade = |c: Color32| c.gamma_multiply(opacity.clamp(0.0, 1.0));
    // A soft shadow, as the mockup's `0 4px 10px #0003`: a stack of faint copies growing outward,
    // each a little fainter, which add up to a blurred edge. Drawn in this window, no glass.
    const LAYERS: usize = 8;
    let blur = (size.x.min(size.y) * 0.22).clamp(3.0, 9.0);
    let drop = blur * 0.45;
    for i in 0..LAYERS {
        let grow = blur * (i as f32 + 1.0) / LAYERS as f32 - blur * 0.35;
        let layer: Vec<Pos2> = outline(size + Vec2::splat(2.0 * grow), radius + grow)
            .iter()
            .map(|v| center + Vec2::new(0.0, drop) + turn(*v, angle))
            .collect();
        painter.add(Shape::convex_polygon(layer, fade(Color32::from_black_alpha(9)), Stroke::NONE));
    }
    match fill {
        Fill::Color(c) => {
            painter.add(Shape::convex_polygon(points.clone(), fade(c), Stroke::NONE));
        }
        Fill::Picture(texture, uv) => {
            let mut mesh = Mesh::with_texture(texture);
            let tint = fade(Color32::WHITE);
            let uv_of = |v: Vec2| {
                let t = Vec2::new((v.x / size.x + 0.5).clamp(0.0, 1.0), (v.y / size.y + 0.5).clamp(0.0, 1.0));
                uv.min + uv.size() * t
            };
            mesh.vertices.push(Vertex { pos: center, uv: uv_of(Vec2::ZERO), color: tint });
            for (p, v) in points.iter().zip(&local) {
                mesh.vertices.push(Vertex { pos: *p, uv: uv_of(*v), color: tint });
            }
            let n = points.len() as u32;
            for i in 0..n {
                mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
            }
            painter.add(Shape::mesh(mesh));
        }
    }
    painter.add(Shape::closed_line(points, Stroke::new(1.0_f32, fade(rim))));
}

/// A plain coloured card with no shadow or rim, for a picture's backing (an icon's tile).
pub fn tile(painter: &Painter, center: Pos2, size: Vec2, radius: f32, angle: f32, color: Color32) {
    let points: Vec<Pos2> = outline(size, radius).iter().map(|v| center + turn(*v, angle)).collect();
    painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
}

/// A picture laid flat and turned, without rounding (an icon on its tile).
pub fn picture(painter: &Painter, center: Pos2, size: Vec2, angle: f32, texture: TextureId, opacity: f32) {
    let mut mesh = Mesh::with_texture(texture);
    let tint = Color32::WHITE.gamma_multiply(opacity.clamp(0.0, 1.0));
    let corners = [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)];
    for (x, y) in corners {
        let v = Vec2::new(x * size.x, y * size.y);
        mesh.vertices.push(Vertex { pos: center + turn(v, angle), uv: egui::pos2(x + 0.5, y + 0.5), color: tint });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_and_tall_pictures_are_cropped_to_their_middle() {
        let wide = cover_uv(Vec2::new(200.0, 100.0));
        assert_eq!((wide.min.x, wide.max.x, wide.min.y, wide.max.y), (0.25, 0.75, 0.0, 1.0));
        let tall = cover_uv(Vec2::new(100.0, 400.0));
        assert_eq!((tall.min.y, tall.max.y), (0.375, 0.625));
        let square = cover_uv(Vec2::new(64.0, 64.0));
        assert_eq!(square, Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)));
    }

    #[test]
    fn the_outline_stays_inside_its_box_and_touches_each_side() {
        let size = Vec2::new(40.0, 30.0);
        let pts = outline(size, 8.0);
        assert_eq!(pts.len(), 28);
        assert!(pts.iter().all(|p| p.x.abs() <= 20.001 && p.y.abs() <= 15.001));
        assert!(pts.iter().any(|p| (p.x - 20.0).abs() < 0.001));
        assert!(pts.iter().any(|p| (p.y + 15.0).abs() < 0.001));
        // A radius larger than half the shorter side is a capsule, not a mess.
        let pill = outline(size, 99.0);
        assert!(pill.iter().all(|p| p.y.abs() <= 15.001));
    }

    #[test]
    fn turning_keeps_lengths() {
        let v = turn(Vec2::new(3.0, 4.0), 0.7);
        assert!((v.length() - 5.0).abs() < 1e-4);
    }
}
