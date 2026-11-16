//! Apps: the installed programs, with search, sort, and Uninstall / Modify. All reading and
//! running is done by a worker (`AppsPage`); the page draws what it last published, and only the
//! rows in view, so hundreds of programs stay smooth. Icons load as rows come into view.

use crate::page;
use crate::state::State;
use egui::{Align, CornerRadius, FontId, Layout, Sense, UiBuilder, Vec2};
use crate::pick::{self, Pick};
use shellman_core::icon_pack::{self, IconSettings};
use shellman_core::installed::app::{App, Kind};
use shellman_core::installed::list::{self, Sort};
use shellman_core::installed::worker::{AppsPage, Do, Page};
use shellman_core::thumbs::Thumbs;
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::draw_text;
use std::sync::Arc;

const ROW_H: f32 = 56.0;
const GAP: f32 = 4.0;
const ICON: f32 = 28.0;

/// The page's live state. The worker starts when the page is first drawn and ends when another
/// page is shown, so nothing runs for it otherwise.
pub struct AppsView {
    handle: Option<AppsPage>,
    page: Page,
    loaded: bool,
    query: String,
    sort: Sort,
    /// The app whose Uninstall is being confirmed in its row.
    confirm: Option<String>,
    /// What the list shows: indexes into the page's apps, kept until the list, the search or the
    /// sort changes.
    shown: Vec<usize>,
    shown_for: Option<(Arc<Vec<App>>, String, Sort)>,
    /// The app whose icon choices are showing in its row.
    icon_menu: Option<String>,
    /// The image chooser, and the app (by id) it was opened for.
    icon_pick: Option<(String, Pick)>,
    /// `icons.json` as last read, and the generation it was read at.
    icons: IconSettings,
    icons_generation: u64,
    note: Option<String>,
}

impl Default for AppsView {
    fn default() -> Self {
        Self {
            handle: None,
            page: Page::default(),
            loaded: false,
            query: String::new(),
            sort: Sort::Name,
            confirm: None,
            shown: Vec::new(),
            shown_for: None,
            icon_menu: None,
            icon_pick: None,
            icons: IconSettings::default(),
            icons_generation: 0,
            note: None,
        }
    }
}

/// The names an app's icon override is kept under: its id, and its program's path when the icon
/// comes from one (the dock knows apps by either).
fn icon_keys(app: &App) -> Vec<String> {
    let mut keys = vec![app.id.clone()];
    if let Kind::Packaged { full_name, .. } = &app.kind {
        // `Name_1.2.3.0_x64__publisherid` is the family `Name_publisherid`.
        let parts: Vec<&str> = full_name.split('_').collect();
        if let (Some(name), Some(publisher)) = (parts.first(), parts.last()) {
            keys.push(format!("{name}_{publisher}"));
        }
    }
    if let Some(exe) = app.icon.as_ref().filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"))) {
        keys.push(exe.to_string_lossy().to_string());
    }
    keys
}

