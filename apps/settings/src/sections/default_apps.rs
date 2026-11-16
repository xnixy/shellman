//! Default apps: which app opens each common file type and link, and a way to change it.
//!
//! Windows takes a new default only from its own Settings (see
//! `shellman_core::system::default_apps_change` for what was tried and measured). So "Change"
//! lists the apps that can open the type, on an island under its row, and picking one opens
//! Windows' Settings on that app's page, where the person confirms. The page watches the
//! choice in the registry and shows the new default as soon as Windows writes it. Without the
//! host for Windows' own apps Settings cannot open, and the page says so and offers the switch.

use crate::page;
use crate::state::State;
use egui::{CornerRadius, FontId, Sense, Vec2};
use shellman_core::system::default_apps::{self as da, Entry};
use shellman_core::apps_index::{App, AppIndex};
use shellman_core::system::default_apps_change::{self as dc, Candidate};
use shellman_core::thumbs::Thumbs;
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::{draw_text, switch};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const ROW_H: f32 = 56.0;
const APP_H: f32 = 48.0;
const ICON: f32 = 28.0;

type Shared<T> = Arc<Mutex<Option<T>>>;

/// The apps for one type and which of them is the default now.
type Choices = (Vec<Candidate>, Option<usize>);

/// "Change" open on one row.
struct Chooser {
    /// The row's index in `da::ITEMS`.
    item: usize,
    list: Shared<Choices>,
    /// The app whose page in Windows' Settings was opened last, by name.
    sent: Option<String>,
    /// Whether it has been scrolled into view.
    shown: bool,
}

/// The page's live state; it reads again each time the page is shown, and again whenever
/// Windows writes a new choice.
#[derive(Default)]
pub struct DefaultAppsView {
    entries: Shared<Vec<Entry>>,
    started: bool,
    changed: Arc<AtomicBool>,
    watch: Option<dc::Watch>,
    chooser: Option<Chooser>,
    /// The launcher's list of apps, for their icons (the Apps folder knows packaged apps' images).
    index: AppIndex,
}

impl DefaultAppsView {
    pub fn stop(&mut self) {
        *self = Self::default();
    }

    fn start(&mut self, ctx: &egui::Context) {
        if !self.started {
            self.started = true;
            let repaint = ctx.clone();
            self.index.refresh_if_older(std::time::Duration::from_secs(60), move || repaint.request_repaint());
            self.read(ctx);
            // SHELLMAN_DEFAULT_APPS_OPEN=.pdf: start with that row's chooser open (for screenshots).
            if let Ok(key) = std::env::var("SHELLMAN_DEFAULT_APPS_OPEN") {
                if let Some(i) = da::ITEMS.iter().position(|it| it.0 == key) {
                    self.open_chooser(ctx, i);
                }
            }
            let (flag, ctx) = (self.changed.clone(), ctx.clone());
            self.watch = Some(dc::Watch::start(move || {
                flag.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            }));
        }
        if self.changed.swap(false, Ordering::Relaxed) {
            self.read(ctx);
            if let Some(ch) = &self.chooser {
                load(ctx, ch.item, &ch.list);
            }
        }
    }

    fn read(&self, ctx: &egui::Context) {
        let (shared, ctx) = (self.entries.clone(), ctx.clone());
        std::thread::spawn(move || {
            let v = da::read();
            if let Ok(mut s) = shared.lock() {
                *s = Some(v);
            }
            ctx.request_repaint();
        });
    }

    fn open_chooser(&mut self, ctx: &egui::Context, item: usize) {
        let list: Shared<Choices> = Arc::default();
        load(ctx, item, &list);
        self.chooser = Some(Chooser { item, list, sent: None, shown: false });
    }
}

/// Ask Windows, on a worker, which apps can open item `item` and which one does now.
fn load(ctx: &egui::Context, item: usize, out: &Shared<Choices>) {
    let (out, ctx) = (out.clone(), ctx.clone());
    std::thread::spawn(move || {
        let (key, kind, _) = da::ITEMS[item];
        let list = dc::candidates(key, kind);
        let now = da::read().into_iter().nth(item);
        let current = dc::current_index(
            &list,
            dc::user_choice(key, kind).as_deref(),
            now.as_ref().and_then(|e| e.exe.as_deref()),
            now.as_ref().and_then(|e| e.app.as_deref()),
        );
        if let Ok(mut s) = out.lock() {
            *s = Some((list, current));
        }
        ctx.request_repaint();
    });
}

