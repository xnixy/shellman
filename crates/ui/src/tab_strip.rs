//! A row of tabs on a pill of its own, as Files draws them: as wide as they need, shrinking down
//! to a smallest width and past that scrolling sideways, round buttons after them (new tab).
//!
//! The geometry is pure ([`strip`], [`reveal`], [`index_at`]) and the painting of one tab is
//! [`tab`], so an app keeps its own tab list, menus and drag rules and only shares the look.
//! (Files still has its own copy of this, from before it was lifted here; it can switch.)

use crate::chips::soft_colors;
use crate::theme::{self, colors, icon_font};
use crate::widgets::draw_text;
use egui::{Align2, Color32, CornerRadius, FontId, Id, Rect, Sense, Ui, Vec2};

/// Inside the pill, around the tabs.
pub const PAD: f32 = 5.0;
pub const TAB_GAP: f32 = 4.0;
pub const MIN_TAB_W: f32 = 112.0;
pub const MAX_TAB_W: f32 = 200.0;

/// How the tabs lie in their pill this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strip {
    pub tab_w: f32,
    /// The pill's whole width.
    pub pill_w: f32,
    /// The width the tabs are seen through (the pill less its padding and the buttons).
    pub view_w: f32,
    /// How far the tabs are scrolled, and how far they can be.
    pub offset: f32,
    pub max_offset: f32,
}

/// The strip for `n` tabs and `buttons` round buttons after them, in a row `room` wide and
/// `height` tall, scrolled by `scroll` (kept within what can scroll).
pub fn strip(n: usize, buttons: usize, room: f32, height: f32, scroll: f32) -> Strip {
    let n = n.max(1);
    let button = height - PAD * 2.0;
    let chrome = PAD * 2.0 + buttons as f32 * (button + TAB_GAP);
    let avail = (room - chrome).max(0.0);
    let tab_w = (avail / n as f32 - TAB_GAP).clamp(MIN_TAB_W, MAX_TAB_W);
    let total = n as f32 * (tab_w + TAB_GAP) - TAB_GAP;
    let view_w = total.min(avail);
    let max_offset = (total - view_w).max(0.0);
    Strip { tab_w, pill_w: view_w + chrome, view_w, offset: scroll.clamp(0.0, max_offset), max_offset }
}

/// The scroll that shows tab `active` whole, moving as little as it can from `s.offset`.
pub fn reveal(s: &Strip, active: usize) -> f32 {
    let left = active as f32 * (s.tab_w + TAB_GAP);
    let right = left + s.tab_w;
    let offset = if left < s.offset {
        left
    } else if right > s.offset + s.view_w {
        right - s.view_w
    } else {
        s.offset
    };
    offset.clamp(0.0, s.max_offset)
}

/// The tab a point `x` from the view's left edge falls on, of `n`: for dragging a tab to a new
/// place. Points past either end give the first or last.
pub fn index_at(s: &Strip, n: usize, x: f32) -> usize {
    let at = ((x + s.offset) / (s.tab_w + TAB_GAP)).floor();
    (at.max(0.0) as usize).min(n.saturating_sub(1))
}

/// The rectangle of tab `i` for a strip whose view starts at `view`.
pub fn tab_rect(s: &Strip, view: Rect, i: usize) -> Rect {
    let x = view.left() - s.offset + i as f32 * (s.tab_w + TAB_GAP);
    Rect::from_min_size(egui::pos2(x, view.top()), Vec2::new(s.tab_w, view.height()))
}

/// The view the tabs are seen through, in a pill at `pill`.
pub fn view(s: &Strip, pill: Rect) -> Rect {
    Rect::from_min_size(egui::pos2(pill.left() + PAD, pill.top() + PAD), Vec2::new(s.view_w, pill.height() - PAD * 2.0))
}

/// The round button `k` after the tabs (0 is the first).
pub fn button_rect(s: &Strip, pill: Rect, k: usize) -> Rect {
    let h = pill.height() - PAD * 2.0;
    let x = pill.left() + PAD + s.view_w + TAB_GAP + k as f32 * (h + TAB_GAP);
    Rect::from_min_size(egui::pos2(x, pill.top() + PAD), Vec2::splat(h))
}

/// Something a tab says about what happened in it while it was not looked at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    None,
    /// New output: a small quiet dot.
    Activity,
    /// A bell rang.
    Bell,
}

/// One tab to draw.
pub struct Tab<'a> {
    pub id: Id,
    pub title: &'a str,
    /// A glyph before the title, or a coloured dot.
    pub icon: Option<char>,
    pub active: bool,
    /// The × shows (on the active tab and on hover).
    pub closable: bool,
    pub mark: Mark,
}

/// What a tab was asked to do this frame.
pub struct TabResponse {
    /// The tab itself: clicks (switch), middle clicks (close), drags (move).
    pub resp: egui::Response,
    pub close: bool,
}

/// The bell glyph (Segoe Fluent's ringer).
const BELL: char = '\u{EA8F}';

