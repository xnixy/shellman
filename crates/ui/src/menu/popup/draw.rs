//! How a popup menu measures and looks (the Islands mockups' menu, `docs/design`): a rounded
//! glass island with 8 pt of padding, 40 pt rows with an icon, the label and a quiet hint or
//! chevron, 12 pt rounded highlights, hairline separators and small headings. A level taller
//! than its monitor scrolls, with a thin bar on the right.

use super::model::{Row, RowKind};
use crate::theme::{self, colors, glyph, icon_font};
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Stroke, TextureId, Vec2};

/// The island's padding and corner radius.
pub const PAD: f32 = 8.0;
pub const RADIUS: f32 = 20.0;
pub const ROW_H: f32 = 40.0;
const ROW_RADIUS: u8 = 12;
const SEPARATOR_H: f32 = 13.0;
const HEADING_H: f32 = 28.0;
const NOTE_H: f32 = 34.0;
/// Inside a row: the left and right padding, the icon column, and the gaps.
const SIDE: f32 = 12.0;
const ICON_W: f32 = 18.0;
/// A picture in the icon column.
const IMAGE: f32 = 16.0;
const GAP: f32 = 12.0;
const CHEVRON_W: f32 = 12.0;
const LABEL_SIZE: f32 = 14.0;
const HINT_SIZE: f32 = 12.0;
const MIN_W: f32 = 240.0;
const MAX_W: f32 = 420.0;
/// Between the menu and its submenu.
pub const SUB_GAP: f32 = 6.0;
/// The scroll bar's width, inside the right padding.
const BAR_W: f32 = 3.0;
/// The shortest a level is made when its monitor is very short.
const MIN_H: f32 = 2.0 * PAD + 3.0 * ROW_H;

/// One level (the menu or a submenu) laid out in points, from its own top-left.
#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    /// What is shown: the whole level, or as much as fits (then it scrolls).
    pub size: Vec2,
    /// The whole level's height, padding included.
    pub content: f32,
    /// Each row's top and height, in the content (before scrolling).
    pub rows: Vec<(f32, f32)>,
    /// Whether rows keep a column for icons and ticks.
    pub icons: bool,
}

impl Level {
    /// How far it can scroll.
    pub fn max_scroll(&self) -> f32 {
        (self.content - self.size.y).max(0.0)
    }

    /// The scroll that brings row `i` into view from `scroll`.
    pub fn reveal(&self, i: usize, scroll: f32) -> f32 {
        let Some(&(top, h)) = self.rows.get(i) else { return scroll };
        let view = self.size.y - 2.0 * PAD;
        let s = if top - PAD < scroll {
            top - PAD
        } else if top + h - PAD > scroll + view {
            top + h - PAD - view
        } else {
            scroll
        };
        s.clamp(0.0, self.max_scroll())
    }
}

fn row_height(r: &Row) -> f32 {
    match r.kind {
        RowKind::Separator => SEPARATOR_H,
        RowKind::Heading => HEADING_H,
        RowKind::Note => NOTE_H,
        RowKind::Picture(_, _, size) => (size.y + 6.0).max(ROW_H),
        _ => ROW_H,
    }
}

/// Lay out `rows`, measuring text with `text_w(text, size)` (points), at most `max_h` points tall
/// (the monitor's work area).
pub fn measure(rows: &[Row], text_w: impl Fn(&str, f32) -> f32, max_h: f32) -> Level {
    let icons = rows.iter().any(Row::has_icon);
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
            RowKind::Note => label_x + text_w(&r.label, HINT_SIZE) + SIDE,
            RowKind::Picture(_, _, size) => size.x + 2.0 * SIDE,
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
    let content = (y + PAD).max(2.0 * PAD + ROW_H);
    let limit = if max_h.is_finite() { max_h.max(MIN_H) } else { content };
    Level { size: vec2(width, content.min(limit).floor()), content, rows: out, icons }
}

/// `rows` measured with egui's fonts.
pub fn measure_with(ctx: &egui::Context, rows: &[Row], max_h: f32) -> Level {
    measure(
        rows,
        |text, size| ctx.fonts(|f| f.layout_no_wrap(text.to_owned(), FontId::proportional(size), Color32::WHITE).size().x),
        max_h,
    )
}

