#![windows_subsystem = "windows"]
//! Task Manager: what runs, what it uses, and ending what hangs. Islands of glass like Settings:
//! a rail of pages, a title pill with the search, and the page's islands.
//!
//! The processes and the machine are read on a thread of their own (`shellman_core::procs`) once
//! a second, and not at all while the window is minimised or hidden; the window repaints when a
//! reading comes in or on input.

mod actions;
mod app;
mod graph;
mod histories;
mod instance;
mod model;
mod performance;
mod processes;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_ui::appearance::Appearance;
use shellman_ui::glass::{GlassPanel, Style};
use shellman_ui::{theme, window};

fn main() -> eframe::Result {
    let first = match instance::claim() {
        instance::Claim::Signalled => return Ok(()),
        instance::Claim::First(first) => first,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Task Manager")
            .with_inner_size([1060.0, 720.0])
            .with_min_inner_size([760.0, 520.0])
            .with_transparent(true)
            .with_decorations(false),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "shellman-tasks",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            // SHELLMAN_FORCE_LIGHT: look at the light look without changing Windows' setting.
            if std::env::var_os("SHELLMAN_FORCE_LIGHT").is_some() {
                cc.egui_ctx.set_theme(egui::ThemePreference::Light);
            }
            theme::set_light(cc.egui_ctx.theme() == egui::Theme::Light);
            let look = Appearance::load_or(Appearance::default());
            look.apply(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx);
            let mut hwnd = 0isize;
            let mut panel = None;
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    hwnd = win32.hwnd.get();
                    window::frameless(hwnd);
                    panel = Some(GlassPanel::attach(hwnd, Style::from_theme(app::RADIUS)));
                }
            }
            first.serve(hwnd, cc.egui_ctx.clone());
            let panel = panel.ok_or("the window has no handle")?;
            Ok(Box::new(app::TasksApp::new(&cc.egui_ctx, look, panel, hwnd, first)))
        }),
    )
}
