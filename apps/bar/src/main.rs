#![windows_subsystem = "windows"]
//! Shellman's bar: a top bar and a dock on every monitor, all windows of this one process, which
//! keeps one window list, one icon cache and the tray.
//!
//! `--no-shell` runs it beside a bar that is already the shell, for testing: no tray, and no
//! reserved space or work area changes.

mod app;
mod appbar;
mod bars;
mod dock;
mod fullscreen;
mod host;
mod monitors;
mod place;
mod preview;
mod reserve;
mod shape;
mod topbar;
mod tray;
mod wake;
mod workarea;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use reserve::Mode;
use shellman_ui::appearance::{self, Appearance};
use shellman_ui::{theme, window};

fn main() -> eframe::Result {
    let test = std::env::args().any(|a| a == "--no-shell");
    let options = eframe::NativeOptions {
        // The host: one pixel, cloaked, never seen (see `host.rs`).
        viewport: egui::ViewportBuilder::default()
            .with_title("shellman bar")
            .with_inner_size([1.0, 1.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            // Cloaked windows are not drawn; this also keeps clicks going through it.
            .with_mouse_passthrough(true)
            .with_taskbar(false),
        ..Default::default()
    };
    eframe::run_native(
        "shellman-bar",
        options,
        Box::new(move |cc| {
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
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    let hwnd = win32.hwnd.get();
                    // Whether Windows can frost a window; the host itself is never drawn.
                    glass = window::frost_window(windows::Win32::Foundation::HWND(hwnd as *mut _));
                    host::start(hwnd, cc.egui_ctx.clone());
                }
            }
            let wake = wake::Wake::new(cc.egui_ctx.clone());
            // Become the tray first: without Explorer, app-bar requests then come to shellman,
            // which declines them, and work areas are set directly instead.
            let tray = if test {
                None
            } else {
                let wake = wake.clone();
                tray::Tray::start(move || wake.bars())
            };
            let mode = match (test, tray.is_some()) {
                (true, _) => Mode::Off,
                (false, true) => Mode::Shell,
                (false, false) => Mode::Explorer,
            };
            let minimized = (mode == Mode::Shell).then(workarea::HiddenMinimized::start);
            let shared = app::Shared {
                dock: dock::Dock::new(cc.egui_ctx.clone(), glass, wake.clone()),
                top: topbar::TopBar::new(tray),
                bars: bars::Bars::new(mode),
                preview: preview::Preview::new(cc.egui_ctx.clone()),
                glass,
            };
            Ok(Box::new(app::Host::new(shared, wake, look, look_watch, minimized)))
        }),
    )
}