/// Which row of `level` (placed at `origin`, scrolled by `scroll`) is under `p`: None when `p` is
/// outside the level, Some(None) when it is inside but on no row (padding).
pub fn row_at(level: &Level, origin: Pos2, scroll: f32, p: Pos2) -> Option<Option<usize>> {
    let rect = Rect::from_min_size(origin, level.size);
    if !rect.contains(p) {
        return None;
    }
    let inside_x = p.x >= origin.x + PAD && p.x < origin.x + level.size.x - PAD;
    let inside_y = p.y >= origin.y + PAD && p.y < origin.y + level.size.y - PAD;
    let y = p.y - origin.y + scroll;
    Some(level.rows.iter().position(|&(top, h)| inside_x && inside_y && y >= top && y < top + h))
}

/// What a level shows lit.
pub struct Lit {
    pub hot: Option<usize>,
    /// The row whose submenu is open.
    pub open: Option<usize>,
}

/// What a screen reader is told about a row: its role, whether it is checked (for a row that
/// can be), and its name. None for what is not read on its own (separators). Headings and notes
/// are labels; a submenu says so after its name; a hint (a shortcut, a folder) follows the name.
pub fn spoken(r: &Row) -> Option<(egui::WidgetType, Option<bool>, String)> {
    let name = |extra: &str| {
        let mut s = r.label.clone();
        if s.trim().is_empty() {
            s = "Picture".to_owned();
        }
        if !r.hint.is_empty() {
            s = format!("{s}, {}", r.hint);
        }
        s + extra
    };
    match &r.kind {
        RowKind::Separator => None,
        RowKind::Heading | RowKind::Note => Some((egui::WidgetType::Label, None, r.label.clone())),
        RowKind::Submenu(_) => Some((egui::WidgetType::Button, None, name(", submenu"))),
        RowKind::Item(_) | RowKind::Picture(..) if r.checked => Some((egui::WidgetType::Checkbox, Some(true), name(""))),
        RowKind::Item(_) | RowKind::Picture(..) => Some((egui::WidgetType::Button, None, name(""))),
    }
}

/// Name one level's visible rows for screen readers: a node over each, with its role and name,
/// the lit row holding the focus (so moving through the menu is read out). The menu reads its own
/// pointer and keys; a screen reader's click lands on the row as a mouse click.
pub fn speak(ui: &egui::Ui, origin: Pos2, level: &Level, rows: &[Row], hot: Option<usize>, scroll: f32, depth: usize) {
    let view = Rect::from_min_size(pos2(origin.x, origin.y + PAD), vec2(level.size.x, level.size.y - 2.0 * PAD));
    for (i, (r, &(top, h))) in rows.iter().zip(&level.rows).enumerate() {
        let rect = Rect::from_min_size(pos2(origin.x + PAD, origin.y + top - scroll), vec2(level.size.x - 2.0 * PAD, h));
        if !rect.intersects(view) {
            continue;
        }
        let Some((role, checked, name)) = spoken(r) else { continue };
        let resp = ui.interact(rect.intersect(view), egui::Id::new(("menu row", depth, i)), egui::Sense::hover());
        let lit = hot == Some(i);
        let enabled = r.enabled;
        crate::access::describe_focused(&resp, lit, || {
            let mut info = egui::WidgetInfo::labeled(role, enabled, name.clone());
            info.selected = checked;
            info
        });
    }
}

