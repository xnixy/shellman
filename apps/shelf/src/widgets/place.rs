//! The widgets' sizes: the design's "Shelf + Widgets" (372 pt wide, 40 pt from the right edge,
//! 16 pt under the top bar, Today above Now playing, 16 pt apart), and how tall each is for
//! what it shows. Where they go, by default and once arranged, is `crate::arrange::place`; each
//! widget's window is as big as the widget at its tallest plus its shadow. Pure.

/// The widgets' width.
pub const WIDTH: f32 = 372.0;
/// From the work area's right edge to the widgets.
pub const RIGHT: f32 = 40.0;
/// From the work area's top (the top bar's lower edge) to the first widget.
pub const TOP: f32 = 16.0;
pub const RADIUS: f32 = shellman_ui::theme::shape::ISLAND;

/// Today: padding (top, sides, bottom).
pub const PAD_TOP: f32 = 22.0;
pub const PAD_X: f32 = 24.0;
pub const PAD_BOTTOM: f32 = 12.0;
/// The date block: weekday, the day's number, the month.
pub const HEADER_H: f32 = 116.0;
/// Between the date and the first row.
pub const ROWS_GAP: f32 = 16.0;
pub const ROW_H: f32 = 65.0;
/// The "Tomorrow" heading over tomorrow's rows.
pub const HEADING_H: f32 = 34.0;
/// A note in place of rows ("Sign-in failed" over a line saying what to do), and one of a single
/// line ("Nothing on today or tomorrow").
pub const NOTE_H: f32 = 65.0;
pub const NOTE_ONE_H: f32 = 46.0;
/// The most event rows Today lists.
pub const MAX_ROWS: usize = 3;

/// Now playing: padding and the art's side.
pub const PLAY_PAD: f32 = 12.0;
pub const ART: f32 = 54.0;

/// What Today shows below its date, for its height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TodayShape {
    pub rows: usize,
    pub heading: bool,
    /// The lines of a note standing in for rows: 0 (none), 1 or 2.
    pub note: u8,
}

pub fn today_height(s: TodayShape) -> f32 {
    let mut h = PAD_TOP + HEADER_H + ROWS_GAP + s.rows.min(MAX_ROWS) as f32 * ROW_H + PAD_BOTTOM;
    if s.heading {
        h += HEADING_H;
    }
    h += match s.note {
        0 => 0.0,
        1 => NOTE_ONE_H,
        _ => NOTE_H,
    };
    h
}

pub fn playing_height() -> f32 {
    2.0 * PLAY_PAD + ART
}

/// The tallest Today can be: with a heading and every row (a note only stands in for rows).
pub fn today_max() -> f32 {
    today_height(TodayShape { rows: MAX_ROWS, heading: true, note: 0 }).max(today_height(TodayShape { rows: 0, heading: true, note: 2 }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_follow_the_content() {
        let none = today_height(TodayShape { rows: 0, heading: false, note: 2 });
        let two = today_height(TodayShape { rows: 2, heading: false, note: 0 });
        assert_eq!(two - (none - NOTE_H), 2.0 * ROW_H);
        // The tallest is with every row and the heading, never more rows than that.
        assert_eq!(today_max(), today_height(TodayShape { rows: MAX_ROWS + 4, heading: true, note: 0 }));
        assert_eq!(playing_height(), 2.0 * PLAY_PAD + ART);
    }
}
