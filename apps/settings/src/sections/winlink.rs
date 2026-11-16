//! "Open in Windows Settings": for what Windows only changes in its own Settings, a row shows the
//! value and a button opens Windows' Settings on that page (an `ms-settings:` link). Windows'
//! Settings needs the host for modern apps; when it is not running, the first click asks for it
//! (`app_host::want_now`, for the rest of this sign-in) and opens the page once it is up.

use crate::page;
use crate::state::State;
use egui::{RichText, Ui};
use shellman_ui::theme::{self, colors};

/// Open a link (`ms-settings:`, `windowsdefender:`) from a thread of its own (ShellExecute may
/// take a moment). Windows' Settings needs the host for modern apps: when it is not running, it is
/// asked for first (for the rest of this sign-in) and the link opens once it is up.
pub fn open_link(link: String) {
    use windows::core::HSTRING;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    std::thread::spawn(move || {
        if !shellman_core::app_host::want_now(std::time::Duration::from_secs(20)) {
            return;
        }
        // SAFETY: COM initialised on this thread first; the strings outlive the call.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(link), None, None, SW_SHOWNORMAL);
        }
    });
}

/// Whether Windows' Settings can be opened: it can now, or its host can be started on demand.
/// `SHELLMAN_PRETEND_HOST=on|off` shows the page as it is with or without it, without changing
/// anything (for screenshots).
pub fn host() -> bool {
    match std::env::var("SHELLMAN_PRETEND_HOST").as_deref() {
        Ok("on") => true,
        Ok("off") => false,
        _ => shellman_core::app_host::modern_apps_reachable(),
    }
}

/// The button: opens `uri` when Windows' Settings can open, dimmed with the reason when not.
/// `name` is what a screen reader says, when the label alone would not tell the buttons apart.
pub fn button(ui: &mut Ui, uri: &str, host: bool, label: &str, name: Option<&str>) {
    // Always clickable: without the host, a click starts it for this sign-in first.
    let resp = ui.add(theme::secondary_button(label));
    if let Some(name) = name {
        theme::speak(&resp, egui::WidgetType::Button, || name.to_owned());
    }
    let resp = if host { resp.on_hover_text(uri) } else { resp.on_hover_text("Starts Windows' host for Settings for this sign-in, then opens the page (a few seconds the first time).") };
    if resp.clicked() {
        open_link(uri.to_owned());
    }
}

/// A read-only setting: its title and what Windows has now on the left, the button on the right.
pub fn row(ui: &mut Ui, title: &str, value: &str, uri: &str, host: bool) {
    let c = colors();
    theme::trailing(
        ui,
        |ui| button(ui, uri, host, "Open in Windows Settings", Some(&format!("Open {title} in Windows Settings"))),
        |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(egui::Label::new(RichText::new(title).size(13.0).color(c.text)).wrap());
                if !value.is_empty() {
                    ui.add(egui::Label::new(RichText::new(value).size(11.5).color(c.muted)).wrap());
                }
            });
        },
    );
}

/// Without the host (only when "Never" is chosen): an island saying why the buttons are off,
/// with the choice.
pub fn host_group(ui: &mut Ui, st: &mut State, host: bool) {
    if host {
        return;
    }
    page::group(ui, "Windows Settings", |ui| {
        ui.add(egui::Label::new(theme::muted("Some of these settings change only in Windows' own Settings, which needs its host to open.").size(12.0)).wrap());
        ui.add_space(10.0);
        windows_apps_choice(ui, st);
    });
}

/// When Windows' host for Settings, Calculator and the Store runs: three chips and what is true
/// now. The one control for it (About, and wherever a page needs Windows' Settings).
pub fn windows_apps_choice(ui: &mut Ui, st: &mut State) {
    use shellman_core::settings::shell::WindowsApps;
    let options = [(WindowsApps::WhenNeeded, "When needed"), (WindowsApps::Always, "Always"), (WindowsApps::Never, "Never")];
    if let Some(choice) = shellman_ui::look_controls::choice_chips(ui, st.shell.windows_apps(), &options, true) {
        st.shell.set_windows_apps(choice);
        st.save_shell();
    }
    ui.add_space(6.0);
    let what = match st.shell.windows_apps() {
        WindowsApps::WhenNeeded => "Starts the first time you open one of them, until you sign out.",
        WindowsApps::Always => "Starts at every sign-in, so they open at once.",
        WindowsApps::Never => "Does not start: those apps and Windows Settings links do not open.",
    };
    let now = match shellman_core::app_host::running_since() {
        Some(Some(at)) => format!("Running since {} (started when one of them was opened).", local_hm(at)),
        Some(None) => "Running now.".to_owned(),
        None if st.shell.windows_apps() == WindowsApps::Never => "Not running.".to_owned(),
        None => "Not running now.".to_owned(),
    };
    ui.add(egui::Label::new(theme::muted(format!("{what} {now}")).size(12.0)).wrap());
    // It may start or stop by itself (a click elsewhere, the desktop's supervisor).
    ui.ctx().request_repaint_after(std::time::Duration::from_secs(2));
}

/// A time today as "22:31", in the local time zone.
fn local_hm(t: std::time::SystemTime) -> String {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let Ok(d) = t.duration_since(std::time::SystemTime::UNIX_EPOCH) else { return "?".into() };
    let ticks = (d.as_secs() + 11_644_473_600) * 10_000_000;
    let ft = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    // SAFETY: plain conversions between stack structs.
    let ok = unsafe { FileTimeToSystemTime(&ft, &mut utc).is_ok() && SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_ok() };
    if ok { format!("{:02}:{:02}", local.wHour, local.wMinute) } else { "?".into() }
}

/// A switch row that may be locked (a policy, or Windows' own decision): dimmed and not
/// clickable then, with `why` as its detail.
pub fn setting(ui: &mut Ui, title: &str, detail: &str, on: bool, locked: Option<&str>) -> Option<bool> {
    let mut v = on;
    let detail = locked.unwrap_or(detail);
    let changed = ui.add_enabled_ui(locked.is_none(), |ui| theme::setting_row(ui, title, detail, &mut v).changed()).inner;
    (changed && v != on).then_some(v)
}
