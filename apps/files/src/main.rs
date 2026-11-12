#![windows_subsystem = "windows"]
//! Files: a file manager in Blurman's frosted style.

mod app;
mod args;
mod placement;
mod settings;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_ui::glass::{GlassPanel, Style};
use shellman_ui::{theme, window};

fn main() -> eframe::Result {
    let settings = settings::load_settings();
    // A folder, or the Recycle Bin, passed on the command line opens in its own tab.
    let start = args::parse(std::env::args_os().skip(1));
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
            theme::install_fonts(&cc.egui_ctx);
            theme::set_light(
                std::env::var_os("SHELLMAN_FORCE_LIGHT").is_some()
                    || cc.egui_ctx.theme() == egui::Theme::Light,
            );
            // The shared look, which the settings app edits; Files only follows it.
            let look = shellman_ui::appearance::Appearance::load_or(Default::default());
            look.apply(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx);
            let mut hwnd = 0isize;
            let mut panel = None;
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    hwnd = win32.hwnd.get();
                    if let Some(at) = at {
                        placement::move_to(hwnd, at);
                    }
                    // No frame and no window backdrop: the window is transparent, and its
                    // islands are pieces of glass under it (see `app::layout`). The resize edges
                    // and Snap stay.
                    window::frameless(hwnd);
                    panel = Some(GlassPanel::attach(hwnd, Style::from_theme(app::RADIUS)));
                }
            }
            Ok(Box::new(app::FileMan::new(
                cc.egui_ctx.clone(),
                settings,
                look,
                start,
                hwnd,
                panel,
            )))
        }),
    )
}
