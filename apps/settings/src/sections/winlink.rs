//! "Open in Windows Settings": for what Windows only changes in its own Settings, a row shows the
//! value and a button opens Windows' Settings on that page (an `ms-settings:` link). Windows'
//! Settings needs the host for modern apps; without it the buttons are off and the page says how
//! to turn the host on.

use crate::page;
use crate::state::State;
use egui::{RichText, Ui};
use shellman_ui::theme::{self, colors};
use shellman_ui::widgets::switch;

/// Open a link (`ms-settings:`, `windowsdefender:`) from a thread of its own (ShellExecute may
/// take a moment).
pub fn open_link(link: String) {
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

/// Whether Windows' Settings can open now. `SHELLMAN_PRETEND_HOST=on|off` shows the page as it is
/// with the host on or off, without changing it (for screenshots).
pub fn host() -> bool {
    match std::env::var("SHELLMAN_PRETEND_HOST").as_deref() {
        Ok("on") => true,
        Ok("off") => false,
        _ => shellman_core::app_host::modern_apps_available(),
    }
}

/// The button: opens `uri` when Windows' Settings can open, dimmed with the reason when not.
/// `name` is what a screen reader says, when the label alone would not tell the buttons apart.
pub fn button(ui: &mut Ui, uri: &str, host: bool, label: &str, name: Option<&str>) {
    let resp = ui.add_enabled(host, theme::secondary_button(label));
    if let Some(name) = name {
        theme::speak(&resp, egui::WidgetType::Button, || name.to_owned());
    }
    let resp = if host { resp.on_hover_text(uri) } else { resp.on_disabled_hover_text("Windows Settings is not running: turn on the host for Settings at the top of this page.") };
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

/// Without the host: an island saying why the buttons are off, with the host's switch (the
/// same one Default apps offers).
pub fn host_group(ui: &mut Ui, st: &mut State, host: bool) {
    if host {
        return;
    }
    page::group(ui, "Windows Settings", |ui| {
        ui.add(egui::Label::new(theme::muted("Some of these settings change only in Windows' own Settings, which needs its host to open.").size(12.0)).wrap());
        ui.add_space(10.0);
        if switch(ui, &mut st.shell.modern_apps, "Keep Settings, Calculator and the Store working").changed() {
            st.save_shell();
        }
        if st.shell.modern_apps {
            ui.add_space(6.0);
            ui.label(theme::muted("Starting it. The buttons work once it runs.").size(12.0));
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        }
    });
}

/// A switch row that may be locked (a policy, or Windows' own decision): dimmed and not
/// clickable then, with `why` as its detail.
pub fn setting(ui: &mut Ui, title: &str, detail: &str, on: bool, locked: Option<&str>) -> Option<bool> {
    let mut v = on;
    let detail = locked.unwrap_or(detail);
    let changed = ui.add_enabled_ui(locked.is_none(), |ui| theme::setting_row(ui, title, detail, &mut v).changed()).inner;
    (changed && v != on).then_some(v)
}
