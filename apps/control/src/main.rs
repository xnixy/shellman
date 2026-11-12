#![windows_subsystem = "windows"]
//! Control centre: Wi-Fi, Bluetooth, sound, brightness, now playing and power, in a glass panel
//! that drops from the top-right corner of the screen under the pointer.
//!
//! Run once, it stays resident with its window hidden; run again (the top bar's status group
//! does), it tells the resident one to show or hide, and exits. `--background` starts the
//! resident process without showing the panel.

mod app;
mod outside;
mod place;
mod power_menu;
mod settings_page;
mod summon;
mod views;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_ui::appearance::{self, Appearance};
use shellman_ui::glass::{GlassPanel, Style};
use shellman_ui::theme;
use std::sync::Arc;

fn main() -> eframe::Result {
    let background = std::env::args().any(|a| a == "--background");
    let instance = match summon::claim(background) {
        summon::Claim::Signalled => return Ok(()),
        summon::Claim::Resident(instance) => instance,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("shellman control centre")
            .with_inner_size([place::WIDTH, 480.0])
            .with_visible(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_always_on_top()
            .with_taskbar(false),
        ..Default::default()
    };
    eframe::run_native(
        "shellman-control",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            theme::set_light(cc.egui_ctx.theme() == egui::Theme::Light);
            theme::apply(&cc.egui_ctx);
            let look = Appearance::load_or(Appearance::default());
            look.apply(&cc.egui_ctx);
            let mut hwnd = 0isize;
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    hwnd = win32.hwnd.get();
                }
            }
            // Rounded glass under the panel; without it, the panel paints its own surface.
            let panel = (hwnd != 0).then(|| GlassPanel::attach(hwnd, Style::from_theme(place::RADIUS)));
            let glass = panel.as_ref().is_some_and(GlassPanel::composed);
            if hwnd != 0 {
                summon::prepare(hwnd);
            }
            let shared = Arc::new(summon::Shared::default());
            instance.serve(hwnd, cc.egui_ctx.clone(), shared.clone());
            if !background {
                instance.summon();
            }
            let look_watch = shellman_core::watch::Watcher::new(cc.egui_ctx.clone());
            if let Some(dir) = appearance::file().and_then(|f| f.parent().map(|p| p.to_path_buf())) {
                look_watch.set_dirs(vec![dir]);
            }
            Ok(Box::new(Shell {
                inner: app::Control::new(cc.egui_ctx.clone(), hwnd, glass, shared),
                panel,
                look,
                look_watch,
                _instance: instance,
            }))
        }),
    )
}

/// The window's shell: follows the shared look live, and hands each frame to the panel.
struct Shell {
    inner: app::Control,
    /// The panel's rounded glass (None only without a window handle).
    panel: Option<GlassPanel>,
    look: Appearance,
    look_watch: shellman_core::watch::Watcher,
    /// Holds the single-instance mutex for as long as the process runs.
    _instance: summon::Instance,
}

impl eframe::App for Shell {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.look_watch.poll().is_empty() {
            let look = Appearance::load_or(self.look);
            if look != self.look {
                self.look = look;
                look.apply(ctx);
            }
        }
        self.inner.frame(ctx);
        // After the frame, which takes the light or dark mode. Free when the look is unchanged.
        if let Some(panel) = &self.panel {
            panel.set_style(Style::from_theme(place::RADIUS));
        }
    }
}
