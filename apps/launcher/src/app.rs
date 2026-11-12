//! The launcher window: a search box on top, the app grid below while it is empty, ranked
//! results while it is not.

use crate::fade::Fade;
use crate::grid::{self, Dir, Grid, Pos};
use crate::results::{self, Item};
use crate::summon::{self, Shared};
use crate::usage::Usage;
use crate::rank;
use shellman_core::apps_index::{self, App, AppIndex};
use shellman_core::thumbs::Thumbs;
use shellman_ui::theme::{self, colors, glyph, icon_font};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How many ranked results are kept: more than fit on screen, fewer than a scroll is worth.
const MAX_RESULTS: usize = 60;
/// The app list is read again on summoning when it is older than this.
const REFRESH_AFTER: Duration = Duration::from_secs(30);
/// The panel's content padding, and how long it takes to appear.
const PAD_X: f32 = 28.0;
const PAD_TOP: f32 = 22.0;
const APPEAR: Duration = Duration::from_millis(120);
/// If the window has no keyboard focus this long after appearing, ask for it again.
const FOCUS_RETRY: Duration = Duration::from_millis(250);
/// Losing focus this soon after appearing is the window settling, not the user clicking away.
const FOCUS_GRACE: Duration = Duration::from_millis(400);

pub struct Launcher {
    hwnd: isize,
    /// Whether the window has real acrylic; without it the panel is painted opaque.
    glass: bool,
    shared: Arc<Shared>,
    index: AppIndex,
    apps: Arc<Vec<App>>,
    usage: Usage,
    thumbs: Thumbs,
    query: String,
    grid: Grid,
    grid_sel: Pos,
    /// Result rows for the query, and which is selected.
    items: Vec<Item>,
    result_sel: usize,
    /// Scroll the selection into view on the next frame (after arrow keys).
    reveal: bool,
    reset_scroll: bool,
    /// The showing this window last saw, whether it has had focus since, and when it appeared.
    seen_shown: u32,
    had_focus: bool,
    shown_at: Instant,
}

impl Launcher {
    pub fn new(ctx: egui::Context, hwnd: isize, glass: bool, shared: Arc<Shared>) -> Self {
        let index = AppIndex::default();
        let repaint = ctx.clone();
        index.refresh(move || repaint.request_repaint());
        let mut me = Self {
            hwnd,
            glass,
            shared,
            index,
            apps: Arc::default(),
            usage: Usage::load(),
            thumbs: Thumbs::icons(ctx),
            query: String::new(),
            grid: Grid::new(&[], &[]),
            grid_sel: Pos::default(),
            items: Vec::new(),
            result_sel: 0,
            reveal: false,
            reset_scroll: false,
            seen_shown: 0,
            had_focus: false,
            shown_at: Instant::now(),
        };
        me.rebuild_grid();
        me
    }

    fn rebuild_grid(&mut self) {
        let top = self.usage.top(&self.apps);
        let listed: Vec<usize> = (0..self.apps.len()).filter(|&i| self.apps[i].is_listed()).collect();
        self.grid = Grid::new(&listed, &top);
        self.grid_sel = self.grid.first();
    }

    /// The settings folder changed: take in a cleared history (or one edited by hand).
    pub fn reload_usage(&mut self) {
        if self.usage.reload() {
            self.rebuild_grid();
            if self.typing() {
                self.rank();
            }
        }
    }

    fn rank(&mut self) {
        let names: Vec<&str> = self.apps.iter().map(|a| a.name.as_str()).collect();
        let uses = |i: usize| self.usage.count(&self.apps[i].id);
        let hits = rank::rank(&names, &uses, &self.query, MAX_RESULTS);
        let settings = !rank::rank(&[results::SETTINGS_NAME], &|_| 0, &self.query, 1).is_empty();
        self.items = results::items(
            hits.into_iter().map(|h| h.index),
            settings,
            self.query.trim().is_empty(),
        );
        self.result_sel = 0;
        self.reveal = true;
    }

