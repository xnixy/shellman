#![windows_subsystem = "windows"]
//! Shellman's bar: for now, the dock. The top bar joins it in this process later.

mod dock;
mod place;
mod shape;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_ui::{theme, window};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("shellman dock")
            .with_inner_size([400.0, dock::HEIGHT])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_always_on_top()
            // No taskbar button of its own.
            .with_taskbar(false),
        ..Default::default()
    };
    eframe::run_native(
        "shellman-bar",
        options,
        Box::new(|cc| {
            theme::install_fonts(&cc.egui_ctx);
            theme::set_light(cc.egui_ctx.theme() == egui::Theme::Light);
            theme::set_accent(&cc.egui_ctx, theme::AccentChoice::System.resolve());
            theme::apply(&cc.egui_ctx);
            let mut glass = false;
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    let hwnd = win32.hwnd.get();
                    glass = window::frost_window(windows::Win32::Foundation::HWND(hwnd as *mut _));
                    shape::never_activate(hwnd);
                }
            }
            Ok(Box::new(dock::Dock::new(cc.egui_ctx.clone(), glass)))
        }),
    )
}
