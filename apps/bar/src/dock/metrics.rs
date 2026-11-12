//! The dock's measurements: how the icons sit on the glass, how big the glass is, and where it
//! and the name label go in the dock window. Pure arithmetic, tested without windows.
//!
//! The dock window is as wide as its monitor's band and taller than the glass by `ABOVE`, room
//! for the name label. It is placed once; as apps come and go only the glass inside it changes
//! (see `shellman_ui::glass::GlassPanel::set_shape`), so its icons never jump while a resized
//! window catches up with its content. Outside the glass (and the label, while shown) the window
//! is neither drawn nor clicked.

use super::items::{Item, Section};
use shellman_core::settings::dock::IconSize;
use windows::Win32::Foundation::RECT;

/// Space between the glass's edge and the icons, on every side.
const PAD: f32 = 11.0;
/// Space between two icons.
const SPACING: f32 = 8.0;
/// Extra space a divider adds between two sections.
const DIVIDER: f32 = 12.0;
/// Room above the glass for the name label.
pub const ABOVE: f32 = 40.0;
/// The label: its height, and the space between it and the glass.
pub const LABEL_H: f32 = 26.0;
pub const LABEL_GAP: f32 = 8.0;
/// Room around the label for its soft shadow, which the window must also show.
const LABEL_SHADOW: f32 = 6.0;
/// Space kept free at the sides of the window when the dock is wider than the monitor.
const SIDE: f32 = 8.0;

/// The dock's sizes for one icon size, in points.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Metrics {
    pub icon: f32,
    pub pad: f32,
    pub height: f32,
    /// The glass's corner: the icons' own corner plus the padding, so the two curves run
    /// together.
    pub radius: f32,
}

impl Metrics {
    pub fn new(size: IconSize) -> Self {
        let icon = size.icon();
        Self {
            icon,
            pad: PAD,
            height: icon + 2.0 * PAD,
            radius: (icon * 0.22 + PAD).round(),
        }
    }
}

/// A divider goes before the first running app (after the Launcher, Files and the pins), and
/// before the bin.
pub fn divider_before(items: &[Item], i: usize) -> bool {
    i > 0
        && match items[i].section {
            Section::Running => items[i - 1].section != Section::Running,
            Section::Bin => true,
            _ => false,
        }
}

/// Left edge of each icon from the glass's left edge, and the glass's width, in points.
pub fn layout(items: &[Item], m: &Metrics) -> (Vec<f32>, f32) {
    let mut x = m.pad;
    let mut xs = Vec::with_capacity(items.len());
    for i in 0..items.len() {
        if i > 0 {
            x += SPACING;
        }
        if divider_before(items, i) {
            x += DIVIDER;
        }
        xs.push(x);
        x += m.icon;
    }
    (xs, x + m.pad)
}

/// The glass in the dock window's pixels: `width` points wide, centred, along the bottom.
/// `window` is the window's size in pixels, `scale` its pixels per point. Never wider than the
/// window allows.
pub fn glass_rect(window: (i32, i32), width: f32, m: &Metrics, scale: f32) -> RECT {
    let px = |pt: f32| (pt * scale).round() as i32;
    let w = px(width).min(window.0 - 2 * px(SIDE)).max(0);
    let left = (window.0 - w) / 2;
    RECT { left, top: (window.1 - px(m.height)).max(0), right: left + w, bottom: window.1 }
}

/// The label for an icon whose centre is `center_x` points from the window's left, `text_w`
/// points of text, above glass whose top is `glass_top` points down: its pill, in points, kept
/// inside a window `window_w` points wide.
pub fn label_rect(center_x: f32, text_w: f32, glass_top: f32, window_w: f32) -> egui::Rect {
    let w = text_w + 2.0 * 11.0;
    let left = (center_x - w / 2.0).clamp(SIDE, (window_w - SIDE - w).max(SIDE));
    let bottom = glass_top - LABEL_GAP;
    egui::Rect::from_min_max(egui::pos2(left, bottom - LABEL_H), egui::pos2(left + w, bottom))
}

