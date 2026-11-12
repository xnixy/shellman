#![windows_subsystem = "windows"]
//! Settings: one window for everything shellman can be set up for. It edits the same files the
//! other apps watch, so they follow as you change things.

mod app;
mod instance;
mod page;
mod saver;
mod sections;
mod state;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_ui::appearance::Appearance;
use shellman_ui::{theme, window};
use windows::Win32::Foundation::HWND;

fn main() -> eframe::Result {
    let first = match instance::claim() {
        instance::Claim::Signalled => return Ok(()),
        instance::Claim::First(first) => first,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("shellman Settings")
            .with_icon(egui::IconData { rgba: include_bytes!("../../../assets/icons/settings-64.rgba").to_vec(), width: 64, height: 64 })
            .with_inner_size([880.0, 620.0])
            .with_min_inner_size([640.0, 420.0])
            .with_transparent(true)
            .with_decorations(false),
        ..Default::default()
    };
    eframe::run_native(
        "shellman-settings",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            theme::set_light(cc.egui_ctx.theme() == egui::Theme::Light);
            let look = Appearance::load_or(Appearance::default());
            look.apply(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx);
            let mut hwnd = 0isize;
            let mut glass = false;
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    hwnd = win32.hwnd.get();
                    glass = window::frost_window(HWND(hwnd as *mut _));
                    window::remove_caption(HWND(hwnd as *mut _), glass);
                }
            }
            first.serve(hwnd, cc.egui_ctx.clone());
            Ok(Box::new(app::SettingsApp::new(&cc.egui_ctx, look, hwnd, glass, first)))
        }),
    )
}
