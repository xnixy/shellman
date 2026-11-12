//! The window: islands of glass with the desktop showing between them. A pill rail of sections
//! at the left, a title pill on top, and the page below it as one island per group, scrolling
//! under the title pill.

use crate::instance::Instance;
use crate::page;
use crate::sections::Section;
use crate::state::State;
use egui::{pos2, vec2, Align2, Rect, Sense, UiBuilder};
use shellman_ui::chips::{self, Chip};
use shellman_ui::glass::islands::{self, Island};
use shellman_ui::glass::{GlassPanel, Style};
use shellman_ui::theme::{self, colors, icon_font, semibold_font, AccentChoice};
use std::time::Duration;

/// Space between the window's edge and the islands, and between islands.
const MARGIN: f32 = 12.0;
const RAIL_W: f32 = 56.0;
const RAIL_ICON: f32 = 34.0;
const TITLE_H: f32 = 46.0;
const BUTTON_H: f32 = 30.0;
/// The page's column is kept to a readable width, however wide the window.
const COLUMN_MAX: f32 = 760.0;
/// How long to wait before reading the files again if our own saves were still going.
const RELOAD_RETRY: Duration = Duration::from_millis(100);

pub struct SettingsApp {
    st: State,
    section: Section,
    panel: GlassPanel,
    /// Wakes the window when something in the settings folder changes.
    watcher: shellman_core::watch::Watcher,
    reload_due: bool,
    /// Keeps the single-instance mutex for as long as the process runs.
    _instance: Instance,
}

impl SettingsApp {
    pub fn new(
        ctx: &egui::Context,
        look: shellman_ui::appearance::Appearance,
        panel: GlassPanel,
        instance: Instance,
        page: Option<Section>,
    ) -> Self {
        let watcher = shellman_core::watch::Watcher::new(ctx.clone());
        if let Some(dir) = shellman_core::config::config_dir() {
            // The folder may not exist yet; saving makes it, and the first run saves the look.
            watcher.set_dirs(vec![dir]);
        }
        let glass = panel.composed();
        Self {
            st: State::new(ctx, look, glass),
            section: page.unwrap_or(Section::Appearance),
            panel,
            watcher,
            reload_due: false,
            _instance: instance,
        }
    }

    /// Follow the Windows app mode, and the Windows accent while that is the choice. Checked as
    /// frames come, not on a timer.
    fn track_system(&self, ctx: &egui::Context) {
        theme::set_light(ctx.theme() == egui::Theme::Light);
        if self.st.look.accent == AccentChoice::System {
            theme::set_accent(ctx, AccentChoice::System.resolve());
        }
    }

    /// Take in changes other apps saved (the dock pinning an app), once our own saves are done.
    fn follow_files(&mut self, ctx: &egui::Context) {
        if !self.watcher.poll().is_empty() {
            self.reload_due = true;
        }
        if !self.reload_due {
            return;
        }
        if self.st.saver.idle() {
            self.reload_due = false;
            self.st.reload(ctx);
        } else {
            ctx.request_repaint_after(RELOAD_RETRY);
        }
    }

    /// Go to the page another run asked for (`--page`, from Control Center's link).
    fn follow_requests(&mut self) {
        let asked = self._instance.requested.lock().ok().and_then(|mut r| r.take());
        if let Some(section) = asked {
            self.section = section;
        }
    }