/// The part of the window the label needs shown (its pill and shadow), in pixels.
pub fn label_region(label: egui::Rect, scale: f32) -> RECT {
    let r = label.expand(LABEL_SHADOW);
    RECT {
        left: (r.left() * scale).floor() as i32,
        top: (r.top() * scale).floor().max(0.0) as i32,
        right: (r.right() * scale).ceil() as i32,
        bottom: (r.bottom() * scale).ceil() as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(section: Section) -> Item {
        Item { section, app_id: String::new(), exe: None, windows: Vec::new() }
    }

    const M: Metrics = Metrics { icon: 48.0, pad: 11.0, height: 70.0, radius: 22.0 };

    #[test]
    fn sizes_follow_the_icon() {
        assert_eq!(Metrics::new(IconSize::Medium), M);
        let [s, l] = [IconSize::Small, IconSize::Large].map(Metrics::new);
        assert_eq!((s.height, l.height), (62.0, 78.0));
        assert_eq!((s.radius, l.radius), (20.0, 23.0));
    }

    #[test]
    fn dividers_sit_between_sections() {
        let items = [
            item(Section::Launcher),
            item(Section::Files),
            item(Section::Pinned),
            item(Section::Running),
            item(Section::Running),
            item(Section::Bin),
        ];
        let (xs, total) = layout(&items, &M);
        let step = 48.0 + SPACING;
        assert_eq!(xs[0], PAD);
        // No divider between the Launcher, Files and the pins.
        assert_eq!((xs[1], xs[2]), (PAD + step, PAD + 2.0 * step));
        // One before the first running app and one before the bin, none inside a section.
        assert_eq!(xs[3], xs[2] + step + DIVIDER);
        assert_eq!(xs[4], xs[3] + step);
        assert_eq!(xs[5], xs[4] + step + DIVIDER);
        assert_eq!(total, xs[5] + 48.0 + PAD);
        // Without running apps, only the one before the bin.
        let (_, total) = layout(&[item(Section::Launcher), item(Section::Files), item(Section::Bin)], &M);
        assert_eq!(total, 2.0 * PAD + 3.0 * 48.0 + 2.0 * SPACING + DIVIDER);
    }

    #[test]
    fn a_lone_running_app_first_has_no_divider() {
        assert!(!divider_before(&[item(Section::Running)], 0));
    }

    #[test]
    fn glass_is_centred_along_the_bottom() {
        // A 4K monitor at 150%: 3840 px wide, the window 110 pt tall.
        let r = glass_rect((3840, 165), 400.0, &M, 1.5);
        assert_eq!(r, RECT { left: 1620, top: 60, right: 2220, bottom: 165 });
        assert_eq!(r.bottom - r.top, 105);
        // Too wide for the window: kept inside, a little in from both sides.
        let r = glass_rect((600, 110), 900.0, &M, 1.0);
        assert_eq!((r.left, r.right), (8, 592));
    }

    #[test]
    fn labels_sit_above_the_glass_and_stay_on_screen() {
        let l = label_rect(500.0, 60.0, ABOVE, 1000.0);
        assert_eq!(l.bottom(), ABOVE - LABEL_GAP);
        assert!(l.top() >= 0.0, "the label fits in the room above the glass");
        assert_eq!(l.center().x, 500.0);
        let l = label_rect(5.0, 60.0, ABOVE, 1000.0);
        assert_eq!(l.left(), SIDE);
        let l = label_rect(995.0, 60.0, ABOVE, 1000.0);
        assert_eq!(l.right(), 1000.0 - SIDE);
        // Its region covers the pill and its shadow, in whole pixels.
        let r = label_region(egui::Rect::from_min_max(egui::pos2(10.0, 6.0), egui::pos2(50.0, 32.0)), 1.5);
        assert_eq!(r, RECT { left: 6, top: 0, right: 84, bottom: 57 });
    }
}
