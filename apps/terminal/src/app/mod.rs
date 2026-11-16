//! The window: a pill of tabs and a pill of window buttons on top, and the terminal island below,
//! the desktop showing between them. egui draws the chrome; the grid is drawn by the renderer
//! inside the island, through a paint callback.
//!
//! Each tab owns one or more panes (`splits::Panes`), each a shell (`Session`) on an island of
//! its own; a small pool of renderers draws the shown tab's panes, one each, and is reused
//! whichever tab is shown. A frame happens only
//! when something changed: new output in the shown tab (its pty thread wakes the window), input,
//! a resize, the theme, or the cursor's blink (a timer, only while a program asked for a blinking
//! cursor and the window has the focus). A tab not shown wakes the window only for its dot,
//! title, bell or exit, so it costs no frames while it runs.

mod find;
mod input;
mod pointer;
mod script;
mod strip;
mod view;
pub mod win32_input;

use crate::cli::Launch;
use crate::config::Config;
use crate::defterm::server::Arrived;
use crate::pty::Pty;
use crate::instance;
use crate::profiles::{self, Profile};
use crate::render::{Frame, Renderer};
use crate::session::{Session, Size, Start};
use crate::splits::{self, Axis, Dir, Panes};
use crate::stats::Stats;
use crate::tabs::Tabs;
use egui::{pos2, vec2, Rect};
use shellman_ui::appearance::Appearance;
use shellman_ui::glass::islands::{self, Island};
use shellman_ui::glass::{GlassPanel, Style};
use shellman_ui::theme::{self, AccentChoice};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// Space between the window's edge and the islands, and between islands.
const MARGIN: f32 = 12.0;
/// The row of tabs and the window buttons beside it (as Files').
const TABS_H: f32 = 40.0;
/// The window buttons' pill.
const BUTTON_H: f32 = 30.0;
const CONTROLS_W: f32 = BUTTON_H * 3.0 + 20.0;
/// The terminal island's corner radius.
pub const RADIUS: f32 = 22.0;
/// Space between the island's edge and the grid. More than the corner's inset (about 0.3 of the
/// radius), so the grid's square corners never reach the rounded ones.
const PADDING: f32 = 14.0;

/// One shell on its own island in a tab.
pub struct Pane {
    id: u64,
    session: Session,
    /// Stays open after its program exits (`--hold`); `held` once that was said in it.
    hold: bool,
    held: bool,
    /// The WSL distribution it runs (a profile's), whose POSIX directories a new tab of that
    /// same distribution may start in.
    distro: Option<String>,
}

/// A tab: its panes, split as the user split them. Its title is its focused pane's.
pub struct Tab {
    id: u64,
    panes: Panes<Pane>,
}

impl Tab {
    /// The focused pane: where typing goes, what the title and find are about.
    pub fn pane(&self) -> &Pane {
        self.panes.focused()
    }
}

/// A renderer and the snapshot it last drew; slot `i` draws the shown tab's pane `i`.
struct Slot {
    renderer: Renderer,
    frame: Frame,
    /// What its last snapshot was taken with (pane and look); None draws afresh.
    drawn: Option<view::Drawn>,
}

impl Slot {
    fn new(renderer: Renderer) -> Self {
        Self { renderer, frame: Frame { cols: 0, rows: 0, cells: Vec::new(), cursor: None, blinking: false, clusters: Default::default() }, drawn: None }
    }
}

/// Where a shown pane was laid out last frame.
#[derive(Clone, Copy)]
struct Placed {
    island: Rect,
    grid: Rect,
}

pub struct TerminalApp {
    tabs: Tabs<Tab>,
    next_id: u64,
    profiles: Vec<Profile>,
    config: Config,
    /// The grid font's size in points, as zoomed.
    font_pt: f32,
    /// Renderers for the shown panes (at least one).
    slots: Vec<Slot>,
    panel: GlassPanel,
    look: Appearance,
    /// Wakes the window; every tab's pty thread holds one.
    wake: Arc<dyn Fn() + Send + Sync>,
    view: view::View,
    keys: input::KeyState,
    pointer: pointer::Pointer,
    strip: strip::StripState,
    /// Where the focused pane's grid was drawn last frame, in points.
    grid: Rect,
    /// The shown tab's panes as laid out last frame, and the dividers between them.
    placed: Vec<Placed>,
    pane_layout: splits::Layout,
    /// The divider being dragged (its split's path).
    dragging: Option<Vec<bool>>,
    /// Whether the window had the focus last frame (focus reports go out on a change).
    focused: bool,
    /// The window's title as last set (the shown tab's).
    window_title: String,
    /// Wakes the window when Settings saves the look (glass, accent), to follow it live.
    look_watch: shellman_core::watch::Watcher,
    /// Launches another start of the terminal handed to this window (`instance`).
    inbox: instance::Inbox,
    stats: Stats,
    /// Keys read from the window procedure for a console in win32-input-mode.
    win32_keys: Option<Arc<win32_input::Gate>>,
    /// Find in the scrollback.
    find: find::Find,
    /// Steps of `SHELLMAN_TERMINAL_SCRIPT` not yet run (a preview; see `script.rs`).
    script: std::collections::VecDeque<script::Step>,
}

