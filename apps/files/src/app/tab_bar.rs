//! The tabs, on a pill of their own at the top, as wide as they need.

use super::actions::Action;
use super::tab::Loc;
use egui::{Align2, CornerRadius, FontId, Id, Rect, Sense, Ui, Vec2};
use shellman_ui::chips::{round_icon, soft_colors};
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::{draw_text, menu_item};

/// Inside the pill, around the tabs.
const PAD: f32 = 5.0;
const TAB_GAP: f32 = 4.0;
const MIN_TAB_W: f32 = 104.0;
const MAX_TAB_W: f32 = 200.0;

/// How wide each of `n` tabs is in a row `room` wide, and how wide the pill is then.
pub(super) fn fit(n: usize, room: f32, height: f32) -> (f32, f32) {
    let plus = height - PAD * 2.0;
    let avail = room - PAD * 2.0 - plus - TAB_GAP;
    let tab_w = (avail / n as f32 - TAB_GAP).clamp(MIN_TAB_W, MAX_TAB_W);
    let width = PAD * 2.0 + n as f32 * (tab_w + TAB_GAP) + plus;
    (tab_w, width)
}

impl super::FileMan {
    /// Draw the tabs at the left of `row`; returns the pill they are on.
    pub(super) fn tab_strip(&mut self, ui: &mut Ui, row: Rect, actions: &mut Vec<Action>) -> Rect {
        let c = colors();
        let n = self.tabs.len();
        let (tab_w, wanted) = fit(n, row.width(), row.height());
        let pill = super::plan::from_left(row, wanted);
        let h = row.height() - PAD * 2.0;
        // Tabs that do not fit are clipped; the active one stays reachable from the keyboard.
        let clip = ui.clip_rect().intersect(pill.shrink(PAD));
        let mut x = pill.left() + PAD;
        for i in 0..n {
            let rect = Rect::from_min_size(egui::pos2(x, pill.top() + PAD), Vec2::new(tab_w, h));
            x += tab_w + TAB_GAP;
            if rect.left() > clip.right() {
                break;
            }
            let active = i == self.active;
            let loc = self.tabs[i].loc.clone();
            let tab_id = self.tabs[i].id;
            let resp = ui.interact(
                rect.intersect(clip),
                Id::new(("tab", tab_id)),
                Sense::click(),
            );
            let hovered = resp.hovered();
            let r = CornerRadius::same((h / 2.0) as u8);
            let (soft, soft_hover, line) = soft_colors(c.light);
            let p = ui.painter().with_clip_rect(clip);
            if active {
                p.rect(rect, r, soft_hover, line, egui::StrokeKind::Inside);
            } else if hovered {
                p.rect_filled(rect, r, soft);
            }
            let g = match loc {
                Loc::Pc => glyph::PC,
                Loc::Bin => glyph::DELETE,
                Loc::Dir(_) => glyph::FOLDER,
            };
            p.text(
                egui::pos2(rect.left() + 16.0, rect.center().y),
                Align2::CENTER_CENTER,
                g,
                icon_font(12.0),
                if active { c.accent_text } else { c.muted },
            );
            let show_close = n > 1 && (active || hovered);
            let text_right = rect.right() - if show_close { 28.0 } else { 12.0 };
            let text = Rect::from_min_max(
                egui::pos2(rect.left() + 30.0, rect.top()),
                egui::pos2(text_right.min(clip.right()), rect.bottom()),
            );
            draw_text(
                ui,
                text,
                &loc.title(),
                FontId::proportional(13.0),
                if active { c.text } else { c.muted },
                false,
            );
            let mut closed = false;
            if show_close {
                let cr = Rect::from_center_size(
                    egui::pos2(rect.right() - 15.0, rect.center().y),
                    Vec2::splat(20.0),
                );
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
                if menu_item(ui, glyph::ADD, "Duplicate tab", "") {
                    actions.push(Action::NewTab(loc.clone()));
                    ui.close_menu();
                }
                if n > 1 && menu_item(ui, glyph::CLOSE, "Close tab", "Ctrl+W") {
                    actions.push(Action::CloseTab(i));
                    ui.close_menu();
                }
            });
        }
        let plus = Rect::from_min_size(
            egui::pos2(x.min(pill.right() - PAD - h), pill.top() + PAD),
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
        let (w, pill) = fit(1, 600.0, 40.0);
        assert_eq!(w, MAX_TAB_W);
        assert!(pill < 300.0);
        // Many: narrower, down to the smallest, and the pill fills the row.
        let (w, pill) = fit(5, 600.0, 40.0);
        assert!((MIN_TAB_W..MAX_TAB_W).contains(&w));
        assert!(pill <= 600.0 + 0.01);
        assert_eq!(fit(40, 600.0, 40.0).0, MIN_TAB_W);
    }
}
