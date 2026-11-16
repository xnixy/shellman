//! The window's layout: islands of glass placed by `plan`, each drawn in its own part of the
//! window, and handed to the glass panel every frame (free while nothing moves). The rest of the
//! window is gap: the desktop shows through, and it moves the window.

use super::actions::Action;
use super::plan::{Plan, GAP};
use super::tab::Loc;
use super::RADIUS;
use crate::settings::ViewMode;
use egui::{Align, Layout, Rect, Shape, Ui, UiBuilder, Vec2};
use shellman_ui::glass::{islands, Island, Style};

/// How much of the glass behind the cards shows (see `layout`). Over a bright, busy wallpaper,
/// cards straight on the desktop could not be read, and glass only partly there (0.55 was
/// tried) lets the sharp picture back through it, which reads no better. So it is whole glass,
/// but without a shadow, so the cards still look like the pieces that float.
const BACKING: f32 = 1.0;

impl super::FileMan {
    pub(super) fn layout(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let screen = ctx.screen_rect();
        let rows = self.rail_rows(ctx);
        let mode = self.rail_mode(screen.width());
        // Read every frame, so the glass follows the theme, accent and glass amount live.
        let style = Style::from_theme(RADIUS);
        let composed = self.panel.as_ref().is_some_and(|p| p.composed());
        if let Some(panel) = &self.panel {
            panel.set_style(style);
        }
        let mut list: Vec<Island> = Vec::with_capacity(10);
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ctx, |ui| {
                // Under everything: the islands themselves when nothing composes them.
                let under = ui.painter().add(Shape::Noop);
                islands::drag_gaps(ui, screen);
                let rail_w = shellman_ui::rail::width(ui, &rows.entries, mode);
                let plan = Plan::new(screen, self.details_on(), rail_w);

                let top = if self.pick.is_some() {
                    self.pick_title(ui, plan.tabs_row, actions)
                } else {
                    self.tab_strip(ui, plan.tabs_row, actions)
                };
                list.push(Island::pill(top));
                self.window_controls(ui, plan.controls);
                list.push(Island::pill(plan.controls));
                self.toolbar(ui, plan.bar, actions);
                list.push(Island::pill(plan.bar));
                let solid = (!composed).then_some(&style);
                self.rail(ui, &mut list, screen, plan.rail_area.top(), &rows, mode, solid, actions);

                // A list sits on one island. Cards are their own surfaces, over shadowless glass
                // that keeps them readable on any wallpaper.
                self.content_island = plan.content;
                let listed = self.listed();
                let content = if listed {
                    list.push(Island::new(plan.content, RADIUS));
                    plan.content.shrink2(Vec2::new(10.0, 8.0))
                } else {
                    list.push(Island::new(plan.content, RADIUS).faint(BACKING));
                    plan.content.shrink(10.0)
                };
                let mut child = ui.new_child(
                    UiBuilder::new()
                        .id_salt("folder")
                        .max_rect(content)
                        .layout(Layout::top_down(Align::Min)),
                );
                child.set_clip_rect(content);
                self.folder_body(&mut child, actions);

                if let Some(d) = plan.details {
                    list.push(Island::new(d, RADIUS));
                    self.details_island(ui, d, actions);
                }

                let mut extra = Vec::new();
                // The free end of the top row, right of the tabs, for messages.
                let top_free = Rect::from_min_max(
                    egui::pos2(top.right() + GAP, plan.tabs_row.top()),
                    plan.tabs_row.max,
                );
                let pill = if self.pick.is_some() {
                    let pill = self.pick_band(ui, plan.band);
                    // Messages go up beside the title, the bottom being the picker's.
                    let whole = pill.unwrap_or(plan.band);
                    self.toast_pill(ui, plan.band, whole, top_free, &mut extra);
                    pill
                } else {
                    let pill = self.band(ui, plan.band, actions);
                    self.toast_pill(ui, plan.band, pill.unwrap_or(Rect::NOTHING), top_free, &mut extra);
                    pill
                };
                if let Some(pill) = pill {
                    list.push(Island::pill(pill));
                }
                list.extend(extra.into_iter().map(Island::pill));
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

    /// Whether the folder shows as rows (on an island) rather than cards.
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
