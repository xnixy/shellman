//! The dock's measurements: how the icons sit on the glass, how big its two islands are (the
//! apps, and the Recycle Bin on its own just to the right), and where they and the name label go
//! in the dock window. Pure arithmetic, tested without windows.
//!
//! The dock window is as wide as its monitor's band and taller than the glass by `ABOVE`, room
//! for the name label. It is placed once; as apps come and go only the islands inside it change
//! (see `shellman_ui::glass::GlassPanel::set_islands_only`), so its icons never jump while a
//! resized window catches up with its content. Outside the islands (and the label, while shown)
//! the window is neither drawn nor clicked.

use super::items::{Item, Section};
use shellman_core::settings::dock::IconSize;
use windows::Win32::Foundation::RECT;

/// Space between the glass's edge and the icons, on every side.
const PAD: f32 = 11.0;
/// Space between two icons.
const SPACING: f32 = 8.0;
/// Extra space a divider adds between two sections.
const DIVIDER: f32 = 12.0;
/// Space between the apps island and the bin's.
pub const ISLAND_GAP: f32 = 10.0;
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
    /// The islands' corner: the icons' own corner plus the padding, so the two curves run
    /// together, a little rounder still (24 pt at the medium size, as designed).
    pub radius: f32,
}

impl Metrics {
    pub fn new(size: IconSize) -> Self {
        let icon = size.icon();
        Self {
            icon,
            pad: PAD,
            height: icon + 2.0 * PAD,
            radius: (icon * 0.22 + PAD + 2.0).round(),
        }
    }
}

/// A divider goes before the first running app (after the Launcher, Files and the pins). The bin
/// needs none: it has an island of its own.
pub fn divider_before(items: &[Item], i: usize) -> bool {
    i > 0 && items[i].section == Section::Running && items[i - 1].section != Section::Running
}

/// Left edge of each icon from its island's left edge, and the apps island's width, in points.
/// The bin sits alone on the bin island, so its left edge is the padding.
pub fn layout(items: &[Item], m: &Metrics) -> (Vec<f32>, f32) {
    let mut x = m.pad;
    let mut first = true;
    let mut xs = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        if item.section == Section::Bin {
            xs.push(m.pad);
            continue;
        }
        if !first {
            x += SPACING;
        }
        if divider_before(items, i) {
            x += DIVIDER;
        }
        xs.push(x);
        x += m.icon;
        first = false;
    }
    (xs, x + m.pad)
}

/// The dock's islands in the dock window, in points: the apps island, `apps_w` wide and centred
/// along the bottom of a window `window` points in size, and with `bin`, the bin's island an
/// `ISLAND_GAP` to its right. Both stay a little in from the window's sides; when they would not
/// fit, the apps island moves left and then narrows.
pub fn islands(window: egui::Vec2, apps_w: f32, bin: bool, m: &Metrics) -> (egui::Rect, Option<egui::Rect>) {
    let bin_w = m.icon + 2.0 * m.pad;
    let extra = if bin { ISLAND_GAP + bin_w } else { 0.0 };
    let room = (window.x - 2.0 * SIDE - extra).max(0.0);
    let w = apps_w.min(room);
    let mut left = (window.x - w) / 2.0;
    left = left.min(window.x - SIDE - extra - w).max(SIDE);
    let top = (window.y - m.height).max(0.0);
    let apps = egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(w, m.height));
    let bin = bin.then(|| {
        egui::Rect::from_min_size(egui::pos2(apps.right() + ISLAND_GAP, top), egui::vec2(bin_w, m.height))
    });
    (apps, bin)
}

