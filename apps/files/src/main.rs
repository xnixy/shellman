#![windows_subsystem = "windows"]
//! Files: a file manager in Blurman's frosted style.

mod app;
mod settings;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_ui::{theme, window};
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
            .with_title("Files")
            .with_inner_size([1100.0, 700.0])
            .with_min_inner_size([640.0, 400.0])
            .with_transparent(true)
            .with_decorations(false),
        ..Default::default()
    };
    eframe::run_native(
        "shellman-files",
        options,
        Box::new(move |cc| {
            // Before the window is subclassed, so no message reaches `frame_proc` unseen.
            window::set_message_hook(shellman_core::shell_menu::forward);
            theme::install_fonts(&cc.egui_ctx);
            theme::set_light(cc.egui_ctx.theme() == egui::Theme::Light);
            // The shared look, which the settings app edits; Files only follows it.
            let look = shellman_ui::appearance::Appearance::load_or(Default::default());
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
            Ok(Box::new(app::FileMan::new(
                cc.egui_ctx.clone(),
                settings,
                look,
                arg,
                hwnd,
                glass,
            )))
        }),
    )
}
