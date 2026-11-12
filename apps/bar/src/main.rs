#![windows_subsystem = "windows"]
//! Shellman's bar: for now, the dock. The top bar joins it in this process later.

mod appbar;
mod dock;
mod place;
mod shape;
mod topbar;
mod tray;
mod workarea;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_ui::appearance::{self, Appearance};
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
            theme::apply(&cc.egui_ctx);
            let look = Appearance::load_or(Appearance::default());
            look.apply(&cc.egui_ctx);
            // Follow the look live: Files saves it when you change the accent or glass.
            let look_watch = shellman_core::watch::Watcher::new(cc.egui_ctx.clone());
            if let Some(dir) = appearance::file().and_then(|f| f.parent().map(|p| p.to_path_buf())) {
                look_watch.set_dirs(vec![dir]);
            }
            let mut glass = false;
            let mut dock_hwnd = 0;
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    let hwnd = win32.hwnd.get();
                    dock_hwnd = hwnd;
                    glass = window::frost_window(windows::Win32::Foundation::HWND(hwnd as *mut _));
                    shape::never_activate(hwnd);
                }
            }
            Ok(Box::new(Bar {
                dock: dock::Dock::new(cc.egui_ctx.clone(), glass, dock_hwnd),
                top: topbar::TopBar::default(),
                glass,
                look,
                look_watch,
            }))
        }),
    )
}

/// The bar process: the dock is its main window, the top bar a second one. Both repaint only on
/// events (window changes, input) and the top bar's clock minute.
struct Bar {
    dock: dock::Dock,
    top: topbar::TopBar,
    glass: bool,
    look: Appearance,
    look_watch: shellman_core::watch::Watcher,
}

impl eframe::App for Bar {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.look_watch.poll().is_empty() {
            // The same folder holds the dock's pin file.
            self.dock.reload_pins();
            let look = Appearance::load_or(self.look);
            if look != self.look {
                self.look = look;
                look.apply(ctx);
            }
        }
        self.dock.frame(ctx);
        self.top.show(ctx, self.dock.front_window(), self.glass);
    }
}
