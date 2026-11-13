//! How a popup menu measures and looks (the Islands mockups' menu, `docs/design`): a rounded
//! glass island with 8 pt of padding, 40 pt rows with an icon, the label and a quiet hint or
//! chevron, 12 pt rounded highlights, hairline separators and small headings.

use super::model::{Row, RowKind};
use crate::theme::{self, colors, glyph, icon_font};
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Stroke, Vec2};

/// The island's padding and corner radius.
pub const PAD: f32 = 8.0;
pub const RADIUS: f32 = 20.0;
pub const ROW_H: f32 = 40.0;
const ROW_RADIUS: u8 = 12;
const SEPARATOR_H: f32 = 13.0;
const HEADING_H: f32 = 28.0;
/// Inside a row: the left and right padding, the icon column, and the gaps.
const SIDE: f32 = 12.0;
const ICON_W: f32 = 18.0;
const GAP: f32 = 12.0;
const CHEVRON_W: f32 = 12.0;
const LABEL_SIZE: f32 = 14.0;
const HINT_SIZE: f32 = 12.0;
const MIN_W: f32 = 240.0;
const MAX_W: f32 = 420.0;
/// Between the menu and its submenu.
pub const SUB_GAP: f32 = 6.0;

/// One level (the menu or its submenu) laid out in points, from its own top-left.
#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    pub size: Vec2,
    /// Each row's top and height.
    pub rows: Vec<(f32, f32)>,
    /// Whether rows keep a column for icons and ticks.
    pub icons: bool,
}

fn row_height(r: &Row) -> f32 {
    match r.kind {
        RowKind::Separator => SEPARATOR_H,
        RowKind::Heading => HEADING_H,
        _ => ROW_H,
    }
}

/// Lay out `rows`, measuring text with `text_w(text, size)` (points).
pub fn measure(rows: &[Row], text_w: impl Fn(&str, f32) -> f32) -> Level {
    let icons = rows.iter().any(|r| r.icon.is_some() || r.checked);
    let label_x = if icons { SIDE + ICON_W + GAP } else { SIDE };
    let mut y = PAD;
    let mut out = Vec::with_capacity(rows.len());
    let mut widest: f32 = 0.0;
    for r in rows {
        let h = row_height(r);
        out.push((y, h));
        y += h;
        let w = match &r.kind {
            RowKind::Separator => 0.0,
            RowKind::Heading => SIDE + text_w(&r.label, HINT_SIZE) + SIDE,
            RowKind::Submenu(_) => label_x + text_w(&r.label, LABEL_SIZE) + GAP + CHEVRON_W + SIDE,
            RowKind::Item(_) => {
                let hint = if r.hint.is_empty() { 0.0 } else { GAP * 2.0 + text_w(&r.hint, HINT_SIZE) };
                label_x + text_w(&r.label, LABEL_SIZE) + hint + SIDE
            }
        };
        widest = widest.max(w);
    }
    // A little slack: text measured at one scale is drawn at another.
    let width = (widest + 2.0 * PAD + 4.0).clamp(MIN_W, MAX_W).ceil();
    Level { size: vec2(width, (y + PAD).max(2.0 * PAD + ROW_H)), rows: out, icons }
}

/// `rows` measured with egui's fonts.
pub fn measure_with(ctx: &egui::Context, rows: &[Row]) -> Level {
    measure(rows, |text, size| {
        ctx.fonts(|f| f.layout_no_wrap(text.to_owned(), FontId::proportional(size), Color32::WHITE).size().x)
    })
}

/// Which row of `level` (placed at `origin`) is under `p`: None when `p` is outside the level,
/// Some(None) when it is inside but on no row (padding).
pub fn row_at(level: &Level, origin: Pos2, p: Pos2) -> Option<Option<usize>> {
    let rect = Rect::from_min_size(origin, level.size);
    if !rect.contains(p) {
        return None;
    }
    let y = p.y - origin.y;
    let inside_x = p.x >= origin.x + PAD && p.x < origin.x + level.size.x - PAD;
    Some(level.rows.iter().position(|&(top, h)| inside_x && y >= top && y < top + h))
}

/// What a level shows lit.
pub struct Lit {
    pub hot: Option<usize>,
    /// The row whose submenu is open.
    pub open: Option<usize>,
}