impl AppsView {
    pub fn stop(&mut self) {
        if self.handle.take().is_some() {
            // The search and the sort are the user's, and stay for the next visit.
            *self = Self { query: std::mem::take(&mut self.query), sort: self.sort, ..Self::default() };
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        if self.handle.is_none() {
            let ctx = ctx.clone();
            self.handle = Some(AppsPage::start(Arc::new(move || ctx.request_repaint())));
        }
    }

    fn poll(&mut self) {
        if let Some(page) = self.handle.as_ref().and_then(AppsPage::take) {
            self.page = page;
            self.loaded = true;
        }
        let generation = icon_pack::generation();
        if generation != self.icons_generation {
            self.icons_generation = generation;
            self.icons = icon_pack::current_settings();
        }
        // The image chooser answered: that is the app's icon from now on.
        let answer = self.icon_pick.as_mut().and_then(|(id, pick)| pick.poll().map(|choice| (id.clone(), choice)));
        if let Some((id, choice)) = answer {
            self.icon_pick = None;
            if let (Some(image), Some(app)) = (choice, self.page.apps.iter().find(|a| a.id == id)) {
                for key in icon_keys(app) {
                    self.icons.set_override(&key, &image.to_string_lossy());
                }
                self.icons.save();
            }
        }
    }

    /// Show the app's own icon, the pack's, or the one the user chose.
    fn set_icon(&mut self, app: &App, choice: IconChoice) {
        for key in icon_keys(app) {
            match choice {
                IconChoice::Original => self.icons.use_original(&key),
                IconChoice::Pack => self.icons.use_pack(&key),
            }
        }
        self.icons.save();
    }

    /// Whether the app has an icon of the user's choosing, or is kept to its own.
    fn customised(&self, app: &App) -> bool {
        let keys: Vec<String> = icon_keys(app).iter().map(|k| icon_pack::settings::key(k)).collect();
        self.icons.override_for(&keys).is_some() || self.icons.is_skipped(&keys)
    }

    /// Work out the visible list again when what it depends on changed.
    fn refresh_shown(&mut self) {
        let same = self.shown_for.as_ref().is_some_and(|(a, q, s)| Arc::ptr_eq(a, &self.page.apps) && *q == self.query && *s == self.sort);
        if !same {
            self.shown = list::view(&self.page.apps, &self.query, self.sort);
            self.shown_for = Some((self.page.apps.clone(), self.query.clone(), self.sort));
        }
    }
}

/// What a press in a row asks for.
enum Press {
    Ask,
    Cancel,
    Uninstall,
    Modify,
    /// Open the icon choices in the row.
    Icon,
    CloseIcon,
    ChangeIcon,
    UseOriginal,
    UsePack,
}

#[derive(Clone, Copy)]
enum IconChoice {
    Original,
    Pack,
}

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Apps", "The programs installed on this PC. Uninstall one, or change it with its own installer.");
    st.apps.start(ui.ctx());
    st.apps.poll();
    if !st.apps.loaded {
        page::group(ui, "", |ui| {
            ui.label(theme::muted("Reading the installed apps..."));
        });
        return;
    }
    st.apps.refresh_shown();
    // The search and the sort on one island, the list on the next. Only the list's rows in view
    // are drawn, and the island it sits on is the part of it in view.
    page::group(ui, "", |ui| {
        controls(ui, &mut st.apps);
        for note in [&st.apps.page.notice, &st.apps.note].into_iter().flatten() {
            ui.add_space(8.0);
            ui.label(theme::muted(note).size(12.0));
        }
    });
    let apps = &mut st.apps;
    if apps.shown.is_empty() {
        page::group(ui, "", |ui| {
            ui.label(theme::muted(if apps.query.trim().is_empty() { "No apps found." } else { "No app matches the search." }));
        });
        return;
    }
    let thumbs = &mut st.thumbs;
    page::group(ui, "", |ui| list_rows(ui, apps, thumbs));
}

/// The search box, the sort choice and the count.
fn controls(ui: &mut egui::Ui, v: &mut AppsView) {
    let c = colors();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(10), c.control);
    ui.painter().text(egui::pos2(rect.left() + 18.0, rect.center().y), egui::Align2::CENTER_CENTER, glyph::SEARCH, icon_font(15.0), c.muted);
    let edit_rect = egui::Rect::from_min_max(egui::pos2(rect.left() + 36.0, rect.top()), egui::pos2(rect.right() - 10.0, rect.bottom()));
    let edit = egui::TextEdit::singleline(&mut v.query)
        .id(egui::Id::new("apps_search"))
        .font(FontId::proportional(14.0))
        .frame(false)
        .margin(egui::Margin::ZERO)
        .vertical_align(Align::Center)
        .hint_text(egui::RichText::new("Search apps").color(c.muted))
        .desired_width(edit_rect.width());
    let resp = ui.put(edit_rect, edit);
    shellman_ui::widgets::name_field(&resp, "Search apps", Some(&v.query));

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        page::segmented(ui, &mut v.sort, &[(Sort::Name, "Name"), (Sort::Size, "Size"), (Sort::Date, "Install date")]);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let shown = v.shown.len();
            let mut count = if shown == v.page.apps.len() { format!("{shown} apps") } else { format!("{shown} of {} apps", v.page.apps.len()) };
            if v.page.reading_packaged {
                count.push_str("  \u{b7}  looking for Store apps...");
            }
            ui.label(theme::muted(count).size(12.0));
        });
    });
}

/// Which rows of `n` (each `stride` tall, the first at `top`) a clip rect reaches.
fn visible_range(clip_top: f32, clip_bottom: f32, top: f32, stride: f32, n: usize) -> std::ops::Range<usize> {
    let first = (((clip_top - top) / stride).floor().max(0.0) as usize).min(n);
    let last = ((((clip_bottom - top) / stride).ceil().max(0.0) as usize) + 1).min(n);
    first..last.max(first)
}

