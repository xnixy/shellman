//! The launcher window: a search box on top, the app grid below while it is empty, ranked
//! results while it is not.

use crate::fade::Fade;
use crate::grid::{self, Dir, Grid, Pos};
use crate::menu::{self, Facts, Pick};
use crate::results::{self, Item};
use crate::summon::{self, Shared};
use crate::usage::Usage;
use crate::rank;
use shellman_core::apps_index::{self, App, AppIndex};
use shellman_core::thumbs::Thumbs;
use shellman_ui::glass;
use shellman_ui::menu::popup::{self, Anchor, PopupMenu};
use std::sync::mpsc::{channel, Receiver};
use shellman_ui::theme::{self, colors, glyph, icon_font};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How many ranked results are kept: more than fit on screen, fewer than a scroll is worth.
const MAX_RESULTS: usize = 60;
/// The app list is read again on summoning when it is older than this.
const REFRESH_AFTER: Duration = Duration::from_secs(30);
/// The islands: the window's margin to them, the search pill's height and inner padding, the gap
/// under it, the list island's corner radius and the padding of what it holds.
const ISLAND_MARGIN: f32 = 20.0;
const PILL_H: f32 = 58.0;
const PILL_PAD: f32 = 24.0;
const GAP: f32 = 12.0;
const ISLAND_RADIUS: f32 = 22.0;
const LIST_PAD: f32 = 10.0;
/// Room above the rows (and the last row's room below), and above the grid's first label.
const RESULTS_PAD_TOP: f32 = 16.0;
const RESULTS_END: f32 = 16.0;
const GRID_PAD_TOP: f32 = 6.0;
/// How long the content takes to fade in.
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
    /// The right-click menu, what it is for, and that row's facts: on their way, then here.
    menu: PopupMenu<Pick>,
    menu_for: Option<Item>,
    facts_rx: Option<Receiver<Facts>>,
    facts: Option<Facts>,
    /// The menu was open on the last frame; and, once it has closed, until when a missing
    /// focus is the focus on its way back from the menu rather than a click elsewhere.
    menu_seen: bool,
    quiet_until: Option<Instant>,
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
            menu: PopupMenu::new(),
            menu_for: None,
            facts_rx: None,
            facts: None,
            menu_seen: false,
            quiet_until: None,
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
        let aliases: Vec<String> = self.apps.iter().map(|a| a.alias().unwrap_or_default()).collect();
        let uses = |i: usize| self.usage.count(&self.apps[i].id);
        let own = |i: usize| self.apps[i].own;
        let hits = rank::rank_apps(&names, &aliases, &own, &uses, &self.query, MAX_RESULTS);
        self.items = results::items(hits.into_iter().map(|h| h.index), self.query.trim().is_empty());
        self.result_sel = 0;
        self.reveal = true;
    }

    fn typing(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// Back to a clean state and out of sight.
    fn hide(&mut self) {
        self.menu.close();
        self.menu_for = None;
        self.facts_rx = None;
        self.shared.mark_hidden();
        summon::hide(self.hwnd);
        self.query.clear();
        self.items.clear();
        self.result_sel = 0;
        self.rebuild_grid();
        self.reset_scroll = true;
        self.thumbs.cancel_pending();
    }

    /// Start what was chosen, and go away. `admin` (Ctrl+Shift, as in Windows' Start) starts it
    /// as administrator, behind the UAC prompt, which the worker waits for.
    fn activate(&mut self, item: Item, admin: bool) {
        match item {
            Item::App(i) => {
                let app = self.apps[i].clone();
                self.usage.record(&app.id);
                std::thread::spawn(move || {
                    com_init();
                    if admin {
                        apps_index::launch_as_admin(&app);
                    } else {
                        apps_index::launch(&app);
                    }
                });
            }
            Item::Run => {
                let text = self.query.clone();
                std::thread::spawn(move || {
                    com_init();
                    if admin {
                        apps_index::admin::run_text(&text);
                    } else {
                        apps_index::run_text(&text);
                    }
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
        // Ctrl+Shift+Enter first: a plain Enter pattern would take it too.
        let admin = ctx.input_mut(|i| i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::Enter));
        let enter = admin || press(Key::Enter);
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
                    self.activate(item, admin);
                }
            }
        } else {
            if let Some(dir) = dir {
                self.grid_sel = self.grid.step(self.grid_sel, dir);
                self.reveal = true;
            }
            if enter {
                if let Some(i) = self.grid.app_at(self.grid_sel) {
                    self.activate(Item::App(i), admin);
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
        // The menu is a window of its own and holds the focus while open: that is not the user
        // leaving. When it closes by a pick or Esc the focus comes back, a moment later.
        if self.menu.is_open() {
            self.menu_seen = true;
            return;
        }
        if std::mem::take(&mut self.menu_seen) {
            self.quiet_until = Some(Instant::now() + FOCUS_GRACE);
            ctx.request_repaint_after(FOCUS_GRACE + Duration::from_millis(10));
        }
        let focused = ctx.input(|i| i.focused);
        if focused {
            self.had_focus = true;
            self.quiet_until = None;
        } else if self.quiet_until.is_some_and(|t| Instant::now() < t) {
            // Still on its way back.
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

    /// The search field inside the pill `bar`: a glyph, the text at 20 pt, and an "esc" chip
    /// once something is typed. No frame and no line beneath; the pill is the field.
    fn search_box(&mut self, ui: &mut egui::Ui, bar: egui::Rect) {
        let c = colors();
        ui.painter().text(
            egui::pos2(bar.left() + PILL_PAD + 9.0, bar.center().y),
            egui::Align2::CENTER_CENTER,
            glyph::SEARCH,
            icon_font(18.0),
            c.muted,
        );
        let chip_w = if self.typing() { 48.0 } else { 0.0 };
        let edit_rect = egui::Rect::from_min_max(
            egui::pos2(bar.left() + PILL_PAD + 32.0, bar.top()),
            egui::pos2(bar.right() - PILL_PAD - chip_w, bar.bottom()),
        );
        let edit = egui::TextEdit::singleline(&mut self.query)
            .id(egui::Id::new("query"))
            .font(egui::FontId::proportional(20.0))
            .frame(false)
            .margin(egui::Margin::ZERO)
            .vertical_align(egui::Align::Center)
            .hint_text(egui::RichText::new("Search apps, files, settings").color(c.muted))
            .desired_width(edit_rect.width());
        let resp = ui.put(edit_rect, edit);
        // The box owns the keyboard whenever the launcher is showing.
        resp.request_focus();
        if resp.changed() {
            self.rank();
        }
        if self.typing() {
            let chip = egui::Rect::from_center_size(
                egui::pos2(bar.right() - PILL_PAD - 17.0, bar.center().y),
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

    /// The search pill and, under it, the island of results (or the app grid); a pill alone when
    /// there is nothing to list. The window stays one size, so what changes with the content is
    /// only these rectangles, and the glass and the content are laid out in the same frame.
    fn layout(&self, screen: egui::Rect) -> Layout {
        let left = screen.left() + ISLAND_MARGIN;
        let right = screen.right() - ISLAND_MARGIN;
        let pill = egui::Rect::from_min_max(
            egui::pos2(left, screen.top()),
            egui::pos2(right, screen.top() + PILL_H),
        );
        let top = pill.bottom() + GAP;
        let room = (screen.bottom() - top).max(0.0);
        let (pad_top, content) = if self.typing() {
            (RESULTS_PAD_TOP, results::ROW_H * self.items.len() as f32 + RESULTS_END)
        } else {
            (GRID_PAD_TOP, self.grid.height())
        };
        let want = if content > 0.0 { pad_top + content } else { 0.0 };
        let height = want.min(room);
        let list = (height > 0.0)
            .then(|| egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(right - left, height)));
        Layout { pill, list, pad_top, overflows: want > room }
    }

    /// One frame. Returns the glass for it: the islands, in points; empty while hidden.
    pub fn frame(&mut self, ctx: &egui::Context) -> Vec<glass::Island> {
        self.menu_frame(ctx);
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
        // Hidden: draw nothing. A drawn search box keeps the focus, and its caret blink asks for
        // repaints that a hidden window never gets, which spins the event loop on a whole core.
        if !self.shared.is_showing() {
            return Vec::new();
        }
        self.keys(ctx);

        // SHELLMAN_FORCE_LIGHT: look at the light palette without changing Windows' setting.
        let force_light = std::env::var_os("SHELLMAN_FORCE_LIGHT").is_some();
        theme::set_light(force_light || ctx.theme() == egui::Theme::Light);
        let screen = ctx.screen_rect();
        let layout = self.layout(screen);
        let mut islands = vec![glass::Island::pill(layout.pill)];
        if let Some(list) = layout.list {
            islands.push(glass::Island::new(list, ISLAND_RADIUS));
        }

        // A press that lands on no island (the margins, the gap, the empty window below) is a
        // click away: the launcher closes, so the transparent part of the window never traps
        // clicks meant for the desktop.
        let pressed = ctx.input(|i| i.pointer.primary_pressed().then(|| i.pointer.interact_pos()).flatten());
        if pressed.is_some_and(|at| !islands.iter().any(|i| i.rect.contains(at))) {
            self.hide();
            return Vec::new();
        }

        // On showing, the content fades in; nothing runs once it is done. (The glass is
        // instant: it is a separate window, and cannot take part in a fade.)
        let t = (self.shown_at.elapsed().as_secs_f32() / APPEAR.as_secs_f32()).min(1.0);
        let ease = 1.0 - (1.0 - t) * (1.0 - t);
        if t < 1.0 {
            ctx.request_repaint();
        }
        let mut picked = None;
        let mut menu_at = None;
        egui::Area::new(egui::Id::new("content"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .constrain(false)
            .show(ctx, |ui| {
                ui.set_min_size(screen.size());
                // Without composed glass the islands paint their own solid fills, under all this.
                let under = ui.painter().add(egui::Shape::Noop);
                ui.set_opacity(ease);
                ui.scope_builder(egui::UiBuilder::new().max_rect(layout.pill), |ui| {
                    self.search_box(ui, layout.pill);
                });
                if let Some(list) = layout.list {
                    let inner = egui::Rect::from_min_max(
                        egui::pos2(list.left() + LIST_PAD, list.top() + layout.pad_top),
                        egui::pos2(list.right() - LIST_PAD, list.bottom()),
                    );
                    // Rows only fade into the island's edge when there are more than fit.
                    let edge = if layout.overflows { list.bottom() - 4.0 } else { 1.0e9 };
                    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                        (picked, menu_at) = self.content(ui, Fade { edge });
                    });
                }
                if !self.glass {
                    let style = glass::Style::from_theme(ISLAND_RADIUS);
                    ui.painter().set(under, egui::Shape::Vec(glass::islands::solid_shapes(&islands, &style)));
                }
            });
        if let Some(item) = picked {
            // Ctrl+Shift+click, like Ctrl+Shift+Enter.
            self.activate(item, admin_held(ctx));
        } else if let Some((item, at)) = menu_at {
            self.open_menu(ctx, item, at);
        }
        self.reveal = false;
        self.reset_scroll = false;
        islands
    }

    /// The right-click menu's part of a frame, also while the launcher is hidden: its window is
    /// kept, late facts go into the open menu, and what was picked is done.
    fn menu_frame(&mut self, ctx: &egui::Context) {
        popup::declare(ctx);
        if let Some(facts) = self.facts_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.facts_rx = None;
            if self.menu.is_open() {
                self.menu.update(menu::app_entries(Some(&facts)));
            }
            self.facts = Some(facts);
        }
        let Some(pick) = self.menu.take_pick() else { return };
        let Some(item) = self.menu_for.take() else { return };
        let facts = self.facts.take().unwrap_or_default();
        match pick {
            Pick::Open => self.activate(item, false),
            Pick::Admin => self.activate(item, true),
            // The launcher stays: the dock shows what happened.
            Pick::Pin | Pick::Unpin => {
                if let Some(pin) = facts.pin {
                    std::thread::spawn(move || menu::set_pinned(&pin, pick == Pick::Pin));
                }
            }
            Pick::Location => {
                if let Some(path) = facts.location {
                    shellman_core::sibling::start_with("files", &["--select", &path.to_string_lossy()]);
                }
                self.hide();
            }
        }
    }

    /// Open the menu for a right-clicked row, at the pointer (`at`, in the window's points).
    fn open_menu(&mut self, ctx: &egui::Context, item: Item, at: egui::Pos2) {
        self.menu_for = Some(item);
        self.facts = None;
        self.facts_rx = None;
        let entries = match item {
            Item::Run => menu::run_entries(),
            Item::App(i) => {
                let app = self.apps[i].clone();
                let (tx, rx) = channel();
                let wake = ctx.clone();
                std::thread::spawn(move || {
                    com_init();
                    let _ = tx.send(menu::facts(&app));
                    wake.request_repaint();
                });
                self.facts_rx = Some(rx);
                menu::app_entries(None)
            }
        };
        self.menu.open(ctx, Anchor::in_window(ctx, at), entries);
    }

    /// The grid or the result list. Returns what was clicked, and what was right-clicked with
    /// where the pointer was. A right-clicked row is selected, so the menu is seen to be its.
    fn content(&mut self, ui: &mut egui::Ui, fade: Fade) -> (Option<Item>, Option<(Item, egui::Pos2)>) {
        if self.typing() {
            let out = results::show(
                ui,
                &self.items,
                &self.apps,
                &self.query,
                self.result_sel,
                admin_held(ui.ctx()),
                &mut self.thumbs,
                fade,
                self.reveal,
            );
            if let Some(i) = out.hover {
                self.result_sel = i;
            }
            let menu = out.menu.and_then(|(i, at)| {
                self.result_sel = i;
                self.items.get(i).map(|item| (*item, at))
            });
            (out.launch.and_then(|i| self.items.get(i).copied()), menu)
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
            let menu = out.menu.map(|(pos, index, at)| {
                self.grid_sel = pos;
                (Item::App(index), at)
            });
            (out.launch.map(Item::App), menu)
        }
    }
}

/// Where the pieces of one frame go.
struct Layout {
    pill: egui::Rect,
    /// The island under the pill, when there is anything to list.
    list: Option<egui::Rect>,
    /// How far the list's content starts below the island's top.
    pad_top: f32,
    /// Whether the content is taller than the island, so it scrolls and fades at the edge.
    overflows: bool,
}

/// Ctrl and Shift are both down: what is started now is started as administrator.
fn admin_held(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift)
}

/// Shell launches want COM on the thread that makes them.
fn com_init() {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}
