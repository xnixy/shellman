#![windows_subsystem = "windows"]
//! Fileman: a file manager in Blurman's frosted style.

mod app;
mod fsops;
mod settings;
mod shell_menu;
mod theme;
mod thumbs;
mod watch;
mod window;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::path::PathBuf;
use windows::Win32::Foundation::HWND;

fn main() -> eframe::Result {
    let settings = settings::load_settings();
    // A folder passed on the command line opens in its own tab.
    let arg = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .filter(|p| p.is_dir());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Fileman")
            .with_inner_size([1100.0, 700.0])
            .with_min_inner_size([640.0, 400.0])
            .with_transparent(true)
            .with_decorations(false),
        ..Default::default()
    };
    eframe::run_native(
        "Fileman",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            theme::set_light(cc.egui_ctx.theme() == egui::Theme::Light);
            theme::set_accent(&cc.egui_ctx, settings.accent.resolve());
            theme::set_glass(&cc.egui_ctx, settings.glass);
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
            Ok(Box::new(app::FileMan::new(
                cc.egui_ctx.clone(),
                settings,
                arg,
                hwnd,
                glass,
            )))
        }),
    )
}
