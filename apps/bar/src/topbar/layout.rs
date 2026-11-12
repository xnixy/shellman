//! Where the top bar's three islands go in its window. Pure arithmetic, tested without windows.
//!
//! The window spans its monitor's width and is `HEIGHT` tall, all of it reserved: the islands
//! float `MARGIN` below the screen's top edge, and `BELOW` more is kept free under them so a
//! maximized window's edge never touches their glass.

/// From the screen's top edge to the islands.
pub const MARGIN: f32 = 12.0;
/// The islands' height.
pub const ISLAND_H: f32 = 40.0;
/// Free space under the islands, above maximized windows.
pub const BELOW: f32 = 8.0;
/// The top bar window's height, which is also the space it reserves.
pub const HEIGHT: f32 = MARGIN + ISLAND_H + BELOW;
/// From the screen's sides to the outer islands.
pub const SIDE: f32 = 16.0;
/// The least space kept between two islands.
pub const SPACE: f32 = 12.0;

/// The band the islands sit in, in a window whose client area is `bar`.
pub fn row(bar: egui::Rect) -> egui::Rect {
    let top = bar.top() + MARGIN;
    egui::Rect::from_x_y_ranges(bar.x_range(), top..=top + ISLAND_H)
}

/// The centre island's left edge for content `width` points wide: centred on the bar, unless that
/// would run into the right island (which begins at `right_starts`), then just left of it.
pub fn centre_left(bar: egui::Rect, width: f32, right_starts: f32) -> f32 {
    let centred = bar.center().x - width / 2.0;
    centred.min(right_starts - SPACE - width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, Rect};

    #[test]
    fn the_islands_float_below_the_edge_and_leave_room_under_them() {
        let bar = Rect::from_min_max(pos2(0.0, 0.0), pos2(2560.0, HEIGHT));
        let r = row(bar);
        assert_eq!((r.top(), r.bottom()), (12.0, 52.0));
        assert_eq!(bar.bottom() - r.bottom(), BELOW);
        assert_eq!(HEIGHT, 60.0);
    }

    #[test]
    fn the_clock_is_centred_unless_the_right_island_is_in_the_way() {
        let bar = Rect::from_min_max(pos2(0.0, 0.0), pos2(1280.0, HEIGHT));
        assert_eq!(centre_left(bar, 140.0, 1000.0), 570.0);
        // A wide tray on a narrow monitor: the clock steps left of it.
        assert_eq!(centre_left(bar, 140.0, 650.0), 650.0 - SPACE - 140.0);
    }
}