impl TerminalApp {
    /// `launch`: what the first tab runs, and where; or `handoff`, a console Windows handed over.
    pub fn new(ctx: &egui::Context, look: Appearance, panel: GlassPanel, hwnd: Option<isize>, launch: Launch, handoff: Option<Arrived>) -> Result<Self, String> {
        let config = if handoff.is_some() { Config::read() } else { Config::load() };
        let detecting = shellman_core::timing::span("terminal: profiles detect");
        let profiles = profiles::build(&profiles::detect());
        drop(detecting);
        let ppp = ctx.pixels_per_point();
        let font_pt = config.font_size;
        let rendering = shellman_core::timing::span("terminal: renderer (font atlas)");
        let renderer = Renderer::new(view::font_px(font_pt, ppp), &config.font_family)?;
        drop(rendering);
        let wake_ctx = ctx.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || wake_ctx.request_repaint());
        let look_watch = shellman_core::watch::Watcher::quick(ctx.clone());
        if let Some(dir) = shellman_ui::appearance::file().and_then(|f| f.parent().map(|p| p.to_path_buf())) {
            look_watch.set_dirs(vec![dir]);
        }
        let minimized = handoff.as_ref().is_some_and(Arrived::minimized);
        let cell = cell_size(&renderer);
        let starting = shellman_core::timing::span("terminal: shell started");
        let (first, distro) = match handoff {
            Some(arrived) => (attach(arrived, cell, &config, wake.clone()).map_err(|e| format!("could not show the console: {e}"))?, None),
            None => {
                let (command, dir, distro) = plan(&profiles, &config, &launch, None);
                (spawn(&command, Some(dir), cell, &config, wake.clone()).map_err(|e| format!("could not start {command}: {e}"))?, distro)
            }
        };
        drop(starting);
        if minimized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        let inbox = instance::listen(wake.clone());
        Ok(Self {
            tabs: Tabs::new(Tab { id: 0, panes: Panes::new(Pane { id: 1, session: first, hold: launch.hold && launch.command.is_some(), held: false, distro }) }),
            next_id: 2,
            profiles,
            font_pt,
            slots: vec![Slot::new(renderer)],
            panel,
            look,
            wake,
            view: view::View::new(),
            keys: input::KeyState::default(),
            pointer: pointer::Pointer::default(),
            strip: strip::StripState::default(),
            grid: Rect::NOTHING,
            placed: Vec::new(),
            pane_layout: splits::Layout::default(),
            dragging: None,
            focused: true,
            window_title: String::new(),
            look_watch,
            inbox,
            stats: Stats::from_env(),
            config,
            win32_keys: hwnd.map(|h| win32_input::install(h, ctx.clone())),
            find: find::Find::default(),
            script: std::env::var("SHELLMAN_TERMINAL_SCRIPT").map(|s| script::parse(&s).into()).unwrap_or_default(),
        })
    }

    /// Open a tab running `profile` (an index into the profiles; None for the default), in the
    /// shown tab's directory when its shell reports one.
    fn new_tab(&mut self, profile: Option<usize>) {
        let (command, dir, distro) = self.launch_for(profile);
        self.open_tab(&command, dir, false, distro);
    }

    /// What a new tab or pane of `profile` runs (None for the default), where, and its WSL
    /// distribution: in the focused pane's directory when its shell reports one.
    fn launch_for(&self, profile: Option<usize>) -> (String, PathBuf, Option<String>) {
        let p = match profile.and_then(|i| self.profiles.get(i)) {
            Some(p) => p,
            None => profiles::pick(&self.profiles, &self.config.default_profile),
        };
        let shown = self.tabs.current().pane();
        let cwd = shown.session.cwd();
        let dir = match &cwd {
            Some(path) => profiles::Dir::Reported { path, distro: shown.distro.as_deref() },
            None => profiles::Dir::None,
        };
        let (command, dir) = p.launch(dir, &home());
        (command, dir, p.distro().map(str::to_owned))
    }

    /// Start a pane running `command` in `dir`.
    fn start_pane(&mut self, command: &str, dir: PathBuf, hold: bool, distro: Option<String>) -> Option<Pane> {
        match spawn(command, Some(dir), cell_size(&self.slots[0].renderer), &self.config, self.wake.clone()) {
            Ok(session) => {
                let id = self.next_id;
                self.next_id += 1;
                Some(Pane { id, session, hold, held: false, distro })
            }
            Err(e) => {
                eprintln!("terminal: could not start {command}: {e}");
                None
            }
        }
    }

    /// Open a tab running `command` in `dir`, right after the shown one, and show it.
    fn open_tab(&mut self, command: &str, dir: PathBuf, hold: bool, distro: Option<String>) {
        if let Some(pane) = self.start_pane(command, dir, hold, distro) {
            let id = self.next_id;
            self.next_id += 1;
            self.leave_tab();
            self.tabs.open(Tab { id, panes: Panes::new(pane) });
            self.enter_tab();
        }
    }

    /// Split the focused pane (`None`: whichever way suits its shape); the new pane runs the
    /// default profile in the focused pane's directory, and takes the focus. Refused when the
    /// halves would be too small.
    fn split(&mut self, axis: Option<Axis>) {
        let focus = self.tabs.current().panes.focus();
        let Some(&rect) = self.pane_layout.panes.get(focus) else { return };
        let axis = axis.unwrap_or_else(|| Panes::<Pane>::auto_axis(rect));
        if !Panes::<Pane>::can_split(rect, axis) {
            return;
        }
        let (command, dir, distro) = self.launch_for(None);
        let Some(pane) = self.start_pane(&command, dir, false, distro) else { return };
        pane.session.set_shown(true);
        self.focus_report(false);
        self.tabs.current_mut().panes.split(axis, pane);
        self.focus_changed();
    }

    /// Ctrl+Shift+W: close the focused pane; the tab when it is the only one.
    fn close_pane(&mut self, ctx: &egui::Context) {
        if self.tabs.current().panes.len() == 1 {
            self.close_tab(ctx, self.tabs.active());
            return;
        }
        let tab = self.tabs.current_mut();
        let focus = tab.panes.focus();
        drop(tab.panes.close(focus));
        self.focus_changed();
    }

    /// Focus pane `i` of the shown tab.
    fn focus_pane(&mut self, i: usize) {
        if i != self.tabs.current().panes.focus() && i < self.tabs.current().panes.len() {
            self.focus_report(false);
            self.tabs.current_mut().panes.set_focus(i);
            self.focus_changed();
        }
    }

    /// Alt+arrow: focus the pane that way.
    fn focus_towards(&mut self, d: Dir) {
        if let Some(i) = splits::neighbour(&self.pane_layout.panes, self.tabs.current().panes.focus(), d) {
            self.focus_pane(i);
        }
    }

    /// Alt+Shift+arrow: move the divider beside the focused pane.
    fn nudge_pane(&mut self, d: Dir) {
        let layout = self.pane_layout.clone();
        self.tabs.current_mut().panes.nudge(&layout, d, 24.0);
    }

    /// The focused pane's program hears the window's focus moved in or out of it, when it
    /// asked to (DECSET 1004) and the window has the focus.
    fn focus_report(&self, gained: bool) {
        let s = &self.tabs.current().pane().session;
        if self.focused && s.mode().contains(alacritty_terminal::term::TermMode::FOCUS_IN_OUT) {
            s.report(crate::mouse::focus(gained));
        }
    }

    /// Another pane took the focus: it hears so, a press under way is forgotten, and its cursor
    /// shows at once.
    fn focus_changed(&mut self) {
        self.focus_report(true);
        self.pointer.reset();
        self.view.hover = crate::snapshot::Hover::None;
        self.view.restart_blink();
    }

    /// Tabs another start of the terminal handed over: each opens, and the window comes forward.
    fn take_handed_over(&mut self, ctx: &egui::Context) {
        let launches: Vec<Launch> = std::mem::take(&mut *self.inbox.lock().unwrap());
        if launches.is_empty() {
            return;
        }
        for launch in launches {
            let shown = self.tabs.current().pane();
            let cwd = shown.session.cwd().map(|c| (c, shown.distro.clone()));
            let (command, dir, distro) = plan(&self.profiles, &self.config, &launch, cwd);
            self.open_tab(&command, dir, launch.hold && launch.command.is_some(), distro);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    /// Close tab `i`; the last one closes the window.
    fn close_tab(&mut self, ctx: &egui::Context, i: usize) {
        if self.tabs.len() == 1 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let shown = i == self.tabs.active();
        if shown {
            self.leave_tab();
        }
        drop(self.tabs.close(i));
        if shown {
            self.enter_tab();
        }
    }

    /// Show tab `i`, telling both tabs.
    fn select_tab(&mut self, i: usize) {
        if i != self.tabs.active() && i < self.tabs.len() {
            self.leave_tab();
            self.tabs.select(i);
            self.enter_tab();
        }
    }

    /// The shown tab is about to be hidden: it stops waking the window, and a program that
    /// tracks focus hears it lost it.
    fn leave_tab(&mut self) {
        for p in self.tabs.current().panes.iter() {
            p.session.set_shown(false);
        }
        self.focus_report(false);
        self.pointer.reset();
    }

    /// A tab was just shown: all its panes draw.
    fn enter_tab(&mut self) {
        for p in self.tabs.current().panes.iter() {
            p.session.set_shown(true);
        }
        self.focus_report(true);
        self.view.restart_blink();
    }

    /// Follow the Windows app mode, the Windows accent while that is the choice, and the look
    /// Settings saves (glass amount, accent).
    fn track_system(&mut self, ctx: &egui::Context) {
        if !self.look_watch.poll().is_empty() {
            let look = Appearance::load_or(self.look);
            if look != self.look {
                self.look = look;
                look.apply(ctx);
            }
            self.reload_config();
        }
        theme::set_light(ctx.theme() == egui::Theme::Light);
        if self.look.accent == AccentChoice::System {
            theme::set_accent(ctx, AccentChoice::System.resolve());
        }
    }

    /// Whether the next keys go to the shown tab's console whole (win32-input-mode): it asked
    /// for them and nothing else (the profile menu) has the keyboard.
    fn gate_keys(&self, ctx: &egui::Context) {
        if let Some(gate) = &self.win32_keys {
            let s = &self.tabs.current().pane().session;
            let active = s.win32_input() && !self.strip.menu_open() && !self.find.has_focus(ctx) && !self.chrome_has_focus(ctx);
            gate.set(active, active && s.with_term(crate::select::has), self.tabs.current().panes.len() > 1);
        }
    }

    /// `terminal.json` changed (Settings saved it): what can change in a running window does.
    /// The font and its size (zoom starts over), the history kept, copy-on-select, the default
    /// profile and what a second start does; `default_terminal` is read where it is used.
    fn reload_config(&mut self) {
        let Some(new) = Config::try_read() else { return };
        if new == self.config {
            return;
        }
        let old = std::mem::replace(&mut self.config, new);
        if old.font_family != self.config.font_family {
            for slot in &mut self.slots {
                if let Err(e) = slot.renderer.set_family(&self.config.font_family) {
                    eprintln!("terminal: {e}");
                }
            }
            self.mark_shown_dirty();
        }
        if old.font_size != self.config.font_size {
            self.font_pt = self.config.font_size;
            self.mark_shown_dirty();
        }
        if old.color_scheme != self.config.color_scheme {
            self.slots.iter_mut().for_each(|s| s.drawn = None);
            self.mark_shown_dirty();
        }
        if old.scrollback != self.config.scrollback {
            for p in self.tabs.iter().flat_map(|t| t.panes.iter()) {
                p.session.set_scrollback(self.config.scrollback);
            }
        }
    }

    /// Every shown pane's snapshot is taken again (the font changed).
    fn mark_shown_dirty(&self) {
        for p in self.tabs.current().panes.iter() {
            p.session.mark_dirty();
        }
    }

    /// Panes whose shell exited close, and tabs with them; the window closes with the last.
    fn reap(&mut self, ctx: &egui::Context) -> bool {
        let gone = |p: &Pane| p.session.exited() && !p.hold;
        if !self.tabs.iter().flat_map(|t| t.panes.iter()).any(|p| p.session.exited() && !p.held) {
            return true;
        }
        // A held pane says its program ended, and stays.
        for t in self.tabs.iter_mut() {
            for p in t.panes.iter_mut().filter(|p| p.hold && !p.held && p.session.exited()) {
                p.held = true;
                let code = p.session.exit_code().map_or_else(|| "?".to_owned(), |c| c.to_string());
                p.session.print(&format!("\r\n\x1b[0m[process exited with code {code}; Ctrl+Shift+W closes it]"));
            }
        }
        let shown = self.tabs.current().pane().id;
        // A tab's last pane stays in `retain`: the tab goes then.
        for t in self.tabs.iter_mut() {
            t.panes.retain(|p| !gone(p));
        }
        let left = self.tabs.retain(|t| !(t.panes.len() == 1 && gone(t.pane())));
        if !left {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return false;
        }
        if self.tabs.current().pane().id != shown {
            self.enter_tab();
        }
        true
    }

    /// Focus in and out, to the program in the shown tab when it asked (DECSET 1004).
    fn track_focus(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|i| i.focused);
        if focused != self.focused {
            self.focused = focused;
            let s = &self.tabs.current().pane().session;
            if s.mode().contains(alacritty_terminal::term::TermMode::FOCUS_IN_OUT) {
                s.report(crate::mouse::focus(focused));
            }
            self.view.restart_blink();
        }
    }

    /// OSC 52 copies, bells, and the window's title.
    fn track_tabs(&mut self, ctx: &egui::Context) {
        for p in self.tabs.iter().flat_map(|t| t.panes.iter()) {
            if let Some(text) = p.session.take_copy() {
                ctx.copy_text(text);
            }
        }
        let s = &self.tabs.current().pane().session;
        if s.take_bell() && !self.focused {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Informational));
        }
        let title = s.title();
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{title} - Terminal")));
            self.window_title = title;
        }
    }

    fn layout(&mut self, ctx: &egui::Context, cpu: Option<f32>) {
        self.panel.set_style(Style::from_theme(RADIUS));
        let ppp = ctx.pixels_per_point();
        let px = view::font_px(self.font_pt, ppp);
        for slot in &mut self.slots {
            if let Err(e) = slot.renderer.set_px(px) {
                eprintln!("terminal: {e}");
            }
        }
        let mut list: Vec<Island> = Vec::new();
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
            let screen = ctx.screen_rect();
            // A solid window (high contrast, SHELLMAN_SOLID) shows the page under the gaps.
            let page = ui.painter().add(egui::Shape::Noop);
            let under = ui.painter().add(egui::Shape::Noop);
            islands::drag_gaps(ui, screen);

            let top = screen.top() + MARGIN;
            let right = screen.right() - MARGIN;
            let controls = Rect::from_min_max(pos2(right - CONTROLS_W, top), pos2(right, top + TABS_H));
            let mut row = Rect::from_min_max(pos2(screen.left() + MARGIN, top), pos2(controls.left() - MARGIN, top + TABS_H));
            // Find's pill, in the row between the tabs and the window buttons; in a narrow window
            // it takes the whole row while it is open.
            let mut tabs_shown = true;
            if self.find.open {
                let (w, alone) = find::pill_width(row.width(), MARGIN);
                let pill = Rect::from_min_max(pos2(row.right() - w, top), row.max);
                row.max.x = pill.left() - MARGIN;
                tabs_shown = !alone;
                list.push(Island::pill(pill));
                self.find_pill(ui, pill);
            }
            if tabs_shown {
                let pill = self.tab_strip(ui, row);
                list.push(Island::pill(pill));
            }
            list.push(Island::pill(controls));
            let buttons = Rect::from_center_size(controls.center(), vec2(BUTTON_H * 3.0, BUTTON_H));
            shellman_ui::chips::window_buttons(ui, buttons);

            // The panes, each an island; the gaps between them are dividers.
            let room = Rect::from_min_max(pos2(screen.left() + MARGIN, controls.bottom() + MARGIN), screen.max - vec2(MARGIN, MARGIN));
            self.pane_layout = self.tabs.current().panes.layout(room);
            self.dividers(ui, room);
            let n = self.pane_layout.panes.len();
            self.ensure_slots(n);
            self.placed.clear();
            for i in 0..n {
                let island = self.pane_layout.panes[i];
                list.push(Island::new(island, RADIUS));
                let grid = self.fit(i, island.shrink(PADDING), ppp);
                self.placed.push(Placed { island, grid });
            }
            // A press on another pane focuses it, before the pointer acts on the focused one.
            self.focus_pressed(ctx);
            let focus = self.tabs.current().panes.focus();
            let island = self.placed[focus].island;
            self.grid = self.placed[focus].grid;
            self.name_panes(ui);
            self.pointer(ctx, island);

            let started = Instant::now();
            let mut rows = 0;
            for i in 0..n.min(self.slots.len()) {
                rows += self.redraw_if_changed(ctx, i);
                ui.painter().add(self.slots[i].renderer.paint(self.placed[i].grid));
            }
            self.stats.frame(cpu, started.elapsed(), rows);
            if n > 1 {
                // The focused pane is outlined (in high contrast, in the highlight colour).
                let c = theme::colors();
                ui.painter().rect_stroke(island.shrink(1.0), egui::CornerRadius::same(RADIUS as u8), egui::Stroke::new(2.0_f32, c.accent_fill), egui::StrokeKind::Inside);
            }

            if let Some(popup) = self.profile_menu(ctx) {
                list.push(Island::new(popup, 14.0));
            }
            list.extend(shellman_ui::hover::islands(ctx));
            if let Some(shape) = islands::solid_window(screen) {
                ui.painter().set(page, shape);
            }
            if !self.panel.composed() {
                ui.painter().set(under, egui::Shape::Vec(islands::under(screen, &list, &self.panel.style())));
            }
        });
        self.panel.set_islands(&list, ctx.pixels_per_point());
    }

    /// Enough renderers for `n` panes (made at the first one's size and font).
    fn ensure_slots(&mut self, n: usize) {
        while self.slots.len() < n {
            match Renderer::new(self.slots[0].renderer.px(), &self.config.font_family) {
                Ok(r) => self.slots.push(Slot::new(r)),
                Err(e) => {
                    eprintln!("terminal: {e}");
                    return;
                }
            }
        }
    }

    /// The gaps between panes: dragging one moves it (the cursor says so on hover). `room` is
    /// where the panes go.
    fn dividers(&mut self, ui: &mut egui::Ui, room: Rect) {
        let dividers = self.pane_layout.dividers.clone();
        let mut moved = false;
        for d in &dividers {
            let id = egui::Id::new(("terminal-divider", d.path.clone()));
            let resp = ui.interact(d.gap, id, egui::Sense::drag());
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(match d.axis {
                    Axis::Across => egui::CursorIcon::ResizeColumn,
                    Axis::Down => egui::CursorIcon::ResizeRow,
                });
            }
            if resp.drag_started() {
                self.dragging = Some(d.path.clone());
            }
            if self.dragging.as_ref() == Some(&d.path) {
                if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                    self.tabs.current_mut().panes.drag(d, p);
                    moved = true;
                }
            }
        }
        if ui.input(|i| !i.pointer.primary_down()) {
            self.dragging = None;
        }
        if moved {
            self.pane_layout = self.tabs.current().panes.layout(room);
        }
    }

    /// A press on a pane other than the focused one focuses it.
    fn focus_pressed(&mut self, ctx: &egui::Context) {
        if self.strip.menu_open() || self.placed.len() < 2 {
            return;
        }
        let pressed = ctx.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten());
        let Some(pos) = pressed else { return };
        if !ctx.layer_id_at(pos).is_none_or(|l| l.order == egui::Order::Background) {
            return;
        }
        if let Some(i) = self.placed.iter().position(|p| p.island.contains(pos)) {
            self.focus_pane(i);
        }
    }

    /// Each pane is a widget a screen reader names ("Terminal, Windows PowerShell", with its
    /// place when there are several); presses on it are the terminal's, not the gaps' (which
    /// move the window). The focused pane holds egui's keyboard focus, with Tab, the arrows and
    /// Esc locked to it: otherwise Tab (completion, in a console that is not sent whole keys)
    /// would move egui's focus onto a tab or button, and the next Enter would press it. Ctrl+F6
    /// lets go of it on purpose (`focus_chrome`).
    fn name_panes(&mut self, ui: &mut egui::Ui) {
        let tab = self.tabs.current();
        let n = self.placed.len();
        let focus = tab.panes.focus();
        let held = ui.memory(|m| m.focused());
        let ids: Vec<egui::Id> = tab.panes.iter().map(|p| pane_id(p.id)).collect();
        for (i, (p, placed)) in tab.panes.iter().zip(&self.placed).enumerate() {
            let title = strip::tab_title(&p.session);
            let name = if n > 1 { format!("Terminal pane {} of {n}, {title}", i + 1) } else { format!("Terminal, {title}") };
            let id = ids[i];
            let resp = ui.interact(placed.island, id, egui::Sense::click_and_drag());
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Other, true, i == focus, &name));
            if i == focus {
                if held.is_none_or(|h| ids.contains(&h) && h != id) && !self.strip.menu_open() {
                    resp.request_focus();
                }
                let all = egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true };
                ui.memory_mut(|m| m.set_focus_lock_filter(id, all));
            }
        }
    }

    /// Whether a tab or button has the keyboard (Ctrl+F6): keys are egui's then, not the shell's.
    pub(super) fn chrome_has_focus(&self, ctx: &egui::Context) -> bool {
        let pane = pane_id(self.tabs.current().pane().id);
        ctx.memory(|m| m.focused()).is_some_and(|f| f != pane) && !self.find.has_focus(ctx)
    }

    /// Ctrl+F6: give the keyboard to the shown tab in the strip (Tab goes on to the buttons).
    fn focus_chrome(&mut self, ctx: &egui::Context) {
        let pane = pane_id(self.tabs.current().pane().id);
        ctx.memory_mut(|m| {
            m.surrender_focus(pane);
            m.request_focus(egui::Id::new(("term-tab", self.tabs.current().id)));
        });
    }

    /// The next step of `SHELLMAN_TERMINAL_SCRIPT`, once the panes are laid out (a frame apart,
    /// so each step sees the last one's layout).
    fn run_script(&mut self, ctx: &egui::Context) {
        if self.pane_layout.panes.is_empty() {
            return;
        }
        let Some(step) = self.script.pop_front() else { return };
        match step {
            script::Step::Key(s) => self.run(ctx, s),
            script::Step::Type(t) => self.tabs.current().pane().session.write(t.into_bytes()),
            script::Step::Find(q) => {
                self.find.show();
                self.find.query = q;
                self.find.changed_by_script();
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(300));
    }

    /// Find's pill: its contents, what was asked of it, and counting on, in the focused pane.
    fn find_pill(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let out = find::pill(ui, rect, &mut self.find);
        if out.close {
            self.find.close();
            self.tabs.current().pane().session.mark_dirty();
            return;
        }
        let pane = self.tabs.current().pane();
        let (id, seq) = (pane.id, pane.session.output_seq());
        let generation = self.find.generation;
        let find = &mut self.find;
        let more = pane.session.with_term_quiet(|t| {
            let more = find.step(t, id, seq, out.changed);
            if let Some(up) = out.go {
                find.go(t, up);
            }
            more
        });
        // The pill drew the count before this step: one more frame shows what it found.
        if more || self.find.generation != generation {
            ui.ctx().request_repaint();
        } else if let Some(due) = self.find.recount_due(seq) {
            ui.ctx().request_repaint_after(due);
        }
    }

    /// The renderers' cell size and font metrics (all alike).
    pub(super) fn metrics(&self) -> crate::render::Metrics {
        self.slots[0].renderer.metrics()
    }

    /// Fit pane `i`'s grid to `room`: as many whole cells as fit, the grid starting on a whole
    /// pixel. Returns the grid's rectangle in points.
    fn fit(&mut self, i: usize, room: Rect, ppp: f32) -> Rect {
        let m = self.metrics();
        let left = (room.left() * ppp).ceil();
        let top = (room.top() * ppp).ceil();
        let cols = (((room.right() * ppp).floor() - left) as i32 / m.cell_w).max(2);
        let rows = (((room.bottom() * ppp).floor() - top) as i32 / m.cell_h).max(1);
        let size = Size { cols: cols as u16, rows: rows as u16, cell_w: m.cell_w as u16, cell_h: m.cell_h as u16 };
        if let Some(p) = self.tabs.current_mut().panes.iter_mut().nth(i) {
            p.session.resize(size);
        }
        Rect::from_min_size(pos2(left / ppp, top / ppp), vec2((cols * m.cell_w) as f32 / ppp, (rows * m.cell_h) as f32 / ppp))
    }
}