/// The rows in view are drawn; the space of those above and below is left empty, so the page's
/// scroll bar is the whole list's.
fn list_rows(ui: &mut egui::Ui, v: &mut AppsView, thumbs: &mut Thumbs) {
    let stride = ROW_H + GAP;
    ui.spacing_mut().item_spacing.y = GAP;
    let n = v.shown.len();
    let top = ui.cursor().top();
    let clip = ui.clip_rect();
    let range = visible_range(clip.top(), clip.bottom(), top, stride, n);
    // Each allocation is followed by one gap, so a spacer of `k` rows is `k * stride - GAP` tall.
    let spacer = |ui: &mut egui::Ui, rows: usize| {
        if rows > 0 {
            ui.allocate_space(Vec2::new(1.0, rows as f32 * stride - GAP));
        }
    };
    spacer(ui, range.start);
    let mut press: Option<(usize, Press)> = None;
    for i in range.clone() {
        let app = &v.page.apps[v.shown[i]];
        let state = RowState {
            asking: v.confirm.as_deref() == Some(app.id.as_str()),
            busy: v.page.busy.contains(&app.id),
            progress: v.handle.as_ref().and_then(|h| h.progress(&app.id)),
            icon_menu: v.icon_menu.as_deref() == Some(app.id.as_str()),
            customised: v.customised(app),
        };
        if let Some(p) = row(ui, app, &state, thumbs) {
            press = Some((v.shown[i], p));
        }
    }
    spacer(ui, n - range.end);
    if let Some((index, p)) = press {
        let app = v.page.apps[index].clone();
        match p {
            Press::Ask => v.confirm = Some(app.id),
            Press::Cancel => v.confirm = None,
            Press::Uninstall => {
                v.confirm = None;
                if let Some(h) = &v.handle {
                    h.act(&app, Do::Uninstall);
                }
            }
            Press::Modify => {
                if let Some(h) = &v.handle {
                    h.act(&app, Do::Modify);
                }
            }
            Press::Icon => {
                v.confirm = None;
                v.note = None;
                v.icon_menu = Some(app.id);
            }
            Press::CloseIcon => v.icon_menu = None,
            Press::ChangeIcon => match Pick::start(shellman_pick::Mode::Open, "Choose an icon", pick::image_types()) {
                Ok(p) => {
                    v.icon_menu = None;
                    v.icon_pick = Some((app.id, p));
                }
                Err(e) => v.note = Some(e),
            },
            Press::UseOriginal => {
                v.icon_menu = None;
                v.set_icon(&app, IconChoice::Original);
            }
            Press::UsePack => {
                v.icon_menu = None;
                v.set_icon(&app, IconChoice::Pack);
            }
        }
    }
}

struct RowState {
    /// Uninstall was pressed and is waiting for a yes.
    asking: bool,
    /// An uninstall or a change is under way.
    busy: bool,
    /// How far a Store app's removal is.
    progress: Option<u32>,
    /// The icon choices are showing in place of the buttons.
    icon_menu: bool,
    /// The app has an icon of the user's choosing, or is kept to its own.
    customised: bool,
}

/// A small button for a row.
fn small_button(ui: &mut egui::Ui, text: &str, danger: bool) -> egui::Response {
    let c = colors();
    let (fill, ink) = if danger { (c.danger, egui::Color32::WHITE) } else { (c.control_hover, c.text) };
    let button = egui::Button::new(egui::RichText::new(text).size(13.0).color(ink))
        .fill(fill)
        .stroke(egui::Stroke::NONE)
        .corner_radius(CornerRadius::same(8))
        .min_size(Vec2::new(0.0, 30.0));
    ui.add(button)
}

