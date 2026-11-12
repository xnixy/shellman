//! The window's layout: one even tint over the glass, the sidebar, the tab strip and toolbar,
//! and the sheet that holds the folder and its footer.

use super::actions::Action;
use super::tab::Loc;
use super::{CAPTION_H, FOOTER_H, SIDEBAR_W};
use crate::settings::ViewMode;
use crate::theme::colors;
use egui::{Color32, Align, CornerRadius, Layout, Stroke, Ui, Vec2};

impl super::FileMan {
    pub(super) fn layout(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let ppp = ctx.pixels_per_point();
        self.caption = if self.glass {
            crate::window::caption_buttons_size(self.hwnd)
                .map(|(w, h)| Vec2::new(w as f32 / ppp, h as f32 / ppp))
        } else {
            None
        };
        // Exactly the caption buttons' height, so the tabs and the buttons share one centre line.
        let caption_h = self.caption.map_or(CAPTION_H, |s| s.y.clamp(32.0, 48.0));
        let c = colors();
        // One even tint over the whole window; nothing else paints a band of its own.
        ctx.layer_painter(egui::LayerId::background()).rect_filled(
            ctx.screen_rect(),
            CornerRadius::ZERO,
            if self.glass { c.shell } else { c.bg },
        );
        egui::SidePanel::left("sidebar")
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                left: 10,
                right: 8,
                top: 0,
                bottom: 10,
            }))
            .exact_width(SIDEBAR_W)
            .resizable(false)
            .show_separator_line(false)
            .show(ctx, |ui| self.sidebar(ui, caption_h, actions));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                left: 0,
                right: 10,
                top: 0,
                bottom: 10,
            }))
            .show(ctx, |ui| {
                egui::TopBottomPanel::top("tabs")
                    .frame(egui::Frame::new())
                    .show_separator_line(false)
                    .exact_height(caption_h)
                    .show_inside(ui, |ui| self.tab_strip(ui, actions));
                egui::TopBottomPanel::top("toolbar")
                    .frame(egui::Frame::new().inner_margin(egui::Margin {
                        left: 0,
                        right: 0,
                        top: 8,
                        bottom: 10,
                    }))
                    .show_separator_line(false)
                    .show_inside(ui, |ui| self.toolbar(ui, actions));
                egui::Frame::new()
                    .fill(c.sheet)
                    .stroke(Stroke::new(1.0_f32, c.hairline))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(egui::Margin {
                        left: 8,
                        right: 8,
                        top: 6,
                        bottom: 0,
                    })
                    // Over glass the sheet is see-through, and a shadow under it would darken
                    // its whole inside; only an opaque sheet gets one.
                    .shadow(egui::Shadow {
                        offset: [0, 2],
                        blur: 16,
                        spread: 0,
                        color: if self.glass { Color32::TRANSPARENT } else { c.shadow },
                    })
                    .show(ui, |ui| {
                        let size = ui.available_size();
                        ui.set_min_size(size);
                        let body = Vec2::new(size.x, (size.y - FOOTER_H).max(0.0));
                        ui.allocate_ui_with_layout(body, Layout::top_down(Align::Min), |ui| {
                            ui.set_min_size(body);
                            self.folder_body(ui, actions);
                        });
                        self.footer(ui);
                    });
            });
    }

    /// This PC, or the folder in the chosen view.
    fn folder_body(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        if self.tabs[self.active].loc == Loc::Pc {
            self.pc_page(ui, actions);
        } else if self.settings.view == ViewMode::Grid {
            self.file_grid(ui, actions);
        } else {
            self.file_list(ui, actions);
        }
    }
}
