//! About: the name, the version and updates, where the settings live, and the host for Windows'
//! own apps.

use crate::page;
use crate::state::State;
use shellman_ui::theme::{self, colors};
use shellman_ui::widgets::switch;
use shellman_update_core::state::{Outcome, State as UpdateState};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// "Check now": the running `shellman-update --now`, and what `update.json` said when last read.
/// Kept here rather than in `State`: nothing else needs it.
struct Check {
    child: Option<std::process::Child>,
    seen: Option<(Instant, UpdateState)>,
    /// Why the helper could not be started.
    error: Option<String>,
}

static CHECK: Mutex<Check> = Mutex::new(Check { child: None, seen: None, error: None });

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "About", "shellman replaces everything Explorer does, in one style.");

    page::group(ui, "", |ui| {
        ui.label(
            egui::RichText::new("shellman Settings")
                .size(15.0)
                .strong()
                .color(colors().text),
        );
        ui.label(theme::muted(format!("Version {}", version())));
    });

    page::group(ui, "Updates", |ui| updates(ui, st));

    page::group(ui, "Windows apps", |ui| {
        if switch(ui, &mut st.shell.modern_apps, "Keep Settings, Calculator and the Store working").changed() {
            st.save_shell();
        }
        // Without Explorer these apps only open while Windows' own host for them runs; the
        // desktop starts and watches it (`app_host`). One window lookup, so asked every frame.
        let now = if shellman_core::app_host::modern_apps_available() { "They can open now." } else { "They cannot open right now." };
        ui.label(theme::muted(format!("Runs Windows' own host for them while Explorer is not running. {now}")).size(12.0));
    });

    page::group(ui, "Settings folder", folder);
}

fn folder(ui: &mut egui::Ui) {
    match shellman_core::config::config_dir() {
        Some(dir) => {
            ui.label(egui::RichText::new(dir.display().to_string()).color(colors().text));
            ui.add_space(8.0);
            if ui.button("Open settings folder").clicked() {
                // It is created by the first save; make sure there is something to open.
                let _ = std::fs::create_dir_all(&dir);
                // In Files, not whatever the system opens folders with (Explorer).
                if !shellman_core::sibling::start_with("files", &[&dir.display().to_string()]) {
                    shellman_core::fsops::open(&dir);
                }
            }
        }
        None => {
            ui.label(theme::muted("Windows did not say where."));
        }
    }
}

/// The version this build is: `0.2.0` for a release, `0.1.0+1d46382` for a developer build.
fn version() -> String {
    shellman_buildinfo::STAMP.split_whitespace().next().unwrap_or(shellman_buildinfo::STAMP).to_string()
}

fn updates(ui: &mut egui::Ui, st: &mut State) {
    if switch(ui, &mut st.shell.auto_update, "Update automatically").changed() {
        st.save_shell();
    }
    ui.label(theme::muted("Looks for a new release about 5 minutes after you sign in, then once a day. A release is checked against shellman's signing key before it runs, and is never installed while an app is fullscreen or files are being copied. The apps restart; you are never signed out.").size(12.0));
    ui.add_space(8.0);

    let Ok(mut c) = CHECK.lock() else { return };
    // The helper ends by itself; read `update.json` again every second while it runs, else
    // every few seconds (the session's scheduled check may be writing it).
    let running = match c.child.as_mut().map(|ch| ch.try_wait()) {
        Some(Ok(None)) => true,
        Some(_) => {
            c.child = None;
            c.seen = None;
            false
        }
        None => false,
    };
    let every = if running { Duration::from_secs(1) } else { Duration::from_secs(5) };
    if c.seen.as_ref().is_none_or(|(t, _)| t.elapsed() >= every) {
        c.seen = Some((Instant::now(), UpdateState::load()));
    }
    ui.ctx().request_repaint_after(every);
    let state = c.seen.as_ref().map(|(_, s)| s.clone()).unwrap_or_default();
    let busy = running || state.outcome == Outcome::Working;

    ui.horizontal(|ui| {
        if ui.add_enabled(!busy, egui::Button::new("Check now")).clicked() {
            match start_check() {
                Ok(child) => {
                    c.child = Some(child);
                    c.error = None;
                    c.seen = None;
                }
                Err(e) => c.error = Some(e),
            }
        }
        if busy {
            ui.spinner();
        }
    });
    ui.add_space(4.0);
    let line = match &c.error {
        Some(e) => e.clone(),
        None if running && state.outcome != Outcome::Working => "Checking…".to_string(),
        None => state.summary(),
    };
    ui.label(egui::RichText::new(line).color(colors().text));
    if state.last_check > 0 && !busy {
        ui.label(theme::muted(format!("Last checked {}.", ago(shellman_update_core::state::now().saturating_sub(state.last_check)))).size(12.0));
    }
    ui.add_space(6.0);
    let page = if state.page.starts_with("https://") { state.page.clone() } else { shellman_update_core::RELEASES_PAGE.to_string() };
    if ui.link("Release notes").on_hover_text(&page).clicked() {
        open_link(page);
    }
}

/// Start `shellman-update --now` from beside this exe (it installs what it finds, whatever the
/// switch above says: pressing the button is asking for it).
fn start_check() -> Result<std::process::Child, String> {
    let helper = std::env::current_exe().map_err(|e| e.to_string())?.with_file_name("shellman-update.exe");
    if !helper.is_file() {
        return Err("shellman-update.exe is not next to Settings, so it cannot check.".into());
    }
    std::process::Command::new(&helper).arg("--now").spawn().map_err(|e| format!("Could not start the check: {e}"))
}

fn ago(secs: u64) -> String {
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

/// Open a web page in the default browser, from a thread of its own (ShellExecute may take a
/// moment).
fn open_link(link: String) {
    use windows::core::HSTRING;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    std::thread::spawn(move || {
        // SAFETY: COM initialised on this thread first; the strings outlive the call.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(link), None, None, SW_SHOWNORMAL);
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn ages_read_plainly() {
        assert_eq!(super::ago(5), "just now");
        assert_eq!(super::ago(125), "2 min ago");
        assert_eq!(super::ago(7200), "2 h ago");
        assert_eq!(super::ago(3 * 86_400), "3 days ago");
    }
}