use super::winlink::open_link;

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Default apps", "Which app opens each kind of file and link.");
    st.default_apps.start(ui.ctx());
    let apps = st.default_apps.index.apps();
    let entries = st.default_apps.entries.lock().ok().and_then(|g| g.clone());
    let Some(entries) = entries else {
        page::group(ui, "", |ui| {
            ui.label(theme::muted("Asking Windows..."));
        });
        return;
    };
    // Settings is the only thing Windows takes a new default from, and it needs the host.
    // SHELLMAN_PRETEND_HOST=on|off: look at the page as it is with the host on or off, without
    // changing it (for screenshots).
    let host = match std::env::var("SHELLMAN_PRETEND_HOST").as_deref() {
        Ok("on") => true,
        Ok("off") => false,
        _ => shellman_core::app_host::modern_apps_available(),
    };
    if !host {
        st.default_apps.chooser = None;
        page::group(ui, "To change a default", |ui| {
            ui.label(
                theme::muted("Windows only takes a new default app from its own Settings, and Settings cannot open while its host is off.")
                    .size(12.0),
            );
            ui.add_space(10.0);
            if switch(ui, &mut st.shell.modern_apps, "Keep Settings, Calculator and the Store working").changed() {
                st.save_shell();
            }
            if st.shell.modern_apps {
                ui.add_space(6.0);
                ui.label(theme::muted("Starting it. The Change buttons appear when it runs.").size(12.0));
                ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
            }
        });
    }

    let open_at = st.default_apps.chooser.as_ref().map(|c| c.item);
    let mut pressed = None;
    // The list is split under the row being changed, so its chooser is an island of its own
    // right there.
    let split = open_at.map_or(entries.len(), |i| (i + 1).min(entries.len()));
    let thumbs = &mut st.thumbs;
    page::group(ui, "File types and links", |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        for (i, e) in entries[..split].iter().enumerate() {
            if row(ui, e, &apps, thumbs, host, open_at == Some(i)) {
                pressed = Some(i);
            }
        }
    });
    if let Some(i) = open_at {
        let mut close = false;
        let (view, thumbs) = (&mut st.default_apps, &mut st.thumbs);
        if let Some(ch) = view.chooser.as_mut() {
            let (key, kind, title) = da::ITEMS[i];
            page::group(ui, &format!("{title}: choose the app"), |ui| {
                close = chooser(ui, ch, key, kind, &apps, thumbs);
            });
        }
        if close {
            view.chooser = None;
        }
    }
    if split < entries.len() {
        page::group(ui, "", |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for (i, e) in entries.iter().enumerate().skip(split) {
                if row(ui, e, &apps, &mut st.thumbs, host, false) {
                    pressed = Some(i);
                }
            }
        });
    }
    if let Some(i) = pressed {
        let view = &mut st.default_apps;
        if open_at == Some(i) {
            view.chooser = None;
        } else {
            view.open_chooser(ui.ctx(), i);
        }
    }
}

/// A small button drawn at `rect` inside a row (the row has already taken its space), looking
/// like the Apps page's row buttons.
fn small_button(ui: &mut egui::Ui, text: &str, rect: egui::Rect) -> egui::Response {
    let c = colors();
    let resp = ui.interact(rect, ui.id().with(("row-button", rect.min.y as i32)), Sense::click());
    // The switch track's grey: unlike a faint tint of the text, it shows on light rows too.
    let fill = if resp.hovered() { theme::with_alpha(c.accent, 70) } else { c.track };
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, FontId::proportional(13.0), c.text);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Where to take an app's icon from: its entry in the Apps folder when the launcher lists one of
/// that name (packaged apps such as Photos have no program to ask, and the folder entry is what the
/// launcher draws, through the icon pack), else the program or logo Windows named.
fn icon_source(apps: &[App], name: Option<&str>, fallback: Option<&Path>) -> Option<PathBuf> {
    let found = name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .and_then(|n| apps.iter().find(|a| !a.direct && a.is_listed() && a.name.trim().eq_ignore_ascii_case(n)));
    match found {
        Some(app) => Some(PathBuf::from(app.shell_path())),
        None => fallback.map(Path::to_path_buf),
    }
}

