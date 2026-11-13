#![windows_subsystem = "windows"]
//! Terminal: shells in tabs, in a glass window. The command line (`-d`, `-p`, `-e`/`--`,
//! `--hold`) is in `cli.rs`; a second start can hand its tab to the open window (`instance.rs`).

mod app;
mod cli;
mod clip;
mod config;
mod instance;
mod keys;
mod mouse;
mod osc;
mod palette;
mod profiles;
mod render;
mod select;
mod session;
mod snapshot;
mod stats;
mod tabs;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_ui::appearance::Appearance;
use shellman_ui::glass::{GlassPanel, Style};
use shellman_ui::{theme, window};

fn main() -> eframe::Result {
    let line = command_line().unwrap_or_default();
    let mut launch = match cli::parse(&line) {
        Ok(cli::Mode::Window(l)) => l,
        Err(e) => {
            say(&format!("shellman-terminal: {e}\n{}", cli::USAGE));
            std::process::exit(2);
        }
    };
    // The folder this was started in goes with the launch, so a window it is handed to opens
    // the tab there (the receiving window's own folder means nothing for it).
    if launch.dir.is_none() {
        launch.dir = app::start_dir();
    }
    if config::Config::load().new_instance == config::NewInstance::Tab && instance::hand_over(&launch) {
        return Ok(());
    }
    run_window(launch)
}

/// Print to the console this was started from, if any (a Windows-subsystem exe has none of its own).
fn say(text: &str) {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
    eprintln!("{text}");
}

fn run_window(launch: cli::Launch) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Terminal")
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
            Ok(Box::new(app::TerminalApp::new(&cc.egui_ctx, look, panel, launch)?))
        }),
    )
}

/// Everything after the exe on the command line, as it was typed (so quoting reaches the
/// program unchanged), or None when there is nothing.
fn command_line() -> Option<String> {
    use windows::Win32::System::Environment::GetCommandLineW;
    use windows::Win32::UI::Shell::PathGetArgsW;
    let rest = unsafe { PathGetArgsW(GetCommandLineW()).to_string().ok()? };
    let rest = rest.trim();
    (!rest.is_empty()).then(|| rest.to_owned())
}
