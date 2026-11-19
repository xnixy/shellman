//! The window's layout: one sheet of glass (`shellman_ui::sheet::Sheet`), its parts placed by
//! `plan`: the rail in the sidebar, the header bar, the row of tabs, the folder straight on the
//! pane with the details box at its right, and the bar along the bottom. The sheet is handed to
//! the glass panel every frame (free while nothing moves).

use super::actions::Action;
use super::plan::{Plan, SIDEBAR_MIN};
use super::tab::Loc;
use crate::settings::ViewMode;
use egui::{Align, Id, Layout, Rect, Sense, Shape, Stroke, Ui, UiBuilder, Vec2};
use shellman_ui::glass::{islands, Island, Style};
use shellman_ui::sheet::{self, Sheet};
use shellman_ui::theme::{colors, shape};
use shellman_ui::widgets::window_drag;

impl super::FileMan {
    pub(super) fn layout(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let screen = ctx.screen_rect();
        let rows = self.rail_rows(ctx);
        let mode = self.rail_mode(screen.width());
        // Read every frame, so the glass follows the theme, accent and glass amount live.
        let style = Style::from_theme(sheet::RADIUS);
        let composed = self.panel.as_ref().is_some_and(|p| p.composed());
        if let Some(panel) = &self.panel {
            panel.set_style(style);
        }
        let mut list: Vec<Island> = Vec::with_capacity(4);
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ctx, |ui| {
                // Under everything: the sheet itself when nothing composes it.
                let under = ui.painter().add(Shape::Noop);
                let rail_w = shellman_ui::rail::width(ui, &rows.entries, mode);
                let side_w = if mode.icons_only() { rail_w } else { rail_w.max(SIDEBAR_MIN) };
                let sheet = Sheet::new(ctx, screen, side_w);
                list.push(sheet.island());
                sheet.paint(ui);
                let plan = Plan::new(screen, self.details_on(), self.details_alone, side_w);
                self.narrow = plan.one_pane;
                if !plan.one_pane {
                    // Wide again: next time it narrows, the folder shows first.
                    self.details_alone = false;
                }
                self.take_focus = std::mem::take(&mut self.row_focus);

                let column = sheet.sidebar_head(ui, if mode.icons_only() { "" } else { "Files" });
                let solid = (!composed).then_some(&style);
                self.rail(ui, &mut list, column, screen, &rows, mode, solid, actions);
                self.toolbar(ui, &plan, sheet.radius, actions);

                // The tabs row's empty space moves the window, as the header's does. Made before
                // what is on it, so that wins.
                let drag = ui.interact(plan.tabs, Id::new("tabs-drag"), Sense::click_and_drag());
                window_drag(ui, &drag);
                let strip = if self.pick.is_some() {
                    self.pick_title(ui, plan.tabs_row, actions)
                } else {
                    self.tab_strip(ui, plan.tabs_row, actions)
                };

                // The folder, straight on the pane. Its ground is the whole of `content`, so a
                // click or a drop beside the items still lands in the folder.
                self.folder_rect = plan.content;
                // With the details in the folder's place (a narrow window), no folder this frame.
                if !plan.alone {
                    let mut child = ui.new_child(
                        UiBuilder::new()
                            .id_salt("folder")
                            .max_rect(plan.folder)
                            .layout(Layout::top_down(Align::Min)),
                    );
                    child.set_clip_rect(plan.content);
                    self.folder_body(&mut child, actions);
                }

                if let Some(d) = plan.details {
                    details_box(ui, d);
                    self.details_pane(ui, d, plan.alone, actions);
                }

                // The free end of the tabs row, right of the tabs, for messages.
                let top_free = Rect::from_min_max(egui::pos2(strip.right(), plan.tabs.top()), plan.tabs_row.max);
                if self.pick.is_some() {
                    self.pick_band(ui, plan.band);
                    // Messages go up beside the title, the bottom being the picker's.
                    self.toast_text(ui, Rect::from_min_size(plan.band.right_top(), Vec2::ZERO), top_free);
                } else {
                    let free = self.band(ui, plan.band, actions);
                    self.toast_text(ui, free, top_free);
                }
                // The rail's hover labels are glass too.
                list.extend(shellman_ui::hover::islands(ui.ctx()));
                // An open dialog floats on glass of its own.
                list.extend(super::dialogs::island(ui.ctx()));
                if !composed {
                    ui.painter()
                        .set(under, Shape::Vec(islands::under(screen, &list, &style)));
                }
            });
        if let Some(panel) = &self.panel {
            panel.set_islands(&list, ctx.pixels_per_point());
        }
    }

    /// Whether the folder shows as rows rather than cards.
    fn listed(&self) -> bool {
        let loc = &self.tabs[self.active].loc;
        *loc == Loc::Bin || (*loc != Loc::Pc && self.settings.view == ViewMode::List)
    }

    /// This PC, or the folder in the chosen view. The bin is always a list: its columns (original
    /// location, date deleted) have no place on a card.
    fn folder_body(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        if self.tabs[self.active].loc == Loc::Pc {
            self.pc_page(ui, actions);
        } else if self.listed() {
            self.file_list(ui, actions);
        } else {
            self.file_grid(ui, actions);
        }
    }
}

/// The details' box on the sheet: Glasshouse's card inside a hairline; in high contrast the
/// window colour inside a text-coloured outline.
fn details_box(ui: &Ui, rect: Rect) {
    let c = colors();
    let (fill, edge) = if c.high_contrast { (c.bg, c.text) } else { (c.card, c.hairline) };
    ui.painter().rect(rect, shape::corner(shape::ISLAND), fill, Stroke::new(1.0_f32, edge), egui::StrokeKind::Inside);
}