    fn layout(&mut self, ctx: &egui::Context) {
        self.panel.set_style(Style::from_theme(page::GROUP_RADIUS));
        let mut list: Vec<Island> = Vec::new();
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
            let screen = ctx.screen_rect();
            let under = ui.painter().add(egui::Shape::Noop);
            islands::drag_gaps(ui, screen);

            // The title pill and the page share one column; the rail stands left of it.
            let left = screen.left() + MARGIN + RAIL_W + MARGIN;
            let room = screen.right() - MARGIN - left;
            let buttons_w = BUTTON_H * 3.0;
            let width = room.min(COLUMN_MAX);
            // Wider than the column, the window buttons sit at the window's corner instead.
            let apart = room > width + MARGIN + buttons_w + 16.0;
            let title = Rect::from_min_size(pos2(left, screen.top() + MARGIN), vec2(width, TITLE_H));
            let content_top = title.bottom() + MARGIN;

            list.push(Island::pill(title));
            self.title(ui, title, !apart);
            if apart {
                let corner = Rect::from_min_size(
                    pos2(screen.right() - MARGIN - buttons_w - 16.0, title.top()),
                    vec2(buttons_w + 16.0, TITLE_H),
                );
                list.push(Island::pill(corner));
                chips::window_buttons(ui, Rect::from_center_size(corner.center(), vec2(buttons_w, BUTTON_H)));
            }
            self.rail(ui, &mut list, screen, content_top);

            let area = Rect::from_min_max(pos2(left, content_top), pos2(left + width, screen.bottom() - MARGIN));
            self.page(ui, area, &mut list);

            if !self.panel.composed() {
                ui.painter().set(under, egui::Shape::Vec(islands::solid_shapes(&list, &self.panel.style())));
            }
        });
        self.panel.set_islands(&list, ctx.pixels_per_point());
    }

    /// The pill on top: "Settings / <page>", and the window buttons when `buttons` is set.
    fn title(&self, ui: &mut egui::Ui, rect: Rect, buttons: bool) {
        let c = colors();
        let y = rect.center().y;
        let name = ui.painter().text(pos2(rect.left() + 22.0, y), Align2::LEFT_CENTER, "Settings", semibold_font(15.0), c.text);
        let slash = ui.painter().text(pos2(name.right() + 10.0, y), Align2::LEFT_CENTER, "/", egui::FontId::proportional(15.0), c.muted);
        let label = self.section.label();
        let w = chips::chip_width(ui, label, false, Chip::Accent);
        let chip = Rect::from_center_size(pos2(slash.right() + 10.0 + w / 2.0, y), vec2(w, chips::CHIP_H));
        chips::chip(ui, chip, egui::Id::new("title-chip"), label, None, Chip::Accent);
        if buttons {
            chips::window_buttons(ui, Rect::from_min_max(pos2(rect.right() - 10.0 - BUTTON_H * 3.0, y - BUTTON_H / 2.0), pos2(rect.right() - 10.0, y + BUTTON_H / 2.0)));
        }
    }

    /// The pill of sections: one icon each, the current one filled with the accent.
    fn rail(&mut self, ui: &mut egui::Ui, list: &mut Vec<Island>, screen: Rect, top: f32) {
        let c = colors();
        let n = Section::ALL.len() as f32;
        let wanted = 28.0 + n * RAIL_ICON + (n - 1.0) * 4.0;
        let height = wanted.min(screen.bottom() - MARGIN - top);
        let rail = Rect::from_min_size(pos2(screen.left() + MARGIN, top), vec2(RAIL_W, height));
        list.push(Island::pill(rail));
        let (soft, hover, _) = chips::soft_colors(c.light);
        let mut y = rail.top() + 14.0;
        for section in Section::ALL {
            let cell = Rect::from_min_size(pos2(rail.center().x - RAIL_ICON / 2.0, y), vec2(RAIL_ICON, RAIL_ICON));
            y += RAIL_ICON + 4.0;
            if cell.bottom() > rail.bottom() {
                break;
            }
            let resp = ui.interact(cell, egui::Id::new(("rail", section.slug())), Sense::click());
            let current = self.section == section;
            let (fill, fg) = if current {
                (c.accent_fill, c.on_accent)
            } else if resp.hovered() {
                (hover, c.text)
            } else {
                (soft.gamma_multiply(0.0), c.muted)
            };
            ui.painter().rect_filled(cell, 12.0, fill);
            ui.painter().text(cell.center(), Align2::CENTER_CENTER, section.glyph(), icon_font(16.0), fg);
            if resp.on_hover_text(section.label()).clicked() {
                self.section = section;
            }
        }
    }

    /// The page, scrolling under the title pill: its groups become islands, clipped to `area`.
    fn page(&mut self, ui: &mut egui::Ui, area: Rect, list: &mut Vec<Island>) {
        page::begin_frame();
        ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
            ui.spacing_mut().scroll = egui::style::ScrollStyle::floating();
            // Dragging the gaps between islands moves the window, so the area must not take it.
            egui::ScrollArea::vertical().auto_shrink([false, false]).drag_to_scroll(false).show(ui, |ui| {
                // A page that declares no groups goes on one island as a whole.
                if self.section.grouped() {
                    self.section.show(ui, &mut self.st);
                } else {
                    page::group(ui, "", |ui| self.section.show(ui, &mut self.st));
                }
                // The workers run only while their page is shown.
                if self.section != Section::Sound {
                    self.st.sound.stop();
                }
                if self.section != Section::Display {
                    self.st.display.stop();
                }
                if self.section != Section::Startup {
                    self.st.startup.stop();
                }
            });
        });
        list.extend(page::take_groups().into_iter().map(|r| Island::new(r, page::GROUP_RADIUS)));
    }
}

impl eframe::App for SettingsApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.st.thumbs.poll(ctx);
        self.track_system(ctx);
        shellman_ui::app_icon::sync_window_icon(ctx, shellman_ui::app_icon::Kind::Settings);
        self.follow_files(ctx);
        self.follow_requests();
        self.layout(ctx);
    }
}