/// Paint one level's rows, scrolled by `scroll` (the glass under them is the panel's, or the
/// caller's fallback).
pub fn paint(painter: &Painter, origin: Pos2, level: &Level, rows: &[Row], lit: Lit, scroll: f32) {
    let c = colors();
    let width = level.size.x;
    let label_x = if level.icons { SIDE + ICON_W + GAP } else { SIDE };
    let view = Rect::from_min_size(pos2(origin.x, origin.y + PAD), vec2(width, level.size.y - 2.0 * PAD));
    let painter = painter.with_clip_rect(view.intersect(painter.clip_rect()));
    for (i, (r, &(top, h))) in rows.iter().zip(&level.rows).enumerate() {
        let rect = Rect::from_min_size(pos2(origin.x + PAD, origin.y + top - scroll), vec2(width - 2.0 * PAD, h));
        if !rect.intersects(view) {
            continue;
        }
        let cy = rect.center().y;
        match &r.kind {
            RowKind::Separator => {
                let y = cy.round() + 0.5;
                // The mockup's 8 % line, as it shows over glass (see `islands::shows_as`).
                let line = if c.light { Color32::from_black_alpha(18) } else { Color32::from_white_alpha(22) };
                let line = if c.high_contrast { c.text } else { crate::glass::islands::shows_as(line) };
                painter.hline(rect.x_range().shrink(SIDE - 4.0), y, Stroke::new(1.0_f32, line));
                continue;
            }
            RowKind::Heading => {
                let at = pos2(rect.left() + SIDE, rect.bottom() - 6.0);
                let text = rect.width() - 2.0 * SIDE;
                label(&painter, at, text, &r.label, HINT_SIZE, c.muted, Align2::LEFT_BOTTOM);
                continue;
            }
            RowKind::Note => {
                let at = pos2(rect.left() + label_x, cy);
                let text = rect.width() - label_x - SIDE;
                label(&painter, at, text, &r.label, HINT_SIZE, c.muted, Align2::LEFT_CENTER);
                continue;
            }
            _ => {}
        }
        let open = lit.open == Some(i);
        let hot = lit.hot == Some(i) || open;
        // High contrast: the lit row is highlight under highlight text, as Windows' menus are.
        let hc_lit = c.high_contrast && hot && r.enabled;
        if hc_lit {
            painter.rect_filled(rect, CornerRadius::same(ROW_RADIUS), c.accent_fill);
        } else if hot && r.enabled && !c.high_contrast {
            // The mockup's quiet hover (--hover: 9 % white, 6 % black), as it shows over glass.
            // The accent is for what is checked, not for where the pointer is.
            let hover = if c.light { Color32::from_black_alpha(15) } else { Color32::from_white_alpha(23) };
            painter.rect_filled(rect, CornerRadius::same(ROW_RADIUS), crate::glass::islands::shows_as(hover));
        }
        if let RowKind::Picture(_, tex, size) = r.kind {
            let at = Rect::from_min_size(pos2(rect.left() + SIDE, cy - size.y / 2.0), size);
            image(&painter, tex, at, r.enabled);
            continue;
        }
        let fg = match () {
            _ if hc_lit => c.on_accent,
            _ if !r.enabled && c.high_contrast => c.muted,
            _ if !r.enabled => theme::with_alpha(c.muted, if c.light { 190 } else { 140 }),
            _ if r.danger => c.danger_text,
            _ => c.text,
        };
        let icon_fg = match () {
            _ if hc_lit || !r.enabled || r.danger => fg,
            _ if hot || c.high_contrast => c.text,
            _ => c.muted,
        };
        if level.icons {
            let at = pos2(rect.left() + SIDE + ICON_W / 2.0, cy);
            if r.checked && (r.icon.is_some() || r.image.is_some()) {
                let well = Rect::from_center_size(at, Vec2::splat(26.0));
                if c.high_contrast {
                    // An outline: highlight would sit under ordinary text.
                    painter.rect_stroke(well, CornerRadius::same(7), Stroke::new(1.5_f32, fg), egui::StrokeKind::Inside);
                } else {
                    painter.rect_filled(well, CornerRadius::same(7), c.row_selected);
                }
            }
            match (r.image, r.icon, r.checked) {
                (Some(tex), _, _) => image(&painter, tex, Rect::from_center_size(at, Vec2::splat(IMAGE)), r.enabled),
                (None, Some(g), _) => {
                    painter.text(at, Align2::CENTER_CENTER, g, icon_font(15.0), icon_fg);
                }
                (None, None, true) => {
                    painter.text(at, Align2::CENTER_CENTER, glyph::CHECK, icon_font(13.0), fg);
                }
                (None, None, false) => {}
            }
        }
        let mut right_w = 0.0;
        let right_x = rect.right() - SIDE;
        if let RowKind::Submenu(_) = r.kind {
            let col = if hc_lit { c.on_accent } else if open || hot { c.text } else { c.muted };
            painter.text(pos2(right_x, cy), Align2::RIGHT_CENTER, glyph::CHEVRON, icon_font(10.0), col);
            right_w = CHEVRON_W + GAP;
        } else if !r.hint.is_empty() {
            // A long hint (a folder's path) gets at most half the row, cut with an ellipsis.
            let room = (rect.width() - label_x - SIDE) * 0.5;
            let col = if hc_lit { c.on_accent } else { c.muted };
            right_w = label(&painter, pos2(right_x, cy), room, &r.hint, HINT_SIZE, col, Align2::RIGHT_CENTER) + GAP * 2.0;
        }
        let room = rect.width() - label_x - SIDE - right_w;
        label(&painter, pos2(rect.left() + label_x, cy), room, &r.label, LABEL_SIZE, fg, Align2::LEFT_CENTER);
    }
    // A level that scrolls: a thin bar in the right padding, as long as the share shown.
    let max = level.max_scroll();
    if max > 0.0 {
        let track = Rect::from_min_max(
            pos2(origin.x + width - PAD / 2.0 - BAR_W / 2.0, origin.y + RADIUS / 2.0),
            pos2(origin.x + width - PAD / 2.0 + BAR_W / 2.0, origin.y + level.size.y - RADIUS / 2.0),
        );
        let share = (level.size.y / level.content).clamp(0.1, 1.0);
        let h = track.height() * share;
        let top = track.top() + (track.height() - h) * (scroll / max).clamp(0.0, 1.0);
        let bar = Rect::from_min_size(pos2(track.left(), top), vec2(BAR_W, h));
        let col = if c.light { Color32::from_black_alpha(60) } else { Color32::from_white_alpha(70) };
        painter.rect_filled(bar, CornerRadius::same(2), col);
    }
}