/// A pane's widget id (its focus and screen-reader node).
fn pane_id(id: u64) -> egui::Id {
    egui::Id::new(("terminal-pane", id))
}

/// The open pane whose id is `id`, in any tab: where input typed into it goes. None once it
/// closed (input for it is dropped, never sent to whichever pane has the focus now).
fn typing_pane(tabs: &Tabs<Tab>, id: u64) -> Option<&Pane> {
    tabs.iter().flat_map(|t| t.panes.iter()).find(|p| p.id == id)
}

/// The renderer's cell size in pixels, which a new shell starts sized by.
fn cell_size(renderer: &Renderer) -> (u16, u16) {
    let m = renderer.metrics();
    (m.cell_w as u16, m.cell_h as u16)
}

/// Start a shell sized like the shown grid (fixed on its first frame). A folder that is not
/// there (or not a folder) is found by CreateProcess itself, not probed beforehand: the shell
/// then starts in home instead.
fn spawn(command: &str, dir: Option<PathBuf>, cell: (u16, u16), config: &Config, wake: Arc<dyn Fn() + Send + Sync>) -> std::io::Result<Session> {
    let size = Size { cols: 80, rows: 24, cell_w: cell.0, cell_h: cell.1 };
    let fallback = dir.as_ref().map(|_| home()).filter(|h| Some(h) != dir.as_ref());
    match Session::start(Start { command, dir, size, scrollback: config.scrollback }, wake.clone()) {
        Err(e) if fallback.is_some() => {
            eprintln!("terminal: {e}; starting in the home folder");
            Session::start(Start { command, dir: fallback, size, scrollback: config.scrollback }, wake)
        }
        started => started,
    }
}

