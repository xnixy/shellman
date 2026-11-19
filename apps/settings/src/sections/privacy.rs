//! Privacy & security: Windows Security, Windows' privacy switches, search permissions, and what
//! each app may use. The per-user switches Windows reads where it uses them (advertising ID,
//! language list, app-launch tracking, suggested content, search) change here at once, through
//! `shellman_core::system::privacy`. App permissions are only shown: Windows' capability access
//! service keeps them and takes changes only from its own Settings, so each opens there, as do
//! the settings an administrator or a policy holds (diagnostic data, activity history).

use super::live::Live;
use super::winlink;
use crate::page;
use crate::state::State;
use egui::{Align2, RichText, Sense, Ui, Vec2};
use shellman_core::registry::Real;
use shellman_core::system::privacy::{self as p, Consent, Permission, SafeSearch, Snapshot};
use shellman_ui::theme::{self, colors, icon_font};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// What the page shows: Windows' values and the names of the apps in them.
#[derive(Clone, Default)]
pub struct Data {
    snap: Snapshot,
    names: HashMap<String, String>,
}

/// A change the worker makes.
pub enum Job {
    Toggle(&'static str, bool),
    SafeSearch(SafeSearch),
}

/// The page's live state; its worker runs while the page shows.
#[derive(Default)]
pub struct PrivacyView {
    live: Live<Data, Job>,
    /// The permission whose details are open, by its key.
    open: Option<&'static str>,
}

impl PrivacyView {
    pub fn stop(&mut self) {
        *self = Self::default();
    }
}

/// A reader that remembers the apps' names (asking the package manager is the slow part).
fn reader() -> Box<dyn FnMut() -> Data + Send> {
    let mut names: HashMap<String, String> = HashMap::new();
    Box::new(move || {
        let snap = p::read(&Real);
        let missing: Vec<String> = snap.permissions.iter().flat_map(|p| p.apps.iter().map(|a| a.family.clone())).filter(|f| !names.contains_key(f)).collect();
        if !missing.is_empty() {
            let found = p::package_names(&missing);
            for f in missing {
                let name = found.get(&f).cloned().unwrap_or_else(|| p::family_label(&f));
                names.insert(f, name);
            }
        }
        Data { snap, names: names.clone() }
    })
}

fn run(job: Job) -> Result<(), String> {
    match job {
        Job::Toggle(id, on) => p::write_toggle(&Real, p::find_toggle(id).ok_or("unknown switch")?, on),
        Job::SafeSearch(s) => p::write_safe_search(&Real, s),
    }
}

pub fn show(ui: &mut Ui, st: &mut State) {
    page::heading(ui, "Privacy & security", "What Windows and apps may use and keep. Changes apply at once.");
    st.privacy.live.start(ui.ctx(), reader(), Box::new(run));
    st.privacy.live.poll();
    st.privacy.live.refresh_every(ui.ctx(), Duration::from_secs(5));
    let Some(d) = st.privacy.live.shown.clone() else {
        page::group(ui, "", |ui| {
            page::row_with(ui, |ui| {
                ui.label(theme::muted("Reading Windows' privacy settings..."));
            });
        });
        return;
    };
    let host = winlink::host();
    winlink::host_group(ui, st, host);
    let v = &mut st.privacy;
    if let Some(n) = v.live.notice.clone() {
        page::group(ui, "", |ui| {
            page::row_with(ui, |ui| {
                ui.add(egui::Label::new(theme::muted(n).size(12.5)).wrap());
            });
        });
    }
    page::group(ui, "Security", |ui| {
        page::row(ui, "Windows Security", "Virus and threat protection, the firewall and device security are kept by the Windows Security app.", |ui| {
            winlink::button(ui, "windowsdefender:", host, "Open Windows Security", None);
        });
        winlink::row(ui, "Find my device", "", "ms-settings:findmydevice", host);
        winlink::row(ui, "For developers", "", "ms-settings:developers", host);
    });
    page::group(ui, "General", |ui| {
        toggles(ui, v, &d.snap, p::GENERAL);
        winlink::row(ui, "More general settings", "", "ms-settings:privacy-general", host);
    });
    page::group(ui, "Speech, inking and typing", |ui| {
        let speech = if d.snap.managed.online_speech { "On" } else { "Off" };
        winlink::row(ui, "Online speech recognition", speech, "ms-settings:privacy-speech", host);
        winlink::row(ui, "Inking and typing personalisation", "", "ms-settings:privacy-speechtyping", host);
    });
    page::group(ui, "Diagnostics and feedback", |ui| {
        let m = &d.snap.managed;
        let diag = match &m.diagnostics {
            Some((v, true)) => format!("{v}, set by your organisation"),
            Some((v, false)) => v.clone(),
            None => "Windows did not say".to_owned(),
        };
        winlink::row(ui, "Diagnostic data", &diag, "ms-settings:privacy-feedback", host);
        let fb = if m.feedback.1 { format!("{}, set by your organisation", m.feedback.0) } else { m.feedback.0.clone() };
        winlink::row(ui, "Feedback frequency", &fb, "ms-settings:privacy-feedback", host);
    });
    page::group(ui, "Activity history", |ui| {
        let value = match d.snap.managed.activity_policy {
            Some(false) => "Off, set by your organisation",
            _ => "Kept as Windows' Settings says",
        };
        winlink::row(ui, "Store my activity history on this device", value, "ms-settings:privacy-activityhistory", host);
    });
    page::group(ui, "Search permissions", |ui| {
        if let Some(ss) = d.snap.safe_search {
            let opts: Vec<(SafeSearch, &str)> = SafeSearch::ALL.iter().map(|s| (*s, s.label())).collect();
            let mut n = ss;
            if page::row(ui, "SafeSearch for web results", "", |ui| page::segmented(ui, &mut n, &opts)) {
                v.live.change(Job::SafeSearch(n), move |mut d| {
                    d.snap.safe_search = Some(n);
                    d
                });
            }
        }
        // Cloud content search, then history and highlights.
        toggles(ui, v, &d.snap, p::SEARCH_TOGGLES);
        winlink::row(ui, "Clear this device's search history", "", "ms-settings:search-permissions", host);
    });
    permissions(ui, v, &d, host);
}

/// Switch rows for `list`, each locked when a policy holds it.
fn toggles(ui: &mut Ui, v: &mut PrivacyView, snap: &Snapshot, list: &'static [p::Toggle]) {
    for t in list {
        let (on, locked) = snap.toggle(t.id).unwrap_or((t.default_on, false));
        let lock = locked.then_some("Set by your organisation.");
        if let Some(n) = winlink::setting(ui, t.title, t.detail, on, lock) {
            let id = t.id;
            v.live.change(Job::Toggle(id, n), move |mut d| {
                if let Some(e) = d.snap.toggles.iter_mut().find(|e| e.0 == id) {
                    e.1 = n;
                }
                d
            });
        }
    }
}

fn permissions(ui: &mut Ui, v: &mut PrivacyView, d: &Data, host: bool) {
    page::group(ui, "App permissions", |ui| {
        page::row_with(ui, |ui| {
            ui.add(egui::Label::new(theme::muted("Windows keeps these itself and takes changes only in its own Settings. Open one to see which apps have it.").size(12.5)).wrap());
        });
        for perm in &d.snap.permissions {
            let open = v.open == Some(perm.cap.key);
            let clicked = page::item(ui, false, |ui| disclosure(ui, perm.cap.name, &perm.summary(), open).clicked());
            if clicked {
                v.open = if open { None } else { Some(perm.cap.key) };
            }
            if open {
                page::row_with(ui, |ui| details(ui, perm, &d.names, host));
            }
        }
    });
}

/// A permission's row: its name, a summary, and a chevron. Says to a screen reader what it is
/// and whether it is open. The hover wash is the list item's.
fn disclosure(ui: &mut Ui, title: &str, summary: &str, open: bool) -> egui::Response {
    let c = colors();
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 22.0), Sense::click());
    let name = format!("{title}, {summary}");
    theme::speak_selected(&resp, egui::WidgetType::CollapsingHeader, open, || name.clone());
    theme::focus_ring(ui, &resp, rect.expand2(Vec2::new(6.0, 4.0)), theme::shape::TILE);
    let chevron = if open { '\u{E70E}' } else { '\u{E70D}' };
    let right = rect.right();
    ui.painter().text(egui::pos2(right, rect.center().y), Align2::RIGHT_CENTER, chevron, icon_font(11.0), c.muted);
    // The summary right-aligned before the chevron, the name left; the name keeps what is left.
    let summary_g = ui.painter().layout_no_wrap(summary.to_owned(), egui::FontId::proportional(12.5), c.muted);
    let sx = right - 24.0 - summary_g.size().x;
    let title_w = (sx - rect.left() - 24.0).max(40.0);
    let title_g = ui.painter().layout(title.to_owned(), egui::FontId::proportional(13.5), c.text, title_w);
    ui.painter().galley(egui::pos2(rect.left(), rect.center().y - title_g.size().y / 2.0), title_g, c.text);
    if sx > rect.left() + 120.0 {
        ui.painter().galley(egui::pos2(sx, rect.center().y - summary_g.size().y / 2.0), summary_g, c.muted);
    }
    resp
}