/// A picture, faded when its row cannot be picked.
fn image(painter: &Painter, tex: TextureId, at: Rect, enabled: bool) {
    let tint = if enabled { Color32::WHITE } else { Color32::from_white_alpha(110) };
    let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
    painter.image(tex, at, uv, tint);
}

/// One line of text, cut with an ellipsis past `room`.
/// Returns the width it took.
fn label(painter: &Painter, at: Pos2, room: f32, text: &str, size: f32, color: Color32, align: Align2) -> f32 {
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat { font_id: FontId::proportional(size), color, ..Default::default() },
    );
    job.wrap = TextWrapping::truncate_at_width(room.max(10.0));
    let galley = painter.ctx().fonts(|f| f.layout_job(job));
    let pos = align.anchor_size(at, galley.size()).min;
    let w = galley.size().x;
    painter.galley(pos, galley, color);
    w
}

#[cfg(test)]
mod tests {
    use super::super::model::{flatten, Entry};
    use super::*;

    #[test]
    fn rows_say_what_they_are() {
        let rows = flatten(vec![
            Entry::item(0, "&Open").hint("Enter"),
            Entry::separator(),
            Entry::heading("Recent"),
            Entry::item(1, "Pinned").checked(true),
            Entry::submenu("Sort by", vec![Entry::item(2, "Name")]),
        ], &mut Vec::new(), 0);
        let said: Vec<_> = rows.iter().map(spoken).collect();
        assert_eq!(said[0], Some((egui::WidgetType::Button, None, "Open, Enter".to_owned())));
        assert_eq!(said[1], None);
        assert_eq!(said[2], Some((egui::WidgetType::Label, None, "Recent".to_owned())));
        assert_eq!(said[3], Some((egui::WidgetType::Checkbox, Some(true), "Pinned".to_owned())));
        assert_eq!(said[4], Some((egui::WidgetType::Button, None, "Sort by, submenu".to_owned())));
    }

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
        let l = measure(&rows, w, f32::INFINITY);
        assert_eq!(l.rows[0], (PAD, ROW_H));
        assert_eq!(l.rows[1].0, PAD + ROW_H);
        assert_eq!(l.rows[3].0, PAD + ROW_H + SEPARATOR_H + HEADING_H);
        assert_eq!(l.size.y, PAD * 2.0 + ROW_H * 2.0 + SEPARATOR_H + HEADING_H);
        assert_eq!(l.size.x, MIN_W);
        assert!(!l.icons);
        let long = flatten(vec![Entry::item(0, "x".repeat(200)).icon('a')], &mut Vec::new(), 0);
        let l = measure(&long, w, f32::INFINITY);
        assert_eq!(l.size.x, MAX_W);
        assert!(l.icons);
        // Even an empty menu is a sane size.
        let l = measure(&[], w, f32::INFINITY);
        assert!(l.size.y >= ROW_H);
    }

    #[test]
    fn hints_make_room() {
        let plain = flatten(vec![Entry::item(0, "x".repeat(30))], &mut Vec::new(), 0);
        let hinted = flatten(vec![Entry::item(0, "x".repeat(30)).hint("Ctrl Shift V")], &mut Vec::new(), 0);
        assert!(measure(&hinted, w, f32::INFINITY).size.x > measure(&plain, w, f32::INFINITY).size.x);
    }

    #[test]
    fn hit_testing_finds_rows_and_padding() {
        let rows = flatten(vec![Entry::item(0, "a"), Entry::separator(), Entry::item(1, "b")], &mut Vec::new(), 0);
        let l = measure(&rows, w, f32::INFINITY);
        let o = pos2(100.0, 50.0);
        assert_eq!(row_at(&l, o, 0.0, pos2(150.0, 50.0 + PAD + 5.0)), Some(Some(0)));
        assert_eq!(row_at(&l, o, 0.0, pos2(150.0, 50.0 + PAD + ROW_H + 3.0)), Some(Some(1)));
        assert_eq!(row_at(&l, o, 0.0, pos2(150.0, 50.0 + 2.0)), Some(None));
        assert_eq!(row_at(&l, o, 0.0, pos2(99.0, 60.0)), None);
    }

    #[test]
    fn a_level_taller_than_the_monitor_scrolls() {
        let entries: Vec<Entry<usize>> = (0..60).map(|i| Entry::item(i, format!("App {i}"))).collect();
        let rows = flatten(entries, &mut Vec::new(), 0);
        let l = measure(&rows, w, 500.0);
        assert_eq!(l.size.y, 500.0);
        assert_eq!(l.content, 2.0 * PAD + 60.0 * ROW_H);
        assert_eq!(l.max_scroll(), l.content - 500.0);
        // Scrolled by two rows, the first row under the top is row 2.
        let o = pos2(0.0, 0.0);
        assert_eq!(row_at(&l, o, 2.0 * ROW_H, pos2(50.0, PAD + 1.0)), Some(Some(2)));
        // Revealing the last row scrolls to the end; the first, back to the top.
        assert_eq!(l.reveal(59, 0.0), l.max_scroll());
        assert_eq!(l.reveal(0, 300.0), 0.0);
        // A row already in view leaves the scroll alone.
        assert_eq!(l.reveal(5, 2.0 * ROW_H), 2.0 * ROW_H);
        // The padding at the bottom of a scrolled level is no row.
        assert_eq!(row_at(&l, o, 0.0, pos2(50.0, 500.0 - 2.0)), Some(None));
    }

    #[test]
    fn notes_pictures_and_images() {
        let tex = TextureId::Managed(7);
        let rows = flatten(
            vec![Entry::note("Loading…"), Entry::picture(0, tex, vec2(300.0, 50.0)), Entry::item(1, "Open").image(tex)],
            &mut Vec::new(),
            0,
        );
        let l = measure(&rows, w, f32::INFINITY);
        assert!(l.icons);
        assert_eq!(l.rows[0].1, NOTE_H);
        assert_eq!(l.rows[1].1, 56.0);
        assert!(l.size.x >= 300.0 + 2.0 * SIDE);
    }
}