/// The app's icon centred at `at`: its program's or logo's image, or a plain glyph.
fn icon(ui: &mut egui::Ui, at: egui::Pos2, source: Option<&Path>, thumbs: &mut Thumbs) {
    let c = colors();
    match source.and_then(|p| shellman_ui::app_icon::get(ui.ctx(), p, true, thumbs)) {
        Some((id, size)) => {
            let scale = (ICON / size.x).min(ICON / size.y);
            egui::Image::from_texture((id, size)).paint_at(ui, egui::Rect::from_center_size(at, size * scale));
        }
        None => {
            ui.painter().text(at, egui::Align2::CENTER_CENTER, glyph::APP, icon_font(18.0), c.muted);
        }
    }
}

/// What a row's right side holds: the "Change" button (or "Close" while its chooser is open)
/// when Windows' Settings can open, else nothing to press.
pub fn action_label(host: bool, open: bool) -> Option<&'static str> {
    match (host, open) {
        (false, _) => None,
        (true, false) => Some("Change"),
        (true, true) => Some("Close"),
    }
}

/// One type: icon, name over its app, the key as typed, and its button. True when pressed.
fn row(ui: &mut egui::Ui, e: &Entry, apps: &[App], thumbs: &mut Thumbs, host: bool, open: bool) -> bool {
    let c = colors();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
    let fill = if open { c.control_hover } else { c.control };
    ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
    let at = egui::pos2(rect.left() + 14.0 + ICON / 2.0, rect.center().y);
    icon(ui, at, icon_source(apps, e.app.as_deref(), e.exe.as_deref()).as_deref(), thumbs);
    let action = action_label(host, open);
    let button_w = if action.is_some() { 84.0 } else { 0.0 };
    let key_w = 90.0;
    let text_left = at.x + ICON / 2.0 + 12.0;
    let key_right = rect.right() - 14.0 - if button_w > 0.0 { button_w + 12.0 } else { 0.0 };
    let text_right = key_right - key_w - 8.0;
    let line = |top: f32, bottom: f32| egui::Rect::from_min_max(egui::pos2(text_left, top), egui::pos2(text_right, bottom));
    draw_text(ui, line(rect.top() + 7.0, rect.center().y), e.title, FontId::proportional(14.0), c.text, false);
    let (app, color) = match &e.app {
        Some(a) => (a.as_str(), c.muted),
        None => (da::NO_APP, theme::with_alpha(c.muted, 170)),
    };
    draw_text(ui, line(rect.center().y, rect.bottom() - 7.0), app, FontId::proportional(12.0), color, false);
    let key = egui::Rect::from_min_max(egui::pos2(key_right - key_w, rect.top()), egui::pos2(key_right, rect.bottom()));
    draw_text(ui, key, &da::key_label(e.key, e.kind), FontId::proportional(12.0), c.muted, true);
    let Some(label) = action else { return false };
    let b = egui::Rect::from_center_size(egui::pos2(rect.right() - 14.0 - button_w / 2.0, rect.center().y), Vec2::new(button_w, 30.0));
    small_button(ui, label, b).clicked()
}

/// How the chooser names the type in a sentence: ".pdf files", "https links".
pub fn type_words(key: &str, kind: da::Kind) -> String {
    match kind {
        da::Kind::Extension => format!("{key} files"),
        da::Kind::Protocol => format!("{key} links"),
    }
}

/// What the chooser says over its list.
pub fn chooser_note(what: &str, sent: Option<&str>) -> String {
    match sent {
        Some(app) => format!("Windows Settings is open at {app}. Choose it there for {what}; this page shows the change as soon as Windows makes it."),
        None => format!("Windows only takes a new default from its own Settings. Pick an app and Settings opens on its page, where you choose it for {what}."),
    }
}

