//! The tabs on their pill (drawn by `shellman_ui::tab_strip`, the look Files' tabs have), the +
//! button and the profile menu beside it.

use shellman_ui::chips::round_icon;
use shellman_ui::glass::islands::shows_as;
use shellman_ui::menu::{self, Icon};
use shellman_ui::tab_strip::{self as ts, Mark, Tab};
use shellman_ui::theme::{colors, glyph};
use egui::{Id, Order, Rect, Sense, Stroke};

/// The terminal glyph (Segoe Fluent's command prompt).
const TERMINAL: char = '\u{E756}';
const MENU_W: f32 = 260.0;

#[derive(Default)]
pub struct StripState {
    scroll: f32,
    /// The active tab, count and row width last frame: a change brings the active tab into
    /// view (find opening narrows the row).
    seen: (usize, usize, u32),
    /// The tab being dragged (by its id).
    dragging: Option<u64>,
    /// The profile menu, open under this button.
    menu: Option<Rect>,
    menu_opened: u64,
}

impl StripState {
    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    pub fn close_menu(&mut self) {
        self.menu = None;
    }
}

enum Do {
    Select(usize),
    Close(usize),
    Move(usize, usize),
}

impl super::TerminalApp {
    /// Draw the tabs at the left of `row`; returns the pill they are on.
    pub(super) fn tab_strip(&mut self, ui: &mut egui::Ui, row: Rect) -> Rect {
        let n = self.tabs.len();
        let active = self.tabs.active();
        let mut s = ts::strip(n, 2, row.width(), row.height(), self.strip.scroll);
        let seen = (active, n, row.width().round() as u32);
        if self.strip.seen != seen {
            self.strip.seen = seen;
            s.offset = ts::reveal(&s, active);
        }
        let pill = Rect::from_min_size(row.min, egui::vec2(s.pill_w.min(row.width()), row.height()));
        let view = ts::view(&s, pill);
        if s.max_offset > 0.0 && ui.rect_contains_pointer(pill) {
            let d = ui.input(|i| i.smooth_scroll_delta);
            let step = if d.x.abs() > d.y.abs() { d.x } else { d.y };
            if step != 0.0 {
                s.offset = (s.offset - step).clamp(0.0, s.max_offset);
            }
        }
        self.strip.scroll = s.offset;
        let clip = ui.clip_rect().intersect(view);
        let mut todo = None;
        let pointer_x = ui.input(|i| i.pointer.interact_pos()).map(|p| p.x - view.left());
        for i in 0..n {
            let rect = ts::tab_rect(&s, view, i);
            if rect.right() < clip.left() || rect.left() > clip.right() {
                continue;
            }
            let tab = self.tabs.get(i).unwrap();
            let title = tab_title(&tab.pane().session);
            let shown = i == active;
            // A tab not shown is marked when any of its panes had output or a bell.
            let (activity, bell) = if shown { (false, false) } else { tab.panes.iter().map(|p| p.session.marks()).fold((false, false), |a, m| (a.0 || m.0, a.1 || m.1)) };
            let mark = if bell { Mark::Bell } else if activity { Mark::Activity } else { Mark::None };
            let id = tab.id;
            let r = ts::tab(ui, rect, clip, &Tab { id: Id::new(("term-tab", id)), title: &title, icon: Some(TERMINAL), active: shown, closable: true, mark });
            if r.close || r.resp.middle_clicked() {
                todo = Some(Do::Close(i));
            } else if r.resp.drag_started() {
                self.strip.dragging = Some(id);
                todo = Some(Do::Select(i));
            } else if r.resp.clicked() {
                todo = Some(Do::Select(i));
            }
            shellman_ui::hover::label(ui, &r.resp, &title);
        }
        // A dragged tab moves to the place under the pointer as it goes.
        if let Some(id) = self.strip.dragging {
            if ui.input(|i| !i.pointer.primary_down()) {
                self.strip.dragging = None;
            } else if let (Some(x), Some(from)) = (pointer_x, self.tabs.iter().position(|t| t.id == id)) {
                let to = ts::index_at(&s, n, x);
                if to != from {
                    todo = Some(Do::Move(from, to));
                }
            }
        }
        ts::edges(ui, &s, view);
        let plus = round_icon(ui, ts::button_rect(&s, pill, 0), Id::new("new-tab"), glyph::ADD, true, false, "New tab");
        shellman_ui::hover::label(ui, &plus, "New tab\nCtrl+Shift+T");
        if plus.clicked() {
            self.new_tab(None);
        }
        let more = ts::button_rect(&s, pill, 1);
        let profiles = round_icon(ui, more, Id::new("profiles"), glyph::SORT_DOWN, true, false, "Open a profile");
        if self.strip.menu.is_none() {
            shellman_ui::hover::label(ui, &profiles, "Open a profile");
        }
        if profiles.clicked() {
            if self.strip.menu.is_some() {
                self.strip.menu = None;
            } else {
                self.strip.menu = Some(more);
                self.strip.menu_opened = ui.ctx().cumulative_pass_nr();
            }
        }
        match todo {
            Some(Do::Select(i)) => self.select_tab(i),
            Some(Do::Close(i)) => self.close_tab(ui.ctx(), i),
            Some(Do::Move(a, b)) => {
                self.tabs.move_tab(a, b);
                self.strip.seen = (self.tabs.active(), self.tabs.len(), row.width().round() as u32);
            }
            None => {}
        }
        pill
    }