    fn typing(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// Back to a clean state and out of sight.
    fn hide(&mut self) {
        self.shared.mark_hidden();
        summon::hide(self.hwnd);
        self.query.clear();
        self.items.clear();
        self.result_sel = 0;
        self.rebuild_grid();
        self.reset_scroll = true;
        self.thumbs.cancel_pending();
    }

    /// Start what was chosen, and go away.
    fn activate(&mut self, item: Item) {
        match item {
            Item::App(i) => {
                let app = self.apps[i].clone();
                self.usage.record(&app.id);
                std::thread::spawn(move || {
                    com_init();
                    apps_index::launch(&app);
                });
            }
            Item::Settings => {
                shellman_core::sibling::start("settings");
            }
            Item::Run => {
                let text = self.query.clone();
                std::thread::spawn(move || {
                    com_init();
                    apps_index::run_text(&text);
                });
            }
        }
        self.hide();
    }

    /// Keyboard: arrows, Enter, Esc. Taken before the search box sees them, so the box does not
    /// move its caret on Up and Down.
    fn keys(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        let typing = self.typing();
        let press = |key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
        if press(Key::Escape) {
            self.hide();
            return;
        }
        let enter = press(Key::Enter);
        let dir = [
            (Key::ArrowLeft, Dir::Left),
            (Key::ArrowRight, Dir::Right),
            (Key::ArrowUp, Dir::Up),
            (Key::ArrowDown, Dir::Down),
        ]
        .into_iter()
        // While typing, left and right stay with the search box, to move its caret.
        .filter(|&(_, d)| !typing || matches!(d, Dir::Up | Dir::Down))
        .find(|&(key, _)| press(key))
        .map(|(_, d)| d);
        if typing {
            if let Some(dir) = dir {
                let last = self.items.len().saturating_sub(1);
                self.result_sel = match dir {
                    Dir::Up => self.result_sel.saturating_sub(1),
                    _ => (self.result_sel + 1).min(last),
                };
                self.reveal = true;
            }
            if enter {
                if let Some(&item) = self.items.get(self.result_sel) {
                    self.activate(item);
                }
            }
        } else {
            if let Some(dir) = dir {
                self.grid_sel = self.grid.step(self.grid_sel, dir);
                self.reveal = true;
            }
            if enter {
                if let Some(i) = self.grid.app_at(self.grid_sel) {
                    self.activate(Item::App(i));
                }
            }
        }
    }

    /// Notice a new showing, and hide when focus is lost after having had it.
    fn follow_window(&mut self, ctx: &egui::Context) {
        let shown = self.shared.shown_count();
        if shown != self.seen_shown {
            self.seen_shown = shown;
            self.had_focus = false;
            self.shown_at = Instant::now();
            // One wake-up to check that focus arrived; no repeating timer.
            ctx.request_repaint_after(FOCUS_RETRY + Duration::from_millis(10));
            let repaint = ctx.clone();
            self.index
                .refresh_if_older(REFRESH_AFTER, move || repaint.request_repaint());
        }
        if self.shared.take_hide_request() {
            self.hide();
            return;
        }
        // After a `--background` start the window library shows the window once by itself.
        if !self.shared.is_showing() && summon::is_visible(self.hwnd) {
            summon::hide(self.hwnd);
        }
        let focused = ctx.input(|i| i.focused);
        if focused {
            self.had_focus = true;
        } else if self.had_focus && self.shown_at.elapsed() > FOCUS_GRACE {
            self.hide();
        } else if self.had_focus {
            // Focus flickered while the window was settling in: not a click elsewhere.
            self.had_focus = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        } else if self.shown_at.elapsed() > FOCUS_RETRY && self.shown_at.elapsed() < FOCUS_RETRY * 2 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    /// The search field: a glyph, the text at 22 pt, and an "esc" chip once something is typed.
    /// No frame and no line beneath; the glass shows around it.
    fn search_box(&mut self, ui: &mut egui::Ui) {
        let c = colors();
        let (bar, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 56.0), egui::Sense::hover());
        ui.painter().text(
            egui::pos2(bar.left() + 11.0, bar.center().y),
            egui::Align2::CENTER_CENTER,
            glyph::SEARCH,
            icon_font(18.0),
            c.muted,
        );
        let chip_w = if self.typing() { 44.0 } else { 0.0 };
        let edit_rect = egui::Rect::from_min_max(
            egui::pos2(bar.left() + 34.0, bar.top()),
            egui::pos2(bar.right() - chip_w, bar.bottom()),
        );
        let edit = egui::TextEdit::singleline(&mut self.query)
            .id(egui::Id::new("query"))
            .font(egui::FontId::proportional(22.0))
            .frame(false)
            .margin(egui::Margin::ZERO)
            .vertical_align(egui::Align::Center)
            .hint_text(egui::RichText::new("Search").color(c.muted))
            .desired_width(edit_rect.width());
        let resp = ui.put(edit_rect, edit);
        // The box owns the keyboard whenever the launcher is showing.
        resp.request_focus();
        if resp.changed() {
            self.rank();
        }
        if self.typing() {
            let chip = egui::Rect::from_center_size(
                egui::pos2(bar.right() - 20.0, bar.center().y),
                egui::vec2(34.0, 22.0),
            );
            ui.painter().rect_filled(chip, 7.0, c.text.gamma_multiply(0.08));
            ui.painter().text(
                chip.center(),
                egui::Align2::CENTER_CENTER,
                "esc",
                egui::FontId::proportional(11.0),
                c.muted,
            );
        }
    }

    pub fn frame(&mut self, ctx: &egui::Context) {
        self.thumbs.poll(ctx);
        let apps = self.index.apps();
        if !Arc::ptr_eq(&apps, &self.apps) {
            self.apps = apps;
            self.rebuild_grid();
            if self.typing() {
                self.rank();
            }
        }
        self.follow_window(ctx);
        self.keys(ctx);

        theme::set_light(ctx.theme() == egui::Theme::Light);
        let c = colors();
        let screen = ctx.screen_rect();

        // ---- PANEL BACKGROUND: the one place the panel's own fill is painted. ----
        let fill = if self.glass { c.shell } else { c.bg };
        egui::CentralPanel::default().frame(egui::Frame::new().fill(fill)).show(ctx, |ui| {
            if self.glass {
                // The same surfaces as Files and the dock: the glass tint, then the sheet.
                ui.painter().rect_filled(screen, 0.0, c.sheet);
            }
        });
        // ---- end of panel background ----

        // On showing, the content fades in and grows from 97%; nothing runs once it is done.
        let t = (self.shown_at.elapsed().as_secs_f32() / APPEAR.as_secs_f32()).min(1.0);
        let ease = 1.0 - (1.0 - t) * (1.0 - t);
        if t < 1.0 {
            ctx.request_repaint();
        }
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("content"));
        let scale = 0.97 + 0.03 * ease;
        ctx.set_transform_layer(
            layer,
            egui::emath::TSTransform::new(screen.center().to_vec2() * (1.0 - scale), scale),
        );
        let mut picked = None;
        egui::Area::new(egui::Id::new("content"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .constrain(false)
            .show(ctx, |ui| {
                ui.set_opacity(ease);
                ui.set_min_size(screen.size());
                let inner = egui::Rect::from_min_max(
                    screen.min + egui::vec2(PAD_X, PAD_TOP),
                    egui::pos2(screen.max.x - PAD_X, screen.max.y),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                    self.search_box(ui);
                    ui.add_space(14.0);
                    picked = self.content(ui, Fade { edge: screen.max.y });
                });
            });
        if let Some(item) = picked {
            self.activate(item);
        }
        self.reveal = false;
        self.reset_scroll = false;
    }

    /// The grid or the result list. Returns what was clicked.
    fn content(&mut self, ui: &mut egui::Ui, fade: Fade) -> Option<Item> {
        if self.typing() {
            let out = results::show(
                ui,
                &self.items,
                &self.apps,
                &self.query,
                self.result_sel,
                &mut self.thumbs,
                fade,
                self.reveal,
            );
            if let Some(i) = out.hover {
                self.result_sel = i;
            }
            out.launch.and_then(|i| self.items.get(i).copied())
        } else {
            let out = grid::show(
                ui,
                &self.grid,
                &self.apps,
                self.grid_sel,
                &mut self.thumbs,
                fade,
                self.reveal,
                self.reset_scroll,
            );
            if let Some(pos) = out.hover {
                self.grid_sel = pos;
            }
            out.launch.map(Item::App)
        }
    }
}

/// Shell launches want COM on the thread that makes them.
fn com_init() {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}
