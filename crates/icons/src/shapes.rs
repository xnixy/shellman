//! Tiny signed-distance shapes on a 0..1 square canvas. Negative inside, positive outside.

/// A rounded rectangle; a radius of half the shorter side makes a pill or a circle.
#[derive(Clone, Copy)]
pub struct RRect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub r: f32,
}

impl RRect {
    pub const fn new(x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> Self {
        Self { x0, y0, x1, y1, r }
    }

    /// A circle, as a rounded square whose corners take the whole side.
    pub const fn circle(cx: f32, cy: f32, r: f32) -> Self {
        Self::new(cx - r, cy - r, cx + r, cy + r, r)
    }

    pub fn dist(&self, x: f32, y: f32) -> f32 {
        let (cx, cy) = ((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0);
        let (hx, hy) = ((self.x1 - self.x0) / 2.0 - self.r, (self.y1 - self.y0) / 2.0 - self.r);
        let (qx, qy) = ((x - cx).abs() - hx, (y - cy).abs() - hy);
        let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
        outside + qx.max(qy).min(0.0) - self.r
    }
}

/// Several shapes drawn as one: inside any of them is inside the group.
pub fn union_dist(shapes: &[RRect], x: f32, y: f32) -> f32 {
    shapes.iter().map(|s| s.dist(x, y)).fold(f32::MAX, f32::min)
}
