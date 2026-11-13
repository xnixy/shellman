#![windows_subsystem = "windows"]
//! Terminal: one shell in a glass window. `shellman-terminal [command line]` runs the command
//! line (default `powershell.exe`) in a Windows pseudo-console.

mod app;
mod keys;
mod palette;
mod render;
mod session;
mod snapshot;
mod stats;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_ui::appearance::Appearance;
use shellman_ui::glass::{GlassPanel, Style};
use shellman_ui::{theme, window};

/// What runs when no command line is given.
const DEFAULT_SHELL: &str = "powershell.exe";

fn main() -> eframe::Result {
    let command = command_line().unwrap_or_else(|| DEFAULT_SHELL.to_owned());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("shellman Terminal")
            .with_inner_size([960.0, 600.0])
            .with_min_inner_size([420.0, 260.0])
            .with_transparent(true)
            .with_decorations(false),
        centered: true,
        // The grid is redrawn only when something changed; vsync caps a flood of output.
        vsync: true,
        ..Default::default()
    };
    eframe::run_native(
        "shellman-terminal",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            // SHELLMAN_FORCE_LIGHT: look at the light look without changing Windows' setting.
            cc.egui_ctx.set_theme(if std::env::var_os("SHELLMAN_FORCE_LIGHT").is_some() {
                egui::ThemePreference::Light
            } else {
                egui::ThemePreference::System
            });
            theme::set_light(cc.egui_ctx.theme() == egui::Theme::Light);
            let look = Appearance::load_or(Appearance::default());
            look.apply(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx);
            let mut panel = None;
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    let hwnd = win32.hwnd.get();
                    window::frameless(hwnd);
                    panel = Some(GlassPanel::attach(hwnd, Style::from_theme(app::RADIUS)));
                }
            }
            let panel = panel.ok_or("the window has no handle")?;
            Ok(Box::new(app::TerminalApp::new(&cc.egui_ctx, look, panel, &command)?))
        }),
    )
}

/// Everything after the exe on the command line, as it was typed (so quoting reaches the
/// program unchanged), or None when there is nothing.
fn command_line() -> Option<String> {
    use windows::Win32::UI::Shell::PathGetArgsW;
    use windows::Win32::System::Environment::GetCommandLineW;
    let rest = unsafe { PathGetArgsW(GetCommandLineW()).to_string().ok()? };
    let rest = rest.trim();
    (!rest.is_empty()).then(|| rest.to_owned())
}
