//! What a glass surface looks like (in points and colours), and where its parts land in pixels.
//! Pure, so it is tested without a window.

use crate::theme::{self, colors, with_alpha, Colors};
use egui::Color32;

/// The soft shadow under a surface.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Shadow {
    /// How far down the shadow falls, in points.
    pub offset_y: f32,
    /// How far it spreads, in points.
    pub blur: f32,
    pub color: Color32,
}

/// The look of one glass surface.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Style {
    /// Corner radius, in points.
    pub radius: f32,
    /// Laid over the blurred backdrop. Its alpha is how much it hides; the user's glass amount
    /// sets it.
    pub tint: Color32,
    /// The 1 px line just inside the edge, at the top; it fades to a third of this lower down.
    pub rim: Color32,
    pub shadow: Shadow,
    /// What fills the shape when Windows has transparency effects turned off.
    pub solid: Color32,
}

impl Style {
    /// The theme's floating glass, in its current mode, accent and glass amount.
    pub fn from_theme(radius: f32) -> Self {
        Self::new(&colors(), theme::glass(), radius)
    }

    /// Floating glass for a palette and a glass amount (0 solid, 1 nearly clear).
    pub fn new(c: &Colors, glass: f32, radius: f32) -> Self {
        // From almost solid at glass 0 to a thin veil at glass 1.
        let alpha = (240.0 + (150.0 - 240.0) * glass).round() as u8;
        let (tint, rim, shadow) = if c.light {
            (
                with_alpha(theme::mix(Color32::from_gray(246), c.accent, 0.04), alpha),
                Color32::from_white_alpha(150),
                Color32::from_black_alpha(46),
            )
        } else {
            (
                with_alpha(theme::mix(Color32::from_gray(22), c.accent, 0.06), alpha),
                Color32::from_white_alpha(34),
                Color32::from_black_alpha(90),
            )
        };
        Self {
            radius,
            tint,
            rim,
            shadow: Shadow {
                offset_y: 8.0,
                blur: 32.0,
                color: shadow,
            },
            solid: c.bg,
        }
    }
}

/// A surface's parts in physical pixels, for a shape of a given pixel size at a given scale.
///
/// The backdrop window is the shape plus room for the shadow on every side; the shape sits at
/// (`left`, `top`) inside it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Geometry {
    pub shape: (i32, i32),
    pub radius: f32,
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub shadow_offset: f32,
    pub shadow_blur: f32,
}

impl Geometry {
    pub fn new(shape: (i32, i32), scale: f32, style: &Style) -> Self {
        let px = |pt: f32| pt * scale;
        // The radius can never be more than half the shorter side.
        let radius = px(style.radius)
            .min(shape.0.min(shape.1) as f32 / 2.0)
            .max(0.0);
        let blur = px(style.shadow.blur).max(0.0);
        let offset = px(style.shadow.offset_y);
        // A composition shadow fades out about one blur radius past its shape.
        let spread = blur.ceil() as i32;
        Self {
            shape,
            radius,
            left: spread,
            right: spread,
            top: (spread - offset.floor() as i32).max(0),
            bottom: spread + offset.ceil().max(0.0) as i32,
            shadow_offset: offset,
            shadow_blur: blur,
        }
    }

    /// The backdrop window's size.
    pub fn window(&self) -> (i32, i32) {
        (
            self.shape.0 + self.left + self.right,
            self.shape.1 + self.top + self.bottom,
        )
    }

    /// The rounding of the window region that takes the clicks: a little tighter than the
    /// drawn corner, so the region (which has no anti-aliasing) never trims the shape's soft edge.
    pub fn region_radius(&self) -> i32 {
        (self.radius - 2.0).max(0.0).round() as i32
    }
}

/// The egui frame for a surface's content. Over composed glass it paints nothing (the backdrop
/// window draws the surface); without it, egui paints the surface itself: the tint made nearly
/// solid (nothing blurs behind it), the rounded shape and the rim. There is no shadow then, since
/// the window is only as big as the shape.
pub fn content_frame(composed: bool, style: &Style) -> egui::Frame {
    if composed {
        return egui::Frame::new();
    }
    egui::Frame::new()
        .fill(theme::mix(style.solid, style.tint, 0.5))
        .corner_radius(style.radius.round().clamp(0.0, 255.0) as u8)
        .stroke(egui::Stroke::new(
            1.0_f32,
            with_alpha(style.rim, style.rim.a() / 2),
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(radius: f32) -> Style {
        Style {
            radius,
            tint: Color32::BLACK,
            rim: Color32::WHITE,
            shadow: Shadow {
                offset_y: 8.0,
                blur: 32.0,
                color: Color32::BLACK,
            },
            solid: Color32::BLACK,
        }
    }

    #[test]
    fn points_become_pixels_at_each_scale() {
        let g = Geometry::new((1080, 840), 1.5, &style(24.0));
        assert_eq!(g.radius, 36.0);
        assert_eq!((g.left, g.right), (48, 48));
        // The shadow falls 12 px down: less room above, more below.
        assert_eq!((g.top, g.bottom), (36, 60));
        assert_eq!(g.window(), (1080 + 96, 840 + 96));
        let g = Geometry::new((2160, 1680), 3.0, &style(24.0));
        assert_eq!(g.radius, 72.0);
        assert_eq!((g.left, g.top, g.bottom), (96, 72, 120));
    }

    #[test]
    fn radius_fits_the_shape() {
        let g = Geometry::new((100, 40), 2.0, &style(28.0));
        assert_eq!(g.radius, 20.0);
        let g = Geometry::new((0, 0), 1.0, &style(28.0));
        assert_eq!(g.radius, 0.0);
        assert_eq!(g.region_radius(), 0);
    }

    #[test]
    fn click_region_is_never_rounder_than_the_drawn_shape() {
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
            let g = Geometry::new((800, 600), scale, &style(24.0));
            assert!(g.region_radius() as f32 <= g.radius);
            assert!(g.region_radius() as f32 >= g.radius - 2.5);
        }
    }

    #[test]
    fn theme_style_follows_mode_and_glass() {
        let accent = Color32::from_rgb(0, 103, 192);
        let dark = Style::new(&Colors::new(false, accent), 0.5, 24.0);
        let light = Style::new(&Colors::new(true, accent), 0.5, 24.0);
        assert!(dark.tint.r() < 80 && light.tint.r() > 200);
        // Light mode's shadow is lighter.
        assert!(light.shadow.color.a() < dark.shadow.color.a());
        assert_eq!(dark.radius, 24.0);
        // More glass, a thinner tint.
        let clear = Style::new(&Colors::new(false, accent), 1.0, 24.0);
        let solid = Style::new(&Colors::new(false, accent), 0.0, 24.0);
        assert!(clear.tint.a() < dark.tint.a() && dark.tint.a() < solid.tint.a());
    }
}