/// The apps for one type. Picking one opens its page in Windows' Settings. True to close.
fn chooser(ui: &mut egui::Ui, ch: &mut Chooser, key: &str, kind: da::Kind, apps: &[App], thumbs: &mut Thumbs) -> bool {
    let c = colors();
    let typed = type_words(key, kind);
    let loaded = ch.list.lock().ok().and_then(|g| g.clone());
    let Some((list, current)) = loaded else {
        ui.label(theme::muted("Asking Windows...").size(12.0));
        return false;
    };
    ui.label(theme::muted(chooser_note(&typed, ch.sent.as_deref())).size(12.0));
    ui.add_space(10.0);
    ui.spacing_mut().item_spacing.y = 4.0;
    if list.is_empty() {
        ui.label(theme::muted("No app says it opens this type. Windows Settings may still offer some.").size(12.0));
    }
    for (i, app) in list.iter().enumerate() {
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), APP_H), Sense::click());
        let fill = if resp.hovered() { c.control_hover } else { c.control };
        ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
        let at = egui::pos2(rect.left() + 14.0 + ICON / 2.0, rect.center().y);
        icon(ui, at, icon_source(apps, Some(&app.name), app.icon.as_deref().or(app.exe.as_deref())).as_deref(), thumbs);
        let right = rect.right() - 150.0;
        let name = egui::Rect::from_min_max(egui::pos2(at.x + ICON / 2.0 + 12.0, rect.top()), egui::pos2(right, rect.bottom()));
        draw_text(ui, name, &app.name, FontId::proportional(14.0), c.text, false);
        let side = egui::Rect::from_min_max(egui::pos2(right, rect.top()), egui::pos2(rect.right() - 16.0, rect.bottom()));
        let (tag, color) = if current == Some(i) { ("Current", c.accent_text) } else { ("Open in Settings", c.muted) };
        draw_text(ui, side, tag, FontId::proportional(12.0), color, true);
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            open_link(app.link.clone());
            ch.sent = Some(app.name.clone());
        }
    }
    ui.add_space(10.0);
    let mut close = false;
    ui.horizontal(|ui| {
        if ui.button("All default apps in Windows Settings").clicked() {
            open_link(dc::SETTINGS_PAGE.to_owned());
        }
        if ui.button("Close").clicked() {
            close = true;
        }
    });
    if !ch.shown {
        // Bring the whole island into view the first time it is drawn with its list.
        ch.shown = true;
        ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
    }
    close
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_packaged_app_takes_its_icon_from_its_apps_folder_entry() {
        let apps = vec![
            App::new("Photos", "Microsoft.Windows.Photos_8wekyb3d8bbwe!App"),
            App { direct: true, ..App::new("Notepad", r"C:\sm\Notepad.lnk") },
        ];
        let photos = icon_source(&apps, Some("photos "), None).unwrap();
        assert_eq!(photos, PathBuf::from(r"shell:AppsFolder\Microsoft.Windows.Photos_8wekyb3d8bbwe!App"));
        // The folder entry wins over the program Windows named; without one, that program is used.
        assert_eq!(icon_source(&apps, Some("Photos"), Some(Path::new(r"C:\x\p.exe"))), Some(photos));
        assert_eq!(icon_source(&apps, Some("Other"), Some(Path::new(r"C:\x\p.exe"))), Some(PathBuf::from(r"C:\x\p.exe")));
        // A shortcut the folder lacks is not a folder entry.
        assert_eq!(icon_source(&apps, Some("Notepad"), None), None);
        assert_eq!(icon_source(&apps, None, None), None);
    }

    #[test]
    fn rows_offer_change_only_when_settings_can_open() {
        assert_eq!(action_label(false, false), None);
        assert_eq!(action_label(false, true), None);
        assert_eq!(action_label(true, false), Some("Change"));
        assert_eq!(action_label(true, true), Some("Close"));
    }

    #[test]
    fn the_note_says_what_happens_next() {
        assert!(chooser_note(".pdf files", None).contains("Pick an app"));
        let sent = chooser_note("https links", Some("Firefox"));
        assert!(sent.contains("open at Firefox") && sent.contains("https links"));
        assert_eq!(type_words(".pdf", da::Kind::Extension), ".pdf files");
        assert_eq!(type_words("https", da::Kind::Protocol), "https links");
    }
}