fn consent_label(perm: &Permission, a: &p::AppConsent) -> &'static str {
    if perm.app_allowed(a) {
        "Allowed"
    } else if a.consent == Consent::Prompt && perm.apps_allowed() {
        "Asks first"
    } else {
        "Not allowed"
    }
}

/// What an open permission shows: the switches' state, the apps, and the button.
fn details(ui: &mut Ui, perm: &Permission, names: &HashMap<String, String>, host: bool) {
    let c = colors();
    egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 0, top: 0, bottom: 0 }).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        let line = |ui: &mut Ui, k: &str, val: &str| {
            theme::trailing(
                ui,
                |ui| ui.label(RichText::new(val).size(12.5).color(c.muted)),
                |ui| {
                    ui.add(egui::Label::new(RichText::new(k).size(12.5).color(c.text)).wrap());
                },
            );
        };
        let state = |on: bool| if on { "On" } else { "Off" };
        line(ui, "On this device", state(perm.device));
        line(ui, &format!("Let apps use your {}", perm.cap.name.to_lowercase()), state(perm.user));
        if let Some(desk) = perm.desktop {
            line(ui, "Let desktop apps use it", state(desk));
        }
        if !perm.apps.is_empty() {
            page::label(ui, "Store apps");
            for a in &perm.apps {
                let name = names.get(&a.family).cloned().unwrap_or_else(|| p::family_label(&a.family));
                line(ui, &name, consent_label(perm, a));
            }
        }
        if !perm.desktop_apps.is_empty() {
            page::label(ui, "Desktop apps that used it");
            for d in &perm.desktop_apps {
                let file = Path::new(&d.path).file_name().map_or(d.path.clone(), |f| f.to_string_lossy().into_owned());
                let when = d.last_used.and_then(p::filetime_date).map_or_else(String::new, |w| format!("Last used {w}"));
                theme::trailing(
                    ui,
                    |ui| ui.label(RichText::new(&when).size(12.0).color(c.muted)),
                    |ui| {
                        ui.add(egui::Label::new(RichText::new(&file).size(12.5).color(c.text)).truncate()).on_hover_text(&d.path);
                    },
                );
            }
        }
        ui.add_space(4.0);
        winlink::button(ui, perm.cap.uri, host, "Open in Windows Settings", Some(&format!("Open {} permissions in Windows Settings", perm.cap.name)));
    });
}