/// Draw `t` in `rect`, clipped to `clip` (the strip's view).
pub fn tab(ui: &mut Ui, rect: Rect, clip: Rect, t: &Tab) -> TabResponse {
    let c = colors();
    let h = rect.height();
    let resp = ui.interact(rect.intersect(clip), t.id, Sense::click_and_drag());
    // A screen reader reads it as a selectable item with its title, selected when shown; with
    // the keyboard on it, it has a ring.
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, t.active, t.title));
    let hovered = resp.hovered() || resp.dragged() || resp.has_focus();
    let r = CornerRadius::same((h / 2.0) as u8);
    let (soft, soft_hover, line) = soft_colors(c.light);
    let p = ui.painter().with_clip_rect(clip);
    if c.high_contrast {
        // Highlight only as an outline, never a fill under ordinary text.
        let stroke = if t.active { egui::Stroke::new(2.0_f32, c.accent_fill) } else { egui::Stroke::new(1.0_f32, c.text) };
        p.rect_stroke(rect, r, stroke, egui::StrokeKind::Inside);
    } else if t.active {
        p.rect(rect, r, soft_hover, line, egui::StrokeKind::Inside);
    } else if hovered {
        p.rect_filled(rect, r, soft);
    }
    theme::focus_ring(ui, &resp, rect, h / 2.0);
    let icon_at = egui::pos2(rect.left() + 16.0, rect.center().y);
    if let Some(g) = t.icon {
        p.text(icon_at, Align2::CENTER_CENTER, g, icon_font(12.0), if t.active { c.accent_text } else { c.muted });
    }
    let show_close = t.closable && (t.active || hovered);
    let text_right = rect.right() - if show_close || t.mark != Mark::None { 28.0 } else { 12.0 };
    let text = Rect::from_min_max(egui::pos2(rect.left() + 30.0, rect.top()), egui::pos2(text_right, rect.bottom())).intersect(clip);
    draw_text(ui, text, t.title, FontId::proportional(13.0), if t.active { c.text } else { c.muted }, false);
    let cr = Rect::from_center_size(egui::pos2(rect.right() - 15.0, rect.center().y), Vec2::splat(20.0));
    let mut close = false;
    if show_close && clip.contains_rect(cr) {
        let b = ui.interact(cr, t.id.with("close"), Sense::click());
        b.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Close {}", t.title)));
        theme::focus_ring(ui, &b, cr, 10.0);
        if b.hovered() {
            p.circle_filled(cr.center(), 10.0, theme::with_alpha(c.text, 30));
        }
        p.text(cr.center(), Align2::CENTER_CENTER, theme::glyph::CLOSE, icon_font(8.0), c.muted);
        close = b.clicked();
    } else {
        match t.mark {
            Mark::None => {}
            Mark::Activity => {
                p.circle_filled(cr.center(), 3.0, theme::with_alpha(c.muted, 200));
            }
            Mark::Bell => {
                p.text(cr.center(), Align2::CENTER_CENTER, BELL, icon_font(11.0), c.accent_text);
            }
        }
    }
    TabResponse { resp, close }
}

/// Short lines at the view's edges where tabs are cut off, so it is plain there are more.
pub fn edges(ui: &Ui, s: &Strip, view: Rect) {
    let c = colors();
    for (show, x) in [(s.offset > 0.5, view.left()), (s.offset < s.max_offset - 0.5, view.right())] {
        if show {
            ui.painter().vline(x, view.y_range().shrink(6.0), egui::Stroke::new(1.0_f32, theme::with_alpha(c.text, 60)));
        }
    }
}

/// A faint fill for something pressed and held (a tab being dragged).
pub fn lifted() -> Color32 {
    soft_colors(colors().light).1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_share_the_row_and_the_pill_hugs_them() {
        let s = strip(1, 1, 600.0, 40.0, 0.0);
        assert_eq!(s.tab_w, MAX_TAB_W);
        assert!(s.pill_w < 300.0);
        assert_eq!(s.max_offset, 0.0);
        let s = strip(4, 2, 600.0, 40.0, 0.0);
        assert!((MIN_TAB_W..MAX_TAB_W).contains(&s.tab_w));
        assert!(s.pill_w <= 600.0 + 0.01);
        assert_eq!(s.max_offset, 0.0);
    }

    #[test]
    fn past_the_smallest_width_they_scroll_and_never_overflow() {
        let s = strip(40, 2, 600.0, 40.0, 0.0);
        assert_eq!(s.tab_w, MIN_TAB_W);
        assert!(s.pill_w <= 600.0 + 0.01, "the pill stays in its row");
        assert!(s.max_offset > 0.0);
        assert_eq!(strip(40, 2, 600.0, 40.0, -50.0).offset, 0.0);
        assert_eq!(strip(40, 2, 600.0, 40.0, 1e6).offset, s.max_offset);
    }

    #[test]
    fn the_active_tab_is_brought_into_view() {
        let s = strip(40, 1, 600.0, 40.0, 0.0);
        let step = s.tab_w + TAB_GAP;
        let o = reveal(&s, 39);
        assert!((o - s.max_offset).abs() < 0.01);
        let at = Strip { offset: o, ..s };
        assert_eq!(reveal(&at, 38), o);
        assert_eq!(reveal(&at, 0), 0.0);
        let o = reveal(&s, 10);
        assert!((o + s.view_w - (10.0 * step + s.tab_w)).abs() < 0.01);
    }

    #[test]
    fn a_point_falls_on_the_tab_under_it() {
        let s = strip(3, 2, 900.0, 40.0, 0.0);
        let step = s.tab_w + TAB_GAP;
        assert_eq!(index_at(&s, 3, 1.0), 0);
        assert_eq!(index_at(&s, 3, step + 1.0), 1);
        assert_eq!(index_at(&s, 3, -40.0), 0);
        assert_eq!(index_at(&s, 3, 10_000.0), 2);
        let view = Rect::from_min_size(egui::pos2(10.0, 0.0), Vec2::new(s.view_w, 30.0));
        assert_eq!(tab_rect(&s, view, 1).left(), 10.0 + step);
    }

    #[test]
    fn buttons_follow_the_tabs() {
        let s = strip(2, 2, 900.0, 40.0, 0.0);
        let pill = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(s.pill_w, 40.0));
        let b1 = button_rect(&s, pill, 1);
        assert!((b1.right() + PAD - pill.right()).abs() < 0.01, "the last button ends at the pill's padding");
        assert!(button_rect(&s, pill, 0).left() > view(&s, pill).right());
    }
}