    /// The profile menu, when open: a glass island under its button. Returns where it is.
    pub(super) fn profile_menu(&mut self, ctx: &egui::Context) -> Option<Rect> {
        let anchor = self.strip.menu?;
        let c = colors();
        let screen = ctx.screen_rect();
        let rows = self.profiles.len() as f32;
        let height = rows * menu::ROW_H + 16.0;
        let x = anchor.left().clamp(screen.left() + 12.0, (screen.right() - 12.0 - MENU_W).max(screen.left() + 12.0));
        let rect = Rect::from_min_size(egui::pos2(x, anchor.bottom() + 12.0), egui::vec2(MENU_W, height));
        let default = crate::profiles::pick(&self.profiles, &self.config.default_profile).name.clone();
        let mut picked = None;
        egui::Area::new(Id::new("profile-menu")).order(Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
            ui.set_min_size(rect.size());
            ui.set_max_size(rect.size());
            let _ = ui.interact(rect, Id::new("profile-menu-bg"), Sense::click_and_drag());
            let fill = if c.light {
                shows_as(egui::Color32::from_rgb(246, 247, 250))
            } else {
                shows_as(egui::Color32::from_rgb(30, 33, 42))
            };
            ui.painter().rect(rect, egui::CornerRadius::same(14), fill, Stroke::new(1.0_f32, c.hairline), egui::StrokeKind::Inside);
            let inner = rect.shrink(8.0);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
            menu::flat_popup(&mut child);
            for (i, p) in self.profiles.iter().enumerate() {
                let keys = if i < 9 { format!("Ctrl+Shift+{}", i + 1) } else { String::new() };
                let icon = if p.name == default { Icon::None } else { Icon::Glyph(TERMINAL) };
                if menu::flat_item(&mut child, icon, &p.name, &keys, p.name == default, true) {
                    picked = Some(i);
                }
            }
        });
        // A press outside closes it (not the press that opened it).
        let pressed = ctx.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten());
        if let Some(p) = pressed {
            if ctx.cumulative_pass_nr() > self.strip.menu_opened && !rect.contains(p) && !anchor.contains(p) {
                self.strip.menu = None;
            }
        }
        if let Some(i) = picked {
            self.strip.menu = None;
            self.new_tab(Some(i));
        }
        Some(rect)
    }
}

/// A tab's title: what the program set (an exe's path, as ConPTY reports a console program's
/// title, shortened to its name), else the shell's name and the folder it is in.
pub fn tab_title(s: &crate::session::Session) -> String {
    let title = s.title();
    if title != s.program {
        return title;
    }
    match s.cwd() {
        Some(dir) => {
            let leaf = dir.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or(&dir);
            if leaf.is_empty() { format!("{title} {dir}") } else { format!("{title} · {leaf}") }
        }
        None => title,
    }
}
