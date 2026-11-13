#![windows_subsystem = "windows"]
//! Terminal: shells in tabs, in a glass window. The command line (`-d`, `-p`, `-e`/`--`,
//! `--hold`) is in `cli.rs`; a second start can hand its tab to the open window (`instance.rs`);
//! `-Embedding` is COM starting it to show a console Windows hands over (`defterm`).

mod app;
mod cli;
mod clip;
mod config;
mod conpty;
mod defterm;
mod instance;
mod keys;
mod mouse;
mod osc;
mod palette;
mod profiles;
mod pty;
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
        Ok(cli::Mode::Embedding) => return embedding(),
        Ok(cli::Mode::RegisterDefault) => {
            attach_console();
            std::process::exit(defterm::register());
        }
        Ok(cli::Mode::UnregisterDefault) => {
            attach_console();
            std::process::exit(defterm::unregister());
        }
        Err(e) => {
            attach_console();
            eprintln!("shellman-terminal: {e}\n{}", cli::USAGE);
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
    run_window(launch, None)
}

/// COM started this for a console handoff: take it, then show it (or, for a console started
/// hidden, run it with no window until its program ends).
fn embedding() -> eframe::Result {
    let rx = match defterm::server::serve() {
        Ok(rx) => rx,
        Err(e) => {
            eprintln!("shellman-terminal: {e}");
            std::process::exit(1);
        }
    };
    let Ok(arrived) = rx.recv_timeout(std::time::Duration::from_secs(30)) else {
        std::process::exit(0);
    };
    if arrived.hidden() {
        app::run_hidden(arrived);
        std::process::exit(0);
    }
    run_window(cli::Launch::default(), Some(arrived))
}

/// Make `println!` reach the console this was started from (a Windows-subsystem exe has none of
/// its own). Redirected output already works.
fn attach_console() {
    use windows::core::w;
    use windows::Win32::Foundation::GENERIC_WRITE;
    use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows::Win32::System::Console::{
        AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    };
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        let has_out = GetStdHandle(STD_OUTPUT_HANDLE).is_ok_and(|h| !h.is_invalid() && !h.0.is_null());
        if !has_out {
            if let Ok(con) = CreateFileW(w!("CONOUT$"), GENERIC_WRITE.0, FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, None) {
                let _ = SetStdHandle(STD_OUTPUT_HANDLE, con);
                let _ = SetStdHandle(STD_ERROR_HANDLE, con);
            }
        }
    }
}

fn run_window(launch: cli::Launch, handoff: Option<defterm::server::Arrived>) -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Terminal")
        .with_inner_size([960.0, 600.0])
        .with_min_inner_size([420.0, 260.0])
        .with_transparent(true)
        .with_decorations(false);
    // A console started with a position (STARTUPINFO's dwX/dwY) opens there.
    let position = handoff.as_ref().and_then(|a| a.position);
    if let Some((x, y)) = position {
        let scale = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() }.max(96) as f32 / 96.0;
        viewport = viewport.with_position([x as f32 / scale, y as f32 / scale]);
    }
    let options = eframe::NativeOptions {
        viewport,
        centered: position.is_none(),
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
            Ok(Box::new(app::TerminalApp::new(&cc.egui_ctx, look, panel, launch, handoff)?))
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
