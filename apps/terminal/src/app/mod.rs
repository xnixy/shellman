//! The window: a pill of tabs and a pill of window buttons on top, and the terminal island below,
//! the desktop showing between them. egui draws the chrome; the grid is drawn by the renderer
//! inside the island, through a paint callback.
//!
//! Each tab owns a shell (`Session`); one renderer draws whichever is shown. A frame happens only
//! when something changed: new output in the shown tab (its pty thread wakes the window), input,
//! a resize, the theme, or the cursor's blink (a timer, only while a program asked for a blinking
//! cursor and the window has the focus). A tab not shown wakes the window only for its dot,
//! title, bell or exit, so it costs no frames while it runs.

mod find;
mod input;
mod pointer;
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

pub struct Tab {
    id: u64,
    session: Session,
    /// Stays open after its program exits (`--hold`); `held` once that was said in it.
    hold: bool,
    held: bool,
}

pub struct TerminalApp {
    tabs: Tabs<Tab>,
    next_id: u64,
    profiles: Vec<Profile>,
    config: Config,
    /// The grid font's size in points, as zoomed.
    font_pt: f32,
    renderer: Renderer,
    frame: Frame,
    panel: GlassPanel,
    look: Appearance,
    /// Wakes the window; every tab's pty thread holds one.
    wake: Arc<dyn Fn() + Send + Sync>,
    view: view::View,
    keys: input::KeyState,
    pointer: pointer::Pointer,
    strip: strip::StripState,
    /// Where the grid was drawn last frame, in points.
    grid: Rect,
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
}