/// Where something dropped between pins would go: for each pinned slot (0 right after Files, up
/// to after the last pin), the x of the boundary, in the same space as `lefts` (each icon's left
/// edge). Halfway between the two icons, or half a spacing past the last pin.
pub fn pin_gaps(items: &[Item], lefts: &[f32], m: &Metrics) -> Vec<(f32, usize)> {
    let Some(files) = items.iter().position(|i| i.section == Section::Files) else { return Vec::new() };
    let pinned: Vec<usize> = items.iter().enumerate().filter(|(_, i)| i.section == Section::Pinned).map(|(k, _)| k).collect();
    let mut out = Vec::with_capacity(pinned.len() + 1);
    for slot in 0..=pinned.len() {
        let left = if slot == 0 { files } else { pinned[slot - 1] };
        let right = pinned.get(slot).copied().or_else(|| {
            items.get(left + 1).filter(|i| i.section == Section::Running).map(|_| left + 1)
        });
        let edge = lefts[left] + m.icon;
        let x = match right {
            Some(r) => (edge + lefts[r]) / 2.0,
            None => edge + SPACING / 2.0,
        };
        out.push((x, slot));
    }
    out
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
        Item::new(section, "", None)
    }

    const M: Metrics = Metrics { icon: 48.0, pad: 11.0, height: 70.0, radius: 24.0 };

    #[test]
    fn sizes_follow_the_icon() {
        assert_eq!(Metrics::new(IconSize::Medium), M);
        let [s, l] = [IconSize::Small, IconSize::Large].map(Metrics::new);
        assert_eq!((s.height, l.height), (62.0, 78.0));
        assert_eq!((s.radius, l.radius), (22.0, 25.0));
    }

    #[test]
    fn dividers_sit_between_sections_and_the_bin_stands_apart() {
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
        // One before the first running app, none inside a section.
        assert_eq!(xs[3], xs[2] + step + DIVIDER);
        assert_eq!(xs[4], xs[3] + step);
        // The bin is on its own island; the apps island ends after the last app.
        assert!(!divider_before(&items, 5));
        assert_eq!(xs[5], PAD);
        assert_eq!(total, xs[4] + 48.0 + PAD);
        // Without running apps, no divider at all.
        let (_, total) = layout(&[item(Section::Launcher), item(Section::Files), item(Section::Bin)], &M);
        assert_eq!(total, 2.0 * PAD + 2.0 * 48.0 + SPACING);
    }

    #[test]
    fn pin_gaps_sit_between_files_and_the_pins() {
        let items = [
            item(Section::Launcher),
            item(Section::Files),
            item(Section::Pinned),
            item(Section::Pinned),
            item(Section::Running),
            item(Section::Bin),
        ];
        let (xs, _) = layout(&items, &M);
        let gaps = pin_gaps(&items, &xs, &M);
        assert_eq!(gaps.len(), 3);
        assert_eq!(gaps[0], (xs[1] + 48.0 + SPACING / 2.0, 0));
        assert_eq!(gaps[1], (xs[2] + 48.0 + SPACING / 2.0, 1));
        // After the last pin, before the divider: halfway across it.
        assert_eq!(gaps[2], ((xs[3] + 48.0 + xs[4]) / 2.0, 2));
        // No pins: one slot, after Files.
        let few = [item(Section::Launcher), item(Section::Files), item(Section::Bin)];
        let (xs, _) = layout(&few, &M);
        assert_eq!(pin_gaps(&few, &xs, &M), vec![(xs[1] + 48.0 + SPACING / 2.0, 0)]);
    }

    #[test]
    fn a_lone_running_app_first_has_no_divider() {
        assert!(!divider_before(&[item(Section::Running)], 0));
    }

    #[test]
    fn the_apps_island_is_centred_and_the_bin_follows() {
        // A 4K monitor at 150%: 2560 pt wide, the window 110 pt tall.
        let (apps, bin) = islands(egui::vec2(2560.0, 110.0), 400.0, true, &M);
        assert_eq!((apps.left(), apps.right()), (1080.0, 1480.0));
        assert_eq!((apps.top(), apps.bottom()), (40.0, 110.0));
        let bin = bin.unwrap();
        assert_eq!(bin.left(), apps.right() + ISLAND_GAP);
        assert_eq!((bin.width(), bin.height()), (70.0, 70.0));
        assert_eq!(bin.y_range(), apps.y_range());
        // Without a bin, the apps island alone.
        assert!(islands(egui::vec2(2560.0, 110.0), 400.0, false, &M).1.is_none());
    }

    #[test]
    fn islands_stay_inside_a_narrow_window() {
        // Room for both only once the apps island moves left of centre.
        let (apps, bin) = islands(egui::vec2(600.0, 110.0), 480.0, true, &M);
        assert_eq!(bin.unwrap().right(), 600.0 - SIDE);
        assert_eq!(apps.width(), 480.0);
        // Too wide even then: it narrows, a little in from both sides.
        let (apps, bin) = islands(egui::vec2(600.0, 110.0), 900.0, true, &M);
        assert_eq!(apps.left(), SIDE);
        assert_eq!(bin.unwrap().right(), 600.0 - SIDE);
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
