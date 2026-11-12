#![windows_subsystem = "windows"]
//! Files: a file manager in Blurman's frosted style.

mod app;
mod placement;
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
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Files")
        .with_inner_size([placement::DEFAULT_SIZE.0, placement::DEFAULT_SIZE.1])
        .with_min_inner_size([placement::MIN_SIZE.0, placement::MIN_SIZE.1])
        .with_transparent(true)
        .with_decorations(false);
    // Opened where it was left, or else centred on the monitor under the pointer. The builder
    // takes points, so the pixel rectangle is divided by the scale of the monitor it lands on.
    let mut at = None;
    if let Some((work, scale)) = placement::pointer_monitor() {
        let rect = placement::place(work, scale, settings.window, &placement::work_areas());
        at = Some(rect);
        let at = rect;
        let scale = placement::scale_at(at.left, at.top);
        viewport = viewport
            .with_position([at.left as f32 / scale, at.top as f32 / scale])
            .with_inner_size([at.width() as f32 / scale, at.height() as f32 / scale]);
    }
    let options = eframe::NativeOptions {
        viewport,
        // Placement is ours (above). The window library's own memory of the last position kept
        // pixels from one monitor and applied them on another, and it loads whatever file it
        // finds, so its state lives in a file of ours that never holds a window.
        persist_window: false,
        persistence_path: shellman_core::config::config_dir().map(|d| d.join("files-ui.ron")),
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
                    if let Some(at) = at {
                        placement::move_to(hwnd, at);
                    }
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
