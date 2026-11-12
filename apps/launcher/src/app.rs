//! The launcher window: a search box on top, the app grid below while it is empty, ranked
//! results while it is not.

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
/// If the window has no keyboard focus this long after appearing, ask for it again.
const FOCUS_RETRY: Duration = Duration::from_millis(250);

pub struct Launcher {
    hwnd: isize,
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
    pub fn new(ctx: egui::Context, hwnd: isize, shared: Arc<Shared>) -> Self {
        let index = AppIndex::default();
        let repaint = ctx.clone();
        index.refresh(move || repaint.request_repaint());
        let mut me = Self {
            hwnd,
            shared,
            index,
            apps: Arc::default(),
            usage: Usage::load(),
            thumbs: Thumbs::new(ctx),
            query: String::new(),
            grid: Grid::new(0, &[]),
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
        self.grid = Grid::new(self.apps.len(), &top);
        self.grid_sel = self.grid.first();
    }

    fn rank(&mut self) {
        let names: Vec<&str> = self.apps.iter().map(|a| a.name.as_str()).collect();
        let uses = |i: usize| self.usage.count(&self.apps[i].id);
        let hits = rank::rank(&names, &uses, &self.query, MAX_RESULTS);
        self.items = results::items(hits.into_iter().map(|h| h.index), self.query.trim().is_empty());
        self.result_sel = 0;
        self.reveal = true;
    }

    fn typing(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// Back to a clean state and out of sight.
    fn hide(&mut self) {
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
        .find(|&(key, _)| press(key))
        .map(|(_, d)| d);
        if self.typing() {
            // A list has only up and down; left and right edit the query as usual.
            if let Some(dir @ (Dir::Up | Dir::Down)) = dir {
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
            let repaint = ctx.clone();
            self.index
                .refresh_if_older(REFRESH_AFTER, move || repaint.request_repaint());
        }
        if self.shared.take_hide_request() {
            self.hide();
            return;
        }
        let focused = ctx.input(|i| i.focused);
        if focused {
            self.had_focus = true;
        } else if self.had_focus {
            self.hide();
        } else if self.shown_at.elapsed() > FOCUS_RETRY && self.shown_at.elapsed() < FOCUS_RETRY * 2 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ctx.request_repaint_after(FOCUS_RETRY);
        }
    }

    fn search_box(&mut self, ui: &mut egui::Ui) {
        let c = colors();
        let (bar, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 52.0), egui::Sense::hover());
        ui.painter().text(
            egui::pos2(bar.left() + 14.0, bar.center().y),
            egui::Align2::CENTER_CENTER,
            glyph::SEARCH,
            icon_font(24.0),
            c.muted,
        );
        let edit_rect = egui::Rect::from_min_max(egui::pos2(bar.left() + 44.0, bar.top()), bar.max);
        let edit = egui::TextEdit::singleline(&mut self.query)
            .id(egui::Id::new("query"))
            .font(egui::FontId::proportional(26.0))
            .frame(false)
            .margin(egui::Margin::ZERO)
            .vertical_align(egui::Align::Center)
            .hint_text(egui::RichText::new("Search apps").color(c.muted))
            .desired_width(edit_rect.width());
        let resp = ui.put(edit_rect, edit);
        // The box owns the keyboard whenever the launcher is showing.
        resp.request_focus();
        if resp.changed() {
            self.rank();
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
        let mut picked = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(c.shell).inner_margin(egui::Margin::same(20)))
            .show(ctx, |ui| {
                self.search_box(ui);
                let y = ui.cursor().top() + 4.0;
                ui.painter().hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0_f32, c.hairline));
                ui.add_space(12.0);
                picked = self.content(ui);
            });
        if let Some(item) = picked {
            self.activate(item);
        }
        self.reveal = false;
        self.reset_scroll = false;
    }

    /// The grid or the result list. Returns what was clicked.
    fn content(&mut self, ui: &mut egui::Ui) -> Option<Item> {
        if self.typing() {
            let out = results::show(
                ui,
                &self.items,
                &self.apps,
                &self.query,
                self.result_sel,
                &mut self.thumbs,
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
