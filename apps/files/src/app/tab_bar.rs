//! The tabs, on a pill of their own at the top, as wide as they need. Many tabs shrink down to
//! a smallest width; past that they scroll sideways with the wheel, and the active tab is
//! brought into view whenever it changes (see `strip`).

use super::actions::Action;
use super::tab::Loc;
use egui::{Align2, CornerRadius, FontId, Id, Rect, Sense, Ui, Vec2};
use shellman_ui::chips::{round_icon, soft_colors};
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::draw_text;

/// Inside the pill, around the tabs.
const PAD: f32 = 5.0;
const TAB_GAP: f32 = 4.0;
const MIN_TAB_W: f32 = 112.0;
const MAX_TAB_W: f32 = 200.0;

/// How the tabs lie in their pill this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Strip {
    pub(super) tab_w: f32,
    /// The pill's whole width.
    pub(super) pill_w: f32,
    /// The width the tabs are seen through (the pill less its padding and the + button).
    pub(super) view_w: f32,
    /// How far the tabs are scrolled, and how far they can be.
    pub(super) offset: f32,
    pub(super) max_offset: f32,
}

/// The strip for `n` tabs in a row `room` wide and `height` tall, scrolled by `scroll` (kept
/// within what can scroll).
pub(super) fn strip(n: usize, room: f32, height: f32, scroll: f32) -> Strip {
    let n = n.max(1);
    let plus = height - PAD * 2.0;
    let chrome = PAD * 2.0 + plus + TAB_GAP;
    let avail = (room - chrome).max(0.0);
    let tab_w = (avail / n as f32 - TAB_GAP).clamp(MIN_TAB_W, MAX_TAB_W);
    let total = n as f32 * (tab_w + TAB_GAP) - TAB_GAP;
    let view_w = total.min(avail);
    let max_offset = (total - view_w).max(0.0);
    Strip {
        tab_w,
        pill_w: view_w + chrome,
        view_w,
        offset: scroll.clamp(0.0, max_offset),
        max_offset,
    }
}