impl TerminalApp {
    /// `launch`: what the first tab runs, and where; or `handoff`, a console Windows handed over.
    pub fn new(ctx: &egui::Context, look: Appearance, panel: GlassPanel, hwnd: Option<isize>, launch: Launch, handoff: Option<Arrived>) -> Result<Self, String> {
        let config = if handoff.is_some() { Config::read() } else { Config::load() };
        let profiles = profiles::build(&profiles::detect());
        let ppp = ctx.pixels_per_point();
        let font_pt = config.font_size;
        let renderer = Renderer::new(view::font_px(font_pt, ppp), &config.font_family)?;
        let wake_ctx = ctx.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || wake_ctx.request_repaint());
        let look_watch = shellman_core::watch::Watcher::quick(ctx.clone());
        if let Some(dir) = shellman_ui::appearance::file().and_then(|f| f.parent().map(|p| p.to_path_buf())) {
            look_watch.set_dirs(vec![dir]);
        }
        let minimized = handoff.as_ref().is_some_and(Arrived::minimized);
        let first = match handoff {
            Some(arrived) => attach(arrived, &renderer, &config, wake.clone()).map_err(|e| format!("could not show the console: {e}"))?,
            None => {
                let (command, dir) = plan(&profiles, &config, &launch, None);
                spawn(&command, Some(dir), &renderer, &config, wake.clone()).map_err(|e| format!("could not start {command}: {e}"))?
            }
        };
        if minimized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        let inbox = instance::listen(wake.clone());
        Ok(Self {
            tabs: Tabs::new(Tab { id: 0, session: first, hold: launch.hold && launch.command.is_some(), held: false }),
            next_id: 1,
            profiles,
            font_pt,
            renderer,
            frame: Frame { cols: 0, rows: 0, cells: Vec::new(), cursor: None, blinking: false },
            panel,
            look,
            wake,
            view: view::View::new(),
            keys: input::KeyState::default(),
            pointer: pointer::Pointer::default(),
            strip: strip::StripState::default(),
            grid: Rect::NOTHING,
            focused: true,
            window_title: String::new(),
            look_watch,
            inbox,
            stats: Stats::from_env(),
            config,
            win32_keys: hwnd.map(|h| win32_input::install(h, ctx.clone())),
            find: find::Find::default(),
        })
    }

    /// Open a tab running `profile` (an index into the profiles; None for the default), in the
    /// shown tab's directory when its shell reports one.
    fn new_tab(&mut self, profile: Option<usize>) {
        let p = match profile.and_then(|i| self.profiles.get(i)) {
            Some(p) => p,
            None => profiles::pick(&self.profiles, &self.config.default_profile),
        };
        let cwd = self.tabs.current().session.cwd();
        let (command, dir) = p.launch(cwd.as_deref(), &home());
        self.open_tab(&command, dir, false);
    }

    /// Open a tab running `command` in `dir`, right after the shown one, and show it.
    fn open_tab(&mut self, command: &str, dir: PathBuf, hold: bool) {
        match spawn(command, Some(dir), &self.renderer, &self.config, self.wake.clone()) {
            Ok(session) => {
                let id = self.next_id;
                self.next_id += 1;
                self.leave_tab();
                self.tabs.open(Tab { id, session, hold, held: false });
                self.enter_tab();
            }
            Err(e) => eprintln!("terminal: could not start {command}: {e}"),
        }
    }

    /// Tabs another start of the terminal handed over: each opens, and the window comes forward.
    fn take_handed_over(&mut self, ctx: &egui::Context) {
        let launches: Vec<Launch> = std::mem::take(&mut *self.inbox.lock().unwrap());
        if launches.is_empty() {
            return;
        }
        for launch in launches {
            let cwd = self.tabs.current().session.cwd();
            let (command, dir) = plan(&self.profiles, &self.config, &launch, cwd);
            self.open_tab(&command, dir, launch.hold && launch.command.is_some());
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
        let s = &self.tabs.current().session;
        s.set_shown(false);
        if self.focused && s.mode().contains(alacritty_terminal::term::TermMode::FOCUS_IN_OUT) {
            s.report(crate::mouse::focus(false));
        }
        self.pointer.reset();
    }

    /// A tab was just shown.
    fn enter_tab(&mut self) {
        let s = &self.tabs.current().session;
        s.set_shown(true);
        if self.focused && s.mode().contains(alacritty_terminal::term::TermMode::FOCUS_IN_OUT) {
            s.report(crate::mouse::focus(true));
        }
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
            let s = &self.tabs.current().session;
            let active = s.win32_input() && !self.strip.menu_open() && !self.find.has_focus(ctx);
            gate.set(active, active && s.with_term(crate::select::has));
        }
    }

    /// Tabs whose shell exited close; the window closes with the last.
    fn reap(&mut self, ctx: &egui::Context) -> bool {
        if !self.tabs.iter().any(|t| t.session.exited() && !t.held) {
            return true;
        }
        // A held tab says its program ended, and stays.
        for t in self.tabs.iter_mut().filter(|t| t.hold && !t.held && t.session.exited()) {
            t.held = true;
            let code = t.session.exit_code().map_or_else(|| "?".to_owned(), |c| c.to_string());
            t.session.print(&format!("\r\n\x1b[0m[process exited with code {code}; Ctrl+Shift+W closes this tab]"));
        }
        let shown = self.tabs.current().id;
        let left = self.tabs.retain(|t| t.hold || !t.session.exited());
        if !left {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return false;
        }
        if self.tabs.current().id != shown {
            self.enter_tab();
        }
        true
    }

    /// Focus in and out, to the program in the shown tab when it asked (DECSET 1004).
    fn track_focus(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|i| i.focused);
        if focused != self.focused {
            self.focused = focused;
            let s = &self.tabs.current().session;
            if s.mode().contains(alacritty_terminal::term::TermMode::FOCUS_IN_OUT) {
                s.report(crate::mouse::focus(focused));
            }
            self.view.restart_blink();
        }
    }

    /// OSC 52 copies, bells, and the window's title.
    fn track_tabs(&mut self, ctx: &egui::Context) {
        for t in self.tabs.iter() {
            if let Some(text) = t.session.take_copy() {
                ctx.copy_text(text);
            }
        }
        let s = &self.tabs.current().session;
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
        if let Err(e) = self.renderer.set_px(view::font_px(self.font_pt, ppp)) {
            eprintln!("terminal: {e}");
        }
        let mut list: Vec<Island> = Vec::new();
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
            let screen = ctx.screen_rect();
            let under = ui.painter().add(egui::Shape::Noop);
            islands::drag_gaps(ui, screen);

            let top = screen.top() + MARGIN;
            let right = screen.right() - MARGIN;
            let controls = Rect::from_min_max(pos2(right - CONTROLS_W, top), pos2(right, top + TABS_H));
            let mut row = Rect::from_min_max(pos2(screen.left() + MARGIN, top), pos2(controls.left() - MARGIN, top + TABS_H));
            // Find's pill, in the row between the tabs and the window buttons.
            if self.find.open {
                let w = find::WIDTH.min(row.width() * 0.6);
                let pill = Rect::from_min_max(pos2(row.right() - w, top), row.max);
                row.max.x = pill.left() - MARGIN;
                list.push(Island::pill(pill));
                self.find_pill(ui, pill);
            }
            let pill = self.tab_strip(ui, row);
            list.push(Island::pill(pill));
            list.push(Island::pill(controls));
            let buttons = Rect::from_center_size(controls.center(), vec2(BUTTON_H * 3.0, BUTTON_H));
            shellman_ui::chips::window_buttons(ui, buttons);

            let island = Rect::from_min_max(pos2(screen.left() + MARGIN, controls.bottom() + MARGIN), screen.max - vec2(MARGIN, MARGIN));
            list.push(Island::new(island, RADIUS));
            self.grid = self.fit(island.shrink(PADDING), ppp);
            // Presses on the island are the terminal's, not the gaps' (which move the window).
            ui.interact(island, egui::Id::new("terminal-island"), egui::Sense::click_and_drag());
            self.pointer(ctx, island);

            let started = Instant::now();
            let rows = self.redraw_if_changed(ctx);
            self.stats.frame(cpu, started.elapsed(), rows);
            ui.painter().add(self.renderer.paint(self.grid));

            if let Some(popup) = self.profile_menu(ctx) {
                list.push(Island::new(popup, 14.0));
            }
            list.extend(shellman_ui::hover::islands(ctx));
            if !self.panel.composed() {
                ui.painter().set(under, egui::Shape::Vec(islands::solid_shapes(&list, &self.panel.style())));
            }
        });
        self.panel.set_islands(&list, ctx.pixels_per_point());
    }

    /// Find's pill: its contents, what was asked of it, and counting on.
    fn find_pill(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let out = find::pill(ui, rect, &mut self.find);
        if out.close {
            self.find.close();
            self.tabs.current().session.mark_dirty();
            return;
        }
        let tab = self.tabs.current();
        let (id, seq) = (tab.id, tab.session.output_seq());
        let find = &mut self.find;
        let more = tab.session.with_term_quiet(|t| {
            let more = find.step(t, id, seq, out.changed);
            if let Some(up) = out.go {
                find.go(t, up);
            }
            more
        });
        if more {
            ui.ctx().request_repaint();
        }
    }

    /// Fit the grid to `room`: as many whole cells as fit, the grid starting on a whole pixel.
    /// Returns the grid's rectangle in points.
    fn fit(&mut self, room: Rect, ppp: f32) -> Rect {
        let m = self.renderer.metrics();
        let left = (room.left() * ppp).ceil();
        let top = (room.top() * ppp).ceil();
        let cols = (((room.right() * ppp).floor() - left) as i32 / m.cell_w).max(2);
        let rows = (((room.bottom() * ppp).floor() - top) as i32 / m.cell_h).max(1);
        let size = Size { cols: cols as u16, rows: rows as u16, cell_w: m.cell_w as u16, cell_h: m.cell_h as u16 };
        self.tabs.current_mut().session.resize(size);
        Rect::from_min_size(pos2(left / ppp, top / ppp), vec2((cols * m.cell_w) as f32 / ppp, (rows * m.cell_h) as f32 / ppp))
    }
}

