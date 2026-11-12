//! The tab strip across the top, which is also the title bar.

use super::actions::Action;
use super::tab::Loc;
use egui::{Align, Align2, CornerRadius, FontId, Id, Layout, Rect, Sense, Ui, Vec2};
use shellman_ui::theme::{colors, glyph, icon_font};
use shellman_ui::widgets::{caption_button, draw_text, menu_item, round_button, window_drag};

impl super::FileMan {
    pub(super) fn tab_strip(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let c = colors();
        // Room for the window buttons: Windows' own over glass, ours otherwise.
        let reserve = self.caption.map_or(46.0 * 3.0, |s| s.x + 6.0);
        let height = ui.available_height();
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let n = self.tabs.len();
            let room = ui.available_width() - reserve - 48.0;
            let tab_w = (room / n as f32 - 4.0).clamp(84.0, 220.0);
            for i in 0..n {
                let active = i == self.active;
                let loc = self.tabs[i].loc.clone();
                let tab_id = self.tabs[i].id;
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(tab_w, 30.0), Sense::click());
                let hovered = ui.rect_contains_pointer(rect);
                if active {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(8), c.control_active);
                } else if hovered {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(8), c.control);
                }
                let g = match loc {
                    Loc::Pc => glyph::PC,
                    Loc::Bin => glyph::DELETE,
                    Loc::Dir(_) => glyph::FOLDER,
                };
                ui.painter().text(
                    egui::pos2(rect.left() + 14.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    g,
                    icon_font(13.0),
                    if active { c.accent_text } else { c.muted },
                );
                let show_close = n > 1 && (active || hovered);
                let text_right = rect.right() - if show_close { 30.0 } else { 10.0 };
                draw_text(
                    ui,
                    Rect::from_min_max(
                        egui::pos2(rect.left() + 36.0, rect.top()),
                        egui::pos2(text_right, rect.bottom()),
                    ),
                    &loc.title(),
                    FontId::proportional(13.0),
                    if active { c.text } else { c.muted },
                    false,
                );
                let mut closed = false;
                if show_close {
                    let cr = Rect::from_center_size(
                        egui::pos2(rect.right() - 16.0, rect.center().y),
                        Vec2::splat(20.0),
                    );
                    let close = ui.interact(cr, Id::new(("tab-close", tab_id)), Sense::click());
                    if close.hovered() {
                        ui.painter()
                            .rect_filled(cr, CornerRadius::same(5), c.control_hover);
                    }
                    ui.painter().text(
                        cr.center(),
                        Align2::CENTER_CENTER,
                        glyph::CLOSE,
                        icon_font(8.5),
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
            if round_button(ui, glyph::ADD, true, "New tab (Ctrl+T)").clicked() {
                actions.push(Action::NewTab(self.tabs[self.active].loc.clone()));
            }
            // The rest of the strip is title bar: it drags the window, and a double click
            // maximizes it.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if self.caption.is_some() {
                    ui.add_space(reserve);
                } else {
                    let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
                    if caption_button(ui, glyph::CHROME_CLOSE, true, height).clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    let g = if maximized {
                        glyph::RESTORE
                    } else {
                        glyph::MAXIMIZE
                    };
                    if caption_button(ui, g, false, height).clicked() {
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                    }
                    if caption_button(ui, glyph::MINIMIZE, false, height).clicked() {
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                }
                let (_, drag) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), height),
                    Sense::click_and_drag(),
                );
                window_drag(ui, &drag);
            });
        });
    }
}