/// The scroll that shows tab `active` whole, moving as little as it can from `s.offset`.
pub(super) fn reveal(s: &Strip, active: usize) -> f32 {
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

impl super::FileMan {
    /// Draw the tabs at the left of `row`; returns the pill they are on.
    pub(super) fn tab_strip(&mut self, ui: &mut Ui, row: Rect, actions: &mut Vec<Action>) -> Rect {
        let c = colors();
        let n = self.tabs.len();
        let mut s = strip(n, row.width(), row.height(), self.tab_scroll);
        if self.tab_seen != (self.active, n) {
            self.tab_seen = (self.active, n);
            s.offset = reveal(&s, self.active);
        }
        let pill = super::plan::from_left(row, s.pill_w);
        let h = row.height() - PAD * 2.0;
        let view = Rect::from_min_size(
            egui::pos2(pill.left() + PAD, pill.top() + PAD),
            Vec2::new(s.view_w, h),
        );
        // The wheel scrolls the tabs sideways while the pointer is over them.
        if s.max_offset > 0.0 && ui.rect_contains_pointer(pill) {
            let d = ui.input(|i| i.smooth_scroll_delta);
            let step = if d.x.abs() > d.y.abs() { d.x } else { d.y };
            if step != 0.0 {
                s.offset = (s.offset - step).clamp(0.0, s.max_offset);
            }
        }
        self.tab_scroll = s.offset;
        let clip = ui.clip_rect().intersect(view);
        let mut x = view.left() - s.offset;
        for i in 0..n {
            let rect = Rect::from_min_size(egui::pos2(x, view.top()), Vec2::new(s.tab_w, h));
            x += s.tab_w + TAB_GAP;
            if rect.right() < clip.left() || rect.left() > clip.right() {
                continue;
            }
            let active = i == self.active;
            let loc = self.tabs[i].loc.clone();
            let tab_id = self.tabs[i].id;
            let resp = ui.interact(
                rect.intersect(clip),
                Id::new(("tab", tab_id)),
                Sense::click(),
            );
            super::dnd::zone(ui, rect.intersect(clip), super::dnd::Dest::Tab(i), h / 2.0);
            let hovered = resp.hovered();
            let r = CornerRadius::same((h / 2.0) as u8);
            let (soft, soft_hover, line) = soft_colors(c.light);
            let p = ui.painter().with_clip_rect(clip);
            if active {
                p.rect(rect, r, soft_hover, line, egui::StrokeKind::Inside);
            } else if hovered {
                p.rect_filled(rect, r, soft);
            }
            let icon_at = egui::pos2(rect.left() + 16.0, rect.center().y);
            match loc {
                Loc::Tag(t) => {
                    p.circle_filled(icon_at, 5.0, super::tag_ui::colour(t as usize));
                }
                _ => {
                    let g = match loc {
                        Loc::Pc => glyph::PC,
                        Loc::Bin => glyph::DELETE,
                        _ => glyph::FOLDER,
                    };
                    let _ = p.text(
                        icon_at,
                        Align2::CENTER_CENTER,
                        g,
                        icon_font(12.0),
                        if active { c.accent_text } else { c.muted },
                    );
                }
            }
            let show_close = n > 1 && (active || hovered);
            let text_right = rect.right() - if show_close { 28.0 } else { 12.0 };
            let text = Rect::from_min_max(
                egui::pos2(rect.left() + 30.0, rect.top()),
                egui::pos2(text_right, rect.bottom()),
            )
            .intersect(clip);
            draw_text(
                ui,
                text,
                &loc.title(),
                FontId::proportional(13.0),
                if active { c.text } else { c.muted },
                false,
            );
            let mut closed = false;
            let cr = Rect::from_center_size(
                egui::pos2(rect.right() - 15.0, rect.center().y),
                Vec2::splat(20.0),
            );
            if show_close && clip.contains_rect(cr) {
                let close = ui.interact(cr, Id::new(("tab-close", tab_id)), Sense::click());
                if close.hovered() {
                    p.circle_filled(cr.center(), 10.0, theme::with_alpha(c.text, 30));
                }
                p.text(
                    cr.center(),
                    Align2::CENTER_CENTER,
                    glyph::CLOSE,
                    icon_font(8.0),
                    c.muted,
                );
                if close.clicked() {
                    actions.push(Action::CloseTab(i));
                    closed = true;
                }
            }
            if !closed && resp.clicked() {
                actions.push(Action::SwitchTab(i));
            }
            if resp.middle_clicked() {
                actions.push(Action::CloseTab(i));
            }
            let resp = resp.on_hover_text(
                loc.dir()
                    .map_or_else(|| loc.title(), |d| d.display().to_string()),
            );
            resp.context_menu(|ui| {
                super::controls::flat_items(ui);
                if super::controls::mi(ui, glyph::ADD, "Duplicate tab", "") {
                    actions.push(Action::NewTab(loc.clone()));
                    ui.close_menu();
                }
                if n > 1 && super::controls::mi(ui, glyph::CLOSE, "Close tab", "Ctrl+W") {
                    actions.push(Action::CloseTab(i));
                    ui.close_menu();
                }
            });
        }
        // Where tabs are cut off, a short line marks the edge, so it is plain there are more.
        for (show, x) in [(s.offset > 0.5, view.left()), (s.offset < s.max_offset - 0.5, view.right())] {
            if show {
                ui.painter().vline(x, view.y_range().shrink(6.0), egui::Stroke::new(1.0_f32, theme::with_alpha(c.text, 60)));
            }
        }
        let plus = Rect::from_min_size(
            egui::pos2(view.right() + TAB_GAP, pill.top() + PAD),
            Vec2::splat(h),
        );
        if round_icon(ui, plus, Id::new("new-tab"), glyph::ADD, true, false)
            .on_hover_text("New tab (Ctrl+T)")
            .clicked()
        {
            actions.push(Action::NewTab(self.tabs[self.active].loc.clone()));
        }
        pill
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_share_the_row_and_the_pill_hugs_them() {
        // One tab: as wide as a tab may be, and the pill no wider than it needs.
        let s = strip(1, 600.0, 40.0, 0.0);
        assert_eq!(s.tab_w, MAX_TAB_W);
        assert!(s.pill_w < 300.0);
        assert_eq!(s.max_offset, 0.0);
        // Several: narrower, down to the smallest, and the pill fits the row.
        let s = strip(4, 600.0, 40.0, 0.0);
        assert!((MIN_TAB_W..MAX_TAB_W).contains(&s.tab_w));
        assert!(s.pill_w <= 600.0 + 0.01);
        assert_eq!(s.max_offset, 0.0);
    }

    #[test]
    fn past_the_smallest_width_they_scroll_and_never_overflow() {
        let s = strip(40, 600.0, 40.0, 0.0);
        assert_eq!(s.tab_w, MIN_TAB_W);
        assert!(s.pill_w <= 600.0 + 0.01, "the pill stays in its row");
        assert!(s.max_offset > 0.0);
        // A scroll past either end is held at it.
        assert_eq!(strip(40, 600.0, 40.0, -50.0).offset, 0.0);
        assert_eq!(strip(40, 600.0, 40.0, 1e6).offset, s.max_offset);
    }

    #[test]
    fn the_active_tab_is_brought_into_view() {
        let s = strip(40, 600.0, 40.0, 0.0);
        let step = s.tab_w + TAB_GAP;
        // The last tab: scrolled to the end.
        let o = reveal(&s, 39);
        assert!((o - s.max_offset).abs() < 0.01);
        let at = Strip { offset: o, ..s };
        // One already in view: no move.
        assert_eq!(reveal(&at, 38), o);
        // The first: back to the start.
        assert_eq!(reveal(&at, 0), 0.0);
        // A tab right of the view: its right edge lands on the view's.
        let o = reveal(&s, 10);
        assert!((o + s.view_w - (10.0 * step + s.tab_w)).abs() < 0.01);
    }
}
