#![windows_subsystem = "windows"]
//! Settings: one window for everything shellman can be set up for. It edits the same files the
//! other apps watch, so they follow as you change things.

mod app;
mod flow;
mod instance;
mod page;
mod pick;
mod saver;
mod search;
mod sections;
mod state;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_ui::appearance::Appearance;
use shellman_ui::glass::{GlassPanel, Style};
use shellman_ui::{theme, window};

fn main() -> eframe::Result {
    // A warming run (see `core::warm`): nothing to do but have run.
    if shellman_core::warm::asked() {
        return Ok(());
    }
    // `--page sound` opens on that page; a window already open is brought to it.
    let mut page = std::env::args()
        .skip_while(|a| a != "--page")
        .nth(1)
        .and_then(|slug| sections::Section::from_slug(&slug));
    // `--check-fonts`, at sign-in: window text comes back if a theme reset it, and the window
    // opens on the Fonts page only if a Windows update put Segoe UI back.
    if std::env::args().any(|a| a == shellman_sysfont::autostart::CHECK_FLAG) {
        if !shellman_sysfont::check::at_signin().segoe_back {
            return Ok(());
        }
        page = Some(sections::Section::Fonts);
    }
    let first = match instance::claim(page) {
        instance::Claim::Signalled => return Ok(()),
        instance::Claim::First(first) => first,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("shellman Settings")
            .with_inner_size([960.0, 720.0])
            .with_min_inner_size([700.0, 540.0])
            .with_transparent(true)
            .with_decorations(false),
        // Without this Windows picks the spot, and it cascades the window down under the dock.
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "shellman-settings",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            // SHELLMAN_FORCE_LIGHT: look at the light look without changing Windows' setting.
            // Otherwise follow Windows: the preference is saved with the window's state, so a
            // forced run would leave the app light for good.
            cc.egui_ctx.set_theme(theme::env::preference());
            // Windows' text size makes everything larger.
            theme::env::follow_text_scale(&cc.egui_ctx);
            theme::set_light(cc.egui_ctx.theme() == egui::Theme::Light);
            let look = Appearance::load_or(Appearance::default());
            look.apply(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx);
            let mut hwnd = 0isize;
            let mut panel = None;
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    hwnd = win32.hwnd.get();
                    // No frame of Windows' own; the glass pieces are laid out by the app.
                    window::frameless(hwnd);
                    panel = Some(GlassPanel::attach(hwnd, Style::from_theme(page::GROUP_RADIUS)));
                }
            }
            first.serve(hwnd, cc.egui_ctx.clone());
            let panel = panel.ok_or("the window has no handle")?;
            Ok(Box::new(app::SettingsApp::new(&cc.egui_ctx, look, panel, first, page)))
        }),
    )
}