/// One app: icon, name over details, and its buttons. Returns what was pressed.
fn row(ui: &mut egui::Ui, app: &App, state: &RowState, thumbs: &mut Thumbs) -> Option<Press> {
    let c = colors();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(10), c.control);

    let icon = egui::Rect::from_center_size(egui::pos2(rect.left() + 14.0 + ICON / 2.0, rect.center().y), Vec2::splat(ICON));
    // Shellman's own apps wear the accent, unless the icon pack has a tile for them.
    let own = app.icon.as_deref().filter(|p| shellman_ui::app_icon::own_from_pack(p).is_none()).and_then(shellman_ui::app_icon::kind_for_exe);
    let image = match own {
        Some(kind) => Some(shellman_ui::app_icon::kind_texture(ui.ctx(), kind, false)),
        None => thumbs.get_app(app.icon.as_deref(), &app.id, &app.name, false).map(|t| (t.id(), t.size_vec2())),
    };
    match image {
        Some((id, size)) => {
            let scale = (ICON / size.x).min(ICON / size.y);
            egui::Image::from_texture((id, size)).paint_at(ui, egui::Rect::from_center_size(icon.center(), size * scale));
        }
        None => {
            ui.painter().text(icon.center(), egui::Align2::CENTER_CENTER, glyph::APP, icon_font(18.0), c.muted);
        }
    }

    // What the right-hand side holds decides how much room the text has.
    let plain = !state.busy && !state.asking && !state.icon_menu;
    let modify = app.modify().is_some() && plain;
    let uninstall = app.uninstall().is_some() && plain;
    let icon_button = plain;
    let controls_w = if state.asking {
        170.0
    } else if state.icon_menu {
        // Cancel, Use original, Change icon, and Use pack when there is something to undo.
        338.0 + if state.customised { 104.0 } else { 0.0 }
    } else {
        (if modify { 76.0 } else { 0.0 }) + (if uninstall { 96.0 } else { 0.0 }) + (if icon_button { 62.0 } else { 0.0 })
    };
    let text_left = icon.right() + 12.0;
    let text_right = (rect.right() - controls_w - 16.0).max(text_left + 40.0);
    let line = |top: f32, bottom: f32| egui::Rect::from_min_max(egui::pos2(text_left, top), egui::pos2(text_right, bottom));

    let (title, sub) = if state.asking {
        (format!("Uninstall {}?", app.name), "This starts the program's own uninstaller.".to_string())
    } else if state.icon_menu {
        (format!("Icon for {}", app.name), "Choose an image, or show the app's own icon.".to_string())
    } else if state.busy {
        let what = match state.progress {
            Some(p) => format!("Removing... {p}%"),
            None => "Waiting for the program to finish...".to_string(),
        };
        (app.name.clone(), what)
    } else {
        (app.name.clone(), app.detail())
    };
    draw_text(ui, line(rect.top() + 7.0, rect.center().y), &title, FontId::proportional(14.0), c.text, false);
    draw_text(ui, line(rect.center().y, rect.bottom() - 7.0), &sub, FontId::proportional(11.5), c.muted, false);

    if controls_w == 0.0 {
        return None;
    }
    let area = egui::Rect::from_min_max(egui::pos2(rect.right() - controls_w - 8.0, rect.top()), egui::pos2(rect.right() - 8.0, rect.bottom()));
    let mut child = ui.new_child(UiBuilder::new().max_rect(area).layout(Layout::right_to_left(Align::Center)));
    child.spacing_mut().item_spacing.x = 8.0;
    // Right to left, so the first one added sits furthest right.
    if state.asking {
        if small_button(&mut child, "Cancel", false).clicked() {
            return Some(Press::Cancel);
        }
        if small_button(&mut child, "Uninstall", true).clicked() {
            return Some(Press::Uninstall);
        }
        return None;
    }
    if state.icon_menu {
        if small_button(&mut child, "Cancel", false).clicked() {
            return Some(Press::CloseIcon);
        }
        if small_button(&mut child, "Use original", false).clicked() {
            return Some(Press::UseOriginal);
        }
        if state.customised && small_button(&mut child, "Use pack", false).clicked() {
            return Some(Press::UsePack);
        }
        if small_button(&mut child, "Change icon...", false).clicked() {
            return Some(Press::ChangeIcon);
        }
        return None;
    }
    if uninstall && small_button(&mut child, "Uninstall", false).clicked() {
        return Some(Press::Ask);
    }
    if modify && small_button(&mut child, "Modify", false).clicked() {
        return Some(Press::Modify);
    }
    if icon_button && small_button(&mut child, "Icon", false).clicked() {
        return Some(Press::Icon);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_rows_in_view_are_drawn() {
        // 100 rows of 60 from y=100; the clip shows y 400..900.
        let r = visible_range(400.0, 900.0, 100.0, 60.0, 100);
        assert_eq!(r, 5..15);
        assert!(r.len() <= 12);
    }

    #[test]
    fn the_range_is_kept_inside_the_list() {
        assert_eq!(visible_range(0.0, 5000.0, 100.0, 60.0, 10), 0..10);
        assert_eq!(visible_range(10_000.0, 10_500.0, 0.0, 60.0, 10), 10..10);
        assert_eq!(visible_range(0.0, 50.0, 200.0, 60.0, 10), 0..1);
        assert_eq!(visible_range(0.0, 100.0, 0.0, 60.0, 0), 0..0);
    }
}