/// What a tab for `launch` runs and where, and the WSL distribution it is: its command, else
/// its profile's shell (the default when none is named or the name is unknown), in its folder,
/// else `cwd` (the shown tab's reported directory, with the distribution that tab runs), else
/// the folder this was started in, else home.
fn plan(profiles: &[Profile], config: &Config, launch: &Launch, cwd: Option<(String, Option<String>)>) -> (String, PathBuf, Option<String>) {
    let started = start_dir();
    let dir = match (&launch.dir, &cwd, &started) {
        (Some(d), _, _) => profiles::Dir::Given(d),
        (None, Some((path, distro)), _) => profiles::Dir::Reported { path, distro: distro.as_deref() },
        (None, None, Some(d)) => profiles::Dir::Given(d),
        (None, None, None) => profiles::Dir::None,
    };
    match &launch.command {
        Some(command) => {
            // Any shell can run a command: only a Windows folder is somewhere to start it.
            let p = profiles::pick(profiles, "Command Prompt");
            let (_, dir) = p.launch(dir, &home());
            (command.clone(), dir, None)
        }
        None => {
            let p = profiles::pick(profiles, launch.profile.as_deref().unwrap_or(&config.default_profile));
            let (command, dir) = p.launch(dir, &home());
            (command, dir, p.distro().map(str::to_owned))
        }
    }
}