/// Start a shell sized like the shown grid (fixed on its first frame).
fn spawn(command: &str, dir: Option<PathBuf>, renderer: &Renderer, config: &Config, wake: Arc<dyn Fn() + Send + Sync>) -> std::io::Result<Session> {
    let m = renderer.metrics();
    let size = Size { cols: 80, rows: 24, cell_w: m.cell_w as u16, cell_h: m.cell_h as u16 };
    Session::start(Start { command, dir, size, scrollback: config.scrollback }, wake)
}

/// What a tab for `launch` runs and where: its command, else its profile's shell (the default
/// when none is named or the name is unknown), in its folder, else `cwd` (the shown tab's), else
/// the folder this was started in, else home.
fn plan(profiles: &[Profile], config: &Config, launch: &Launch, cwd: Option<String>) -> (String, PathBuf) {
    let here = launch.dir.as_ref().map(|d| d.display().to_string()).or(cwd).or_else(|| start_dir().map(|d| d.display().to_string()));
    match &launch.command {
        Some(command) => {
            let dir = here.map(PathBuf::from).filter(|d| d.is_dir()).unwrap_or_else(home);
            (command.clone(), dir)
        }
        None => {
            let p = profiles::pick(profiles, launch.profile.as_deref().unwrap_or(&config.default_profile));
            p.launch(here.as_deref(), &home())
        }
    }
}

/// Show a handed-over console in a tab sized like the shown grid (fixed on its first frame).
fn attach(arrived: Arrived, renderer: &Renderer, config: &Config, wake: Arc<dyn Fn() + Send + Sync>) -> std::io::Result<Session> {
    let m = renderer.metrics();
    let size = Size { cols: 80, rows: 24, cell_w: m.cell_w as u16, cell_h: m.cell_h as u16 };
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
        if !self.reap(ctx) {
            return;
        }
        self.take_handed_over(ctx);
        self.track_system(ctx);
        self.track_focus(ctx);
        shellman_ui::app_icon::sync_window_icon(ctx, shellman_ui::app_icon::Kind::Shellman);
        self.input(ctx);
        self.track_tabs(ctx);
        let cpu = self.stats.on().then(|| frame.info().cpu_usage).flatten();
        self.layout(ctx, cpu);
        self.gate_keys(ctx);
    }

    fn on_exit(&mut self, gl: Option<&egui_glow::glow::Context>) {
        if let Some(gl) = gl {
            self.renderer.destroy(gl);
        }
    }
}