/// Paint one level's rows (the glass under them is the panel's, or the caller's fallback).
pub fn paint(painter: &Painter, origin: Pos2, level: &Level, rows: &[Row], lit: Lit) {
    let c = colors();
    let width = level.size.x;
    let label_x = if level.icons { SIDE + ICON_W + GAP } else { SIDE };
    for (i, (r, &(top, h))) in rows.iter().zip(&level.rows).enumerate() {
        let rect = Rect::from_min_size(pos2(origin.x + PAD, origin.y + top), vec2(width - 2.0 * PAD, h));
        match r.kind {
            RowKind::Separator => {
                let y = rect.center().y.round() + 0.5;
                // The mockup's 8 % line, as it shows over glass (see `islands::shows_as`).
                let line = if c.light { Color32::from_black_alpha(18) } else { Color32::from_white_alpha(22) };
                let line = crate::glass::islands::shows_as(line);
                painter.hline(rect.x_range().shrink(SIDE - 4.0), y, Stroke::new(1.0_f32, line));
                continue;
            }
            RowKind::Heading => {
                let at = pos2(rect.left() + SIDE, rect.bottom() - 6.0);
                let text = rect.width() - 2.0 * SIDE;
                label(painter, at, text, &r.label, HINT_SIZE, c.muted, Align2::LEFT_BOTTOM);
                continue;
            }
            _ => {}
        }
        let open = lit.open == Some(i);
        let hot = lit.hot == Some(i) || open;
        if hot && r.enabled {
            // The mockup's quiet hover (--hover: 9 % white, 6 % black), as it shows over glass.
            // The accent is for what is checked, not for where the pointer is.
            let hover = if c.light { Color32::from_black_alpha(15) } else { Color32::from_white_alpha(23) };
            painter.rect_filled(rect, CornerRadius::same(ROW_RADIUS), crate::glass::islands::shows_as(hover));
        }
        let fg = match () {
            _ if !r.enabled => theme::with_alpha(c.muted, if c.light { 190 } else { 140 }),
            _ if r.danger => c.danger_text,
            _ => c.text,
        };
        let icon_fg = match () {
            _ if !r.enabled || r.danger => fg,
            _ if hot => c.text,
            _ => c.muted,
        };
        let cy = rect.center().y;
        if level.icons {
            let at = pos2(rect.left() + SIDE + ICON_W / 2.0, cy);
            match (r.icon, r.checked) {
                (Some(g), checked) => {
                    if checked {
                        let well = Rect::from_center_size(at, Vec2::splat(26.0));
                        painter.rect_filled(well, CornerRadius::same(7), c.row_selected);
                    }
                    painter.text(at, Align2::CENTER_CENTER, g, icon_font(15.0), icon_fg);
                }
                (None, true) => {
                    painter.text(at, Align2::CENTER_CENTER, glyph::CHECK, icon_font(13.0), fg);
                }
                (None, false) => {}
            }
        }
        let mut right_w = 0.0;
        let right_x = rect.right() - SIDE;
        if let RowKind::Submenu(_) = r.kind {
            let col = if open || hot { c.text } else { c.muted };
            painter.text(pos2(right_x, cy), Align2::RIGHT_CENTER, glyph::CHEVRON, icon_font(10.0), col);
            right_w = CHEVRON_W + GAP;
        } else if !r.hint.is_empty() {
            let g = painter.text(pos2(right_x, cy), Align2::RIGHT_CENTER, &r.hint, FontId::proportional(HINT_SIZE), c.muted);
            right_w = g.width() + GAP * 2.0;
        }
        let room = rect.width() - label_x - SIDE - right_w;
        label(painter, pos2(rect.left() + label_x, cy), room, &r.label, LABEL_SIZE, fg, Align2::LEFT_CENTER);
    }
}

/// One line of text, cut with an ellipsis past `room`.
fn label(painter: &Painter, at: Pos2, room: f32, text: &str, size: f32, color: Color32, align: Align2) {
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat { font_id: FontId::proportional(size), color, ..Default::default() },
    );
    job.wrap = TextWrapping::truncate_at_width(room.max(10.0));
    let galley = painter.ctx().fonts(|f| f.layout_job(job));
    let pos = align.anchor_size(at, galley.size()).min;
    painter.galley(pos, galley, color);
}

#[cfg(test)]
mod tests {
    use super::super::model::{flatten, Entry};
    use super::*;

    /// Every character 7 points wide.
    fn w(text: &str, _size: f32) -> f32 {
        text.chars().count() as f32 * 7.0
    }

    #[test]
    fn rows_stack_from_the_padding_and_the_width_has_limits() {
        let rows = flatten(
            vec![Entry::item(0, "Open"), Entry::separator(), Entry::heading("More"), Entry::item(1, "Close")],
            &mut Vec::new(),
            0,
        );
        let l = measure(&rows, w);
        assert_eq!(l.rows[0], (PAD, ROW_H));
        assert_eq!(l.rows[1].0, PAD + ROW_H);
        assert_eq!(l.rows[3].0, PAD + ROW_H + SEPARATOR_H + HEADING_H);
        assert_eq!(l.size.y, PAD * 2.0 + ROW_H * 2.0 + SEPARATOR_H + HEADING_H);
        assert_eq!(l.size.x, MIN_W);
        assert!(!l.icons);
        let long = flatten(vec![Entry::item(0, "x".repeat(200)).icon('a')], &mut Vec::new(), 0);
        let l = measure(&long, w);
        assert_eq!(l.size.x, MAX_W);
        assert!(l.icons);
        // Even an empty menu is a sane size.
        let l = measure(&[], w);
        assert!(l.size.y >= ROW_H);
    }

    #[test]
    fn hints_make_room() {
        let plain = flatten(vec![Entry::item(0, "x".repeat(30))], &mut Vec::new(), 0);
        let hinted = flatten(vec![Entry::item(0, "x".repeat(30)).hint("Ctrl Shift V")], &mut Vec::new(), 0);
        assert!(measure(&hinted, w).size.x > measure(&plain, w).size.x);
    }

    #[test]
    fn hit_testing_finds_rows_and_padding() {
        let rows = flatten(vec![Entry::item(0, "a"), Entry::separator(), Entry::item(1, "b")], &mut Vec::new(), 0);
        let l = measure(&rows, w);
        let o = pos2(100.0, 50.0);
        assert_eq!(row_at(&l, o, pos2(150.0, 50.0 + PAD + 5.0)), Some(Some(0)));
        assert_eq!(row_at(&l, o, pos2(150.0, 50.0 + PAD + ROW_H + 3.0)), Some(Some(1)));
        assert_eq!(row_at(&l, o, pos2(150.0, 50.0 + 2.0)), Some(None));
        assert_eq!(row_at(&l, o, pos2(99.0, 60.0)), None);
    }
}