/// Show a handed-over console in a tab sized like the shown grid (fixed on its first frame).
fn attach(arrived: Arrived, cell: (u16, u16), config: &Config, wake: Arc<dyn Fn() + Send + Sync>) -> std::io::Result<Session> {
    let size = Size { cols: 80, rows: 24, cell_w: cell.0, cell_h: cell.1 };
    // A console's title is often its exe's whole path: shortened as a program's title is.
    let name = arrived.title.map(|t| crate::session::short_title(&t)).unwrap_or(arrived.program);
    let pty = Pty::handoff(arrived.handles, size.into())?;
    Session::attach(pty, name, size, config.scrollback, wake)
}

/// A console started hidden: no window. Its output is read (so it never blocks) and dropped,
/// until its program ends.
pub fn run_hidden(arrived: Arrived) {
    use alacritty_terminal::event::WindowSize;
    use alacritty_terminal::tty::{EventedPty, EventedReadWrite};
    use std::io::Read;
    let size = WindowSize { num_lines: 24, num_cols: 80, cell_width: 1, cell_height: 1 };
    let Ok(mut pty) = Pty::handoff(arrived.handles, size) else { return };
    let mut sink = [0u8; 64 * 1024];
    loop {
        while pty.reader().read(&mut sink).unwrap_or(0) > 0 {}
        if pty.next_child_event().is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\"))
}

/// The directory the first tab starts in: the one this was started in, unless that is only
/// where Windows starts programs (the system folder, or the exe's own).
pub fn start_dir() -> Option<PathBuf> {
    let here = std::env::current_dir().ok()?;
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf()));
    let system = std::env::var_os("SystemRoot").map(|r| PathBuf::from(r).join("System32"));
    let same = |a: &PathBuf, b: &Option<PathBuf>| b.as_ref().is_some_and(|b| a.as_os_str().eq_ignore_ascii_case(b.as_os_str()));
    (!same(&here, &exe_dir) && !same(&here, &system)).then_some(here)
}

impl eframe::App for TerminalApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // This frame's keys were typed into the tab shown now; closing tabs and tabs handed
        // over may show another before they are sent.
        let typed_into = self.tabs.current().pane().id;
        if !self.reap(ctx) {
            return;
        }
        self.take_handed_over(ctx);
        self.track_system(ctx);
        self.track_focus(ctx);
        shellman_ui::app_icon::sync_window_icon(ctx, shellman_ui::app_icon::Kind::Shellman);
        self.input(ctx, typed_into);
        self.track_tabs(ctx);
        let cpu = self.stats.on().then(|| frame.info().cpu_usage).flatten();
        self.layout(ctx, cpu);
        self.gate_keys(ctx);
        self.run_script(ctx);
        shellman_ui::timing::content(ctx, "first frame", 0);
        // The shell's first bytes (its prompt) are drawn in this frame.
        if self.tabs.current().session.output_seq() > 0 {
            shellman_ui::timing::content(ctx, "shell prompt", 0);
        }
    }

    fn on_exit(&mut self, gl: Option<&egui_glow::glow::Context>) {
        if let Some(gl) = gl {
            for slot in &self.slots {
                slot.renderer.destroy(gl);
            }
        }
    }
}
