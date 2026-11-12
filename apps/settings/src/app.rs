//! The window: a sidebar of sections on the left, the chosen page on a sheet on the right. The
//! top of both is the title bar, so the glass runs unbroken to the window's edge.

use crate::instance::Instance;
use crate::sections::Section;
use crate::state::State;
use egui::{Align, CornerRadius, Layout, Sense, Stroke, Vec2};
use shellman_ui::nav::{nav_row, sidebar_title};
use shellman_ui::theme::{self, colors, glyph, AccentChoice};
use shellman_ui::widgets::{caption_button, window_drag};
use std::time::Duration;

const SIDEBAR_W: f32 = 200.0;
/// Height of the title bar when Windows draws no caption buttons to measure.
const CAPTION_H: f32 = 40.0;
/// The page's content is kept to a readable width, however wide the window.
const PAGE_W: f32 = 560.0;
/// How long to wait before reading the files again if our own saves were still going.
const RELOAD_RETRY: Duration = Duration::from_millis(100);

pub struct SettingsApp {
    st: State,
    section: Section,
    /// Wakes the window when something in the settings folder changes.
    watcher: shellman_core::watch::Watcher,
    reload_due: bool,
    /// Size of the Windows caption buttons in points, when Windows draws them over the glass.
    caption: Option<Vec2>,
    /// Keeps the single-instance mutex for as long as the process runs.
    _instance: Instance,
}

impl SettingsApp {
    pub fn new(
        ctx: &egui::Context,
        look: shellman_ui::appearance::Appearance,
        hwnd: isize,
        glass: bool,
        instance: Instance,
        page: Option<Section>,
    ) -> Self {
        let watcher = shellman_core::watch::Watcher::new(ctx.clone());
        if let Some(dir) = shellman_core::config::config_dir() {
            // The folder may not exist yet; saving makes it, and the first run saves the look.
            watcher.set_dirs(vec![dir]);
        }
        Self {
            st: State::new(ctx, look, hwnd, glass),
            section: page.unwrap_or(Section::Appearance),
            watcher,
            reload_due: false,
            caption: None,
            _instance: instance,
        }
    }

    /// Follow the Windows app mode, and the Windows accent while that is the choice. Checked as
    /// frames come, not on a timer.
    fn track_system(&self, ctx: &egui::Context) {
        let mut repaint_frame = theme::set_light(ctx.theme() == egui::Theme::Light);
        if self.st.look.accent == AccentChoice::System {
            repaint_frame |= theme::set_accent(ctx, AccentChoice::System.resolve());
        }
        if repaint_frame {
            shellman_ui::window::paint_frame(self.st.hwnd);
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
        let ppp = ctx.pixels_per_point();
        self.caption = if self.st.glass {
            shellman_ui::window::caption_buttons_size(self.st.hwnd)
                .map(|(w, h)| Vec2::new(w as f32 / ppp, h as f32 / ppp))
        } else {
            None
        };
        let caption_h = self.caption.map_or(CAPTION_H, |s| s.y.clamp(32.0, 48.0));
        let c = colors();
        // One even tint over the whole window; nothing else paints a band of its own.
        ctx.layer_painter(egui::LayerId::background()).rect_filled(
            ctx.screen_rect(),
            CornerRadius::ZERO,
            if self.st.glass { c.shell } else { c.bg },
        );
        egui::SidePanel::left("sidebar")
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 8, top: 0, bottom: 10 }))
            .exact_width(SIDEBAR_W)
            .resizable(false)
            .show_separator_line(false)
            .show(ctx, |ui| {
                sidebar_title(ui, caption_h, "Settings");
                ui.add_space(2.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                for section in Section::ALL {
                    if nav_row(ui, section.glyph(), section.label(), self.section == section).clicked() {
                        self.section = section;
                    }
                }
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 10, top: 0, bottom: 10 }))
            .show(ctx, |ui| {
                egui::TopBottomPanel::top("title")
                    .frame(egui::Frame::new())
                    .show_separator_line(false)
                    .exact_height(caption_h)
                    .show_inside(ui, |ui| self.title_bar(ui, caption_h));
                self.sheet(ui);
            });
    }

    /// The strip along the top: it drags the window, and holds the caption buttons when Windows
    /// does not draw them.
    fn title_bar(&self, ui: &mut egui::Ui, height: f32) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if self.caption.is_none() {
                let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
                if caption_button(ui, glyph::CHROME_CLOSE, true, height).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                let g = if maximized { glyph::RESTORE } else { glyph::MAXIMIZE };
                if caption_button(ui, g, false, height).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
                if caption_button(ui, glyph::MINIMIZE, false, height).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
            }
            let (_, drag) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), height),
                Sense::click_and_drag(),
            );
            window_drag(ui, &drag);
        });
    }

    /// The page, on the lighter sheet, scrolling when it is taller than the window.
    fn sheet(&mut self, ui: &mut egui::Ui) {
        let c = colors();
        egui::Frame::new()
            .fill(c.sheet)
            .stroke(Stroke::new(1.0_f32, c.hairline))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(egui::Margin::same(8))
            // Over glass the sheet is see-through, and a shadow under it would darken its inside.
            .shadow(egui::Shadow {
                offset: [0, 2],
                blur: 16,
                spread: 0,
                color: if self.st.glass { egui::Color32::TRANSPARENT } else { c.shadow },
            })
            .show(ui, |ui| {
                let size = ui.available_size();
                ui.set_min_size(size);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        ui.vertical(|ui| {
                            ui.set_max_width(PAGE_W.min(ui.available_width() - 24.0));
                            self.section.show(ui, &mut self.st);
                            // The sound worker runs only while its page is shown.
                            if self.section != Section::Sound {
                                self.st.sound.stop();
                            }
                            if self.section != Section::Display {
                                self.st.display.stop();
                            }
                            if self.section != Section::Startup {
                                self.st.startup.stop();
                            }
                            ui.add_space(24.0);
                        });
                    });
                });
            });
    }
}

impl eframe::App for SettingsApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.st.glass {
            [0.0, 0.0, 0.0, 0.0]
        } else {
            colors().bg.to_normalized_gamma_f32()
        }
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
