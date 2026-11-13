//! The window: a title pill on top (room for a tab strip later) and the terminal island below,
//! the desktop showing between them. egui draws the chrome; the grid is drawn by the renderer
//! inside the island, through a paint callback.
//!
//! A frame happens only when something changed: new output (the pty thread wakes the window),
//! input, a resize, or the theme. Nothing blinks, so an idle terminal draws nothing.

use crate::keys::{self, Key, Local, Mods};
use crate::palette::Palette;
use crate::render::{Frame, Renderer};
use crate::session::{Session, Size};
use crate::snapshot::{self, Look};
use crate::stats::Stats;
use alacritty_terminal::term::TermMode;
use egui::{pos2, vec2, Align2, Rect};
use shellman_ui::appearance::Appearance;
use shellman_ui::chips;
use shellman_ui::glass::islands::{self, Island};
use shellman_ui::glass::{GlassPanel, Style};
use shellman_ui::theme::{self, colors, semibold_font, AccentChoice};
use std::sync::Arc;
use std::time::Instant;

/// Space between the window's edge and the islands, and between islands.
const MARGIN: f32 = 12.0;
const TITLE_H: f32 = 46.0;
const BUTTON_H: f32 = 30.0;
/// The terminal island's corner radius.
pub const RADIUS: f32 = 22.0;
/// Space between the island's edge and the grid. More than the corner's inset (about 0.3 of the
/// radius), so the grid's square corners never reach the rounded ones.
const PADDING: f32 = 14.0;
/// The grid font's size in points (Windows Terminal's default, 12 pt).
const FONT_PT: f32 = 12.0;

pub struct TerminalApp {
    session: Session,
    renderer: Renderer,
    frame: Frame,
    panel: GlassPanel,
    look: Appearance,
    /// What the snapshot was last taken with, to take it again when it changes.
    drawn_with: Option<(bool, egui::Color32, bool)>,
    /// A key pressed with Alt whose text has not come yet: the text gets ESC before it.
    alt_key: Option<char>,
    /// Wheel movement not yet a whole line, in lines.
    wheel: f32,
    /// Wakes the window when Settings saves the look (glass, accent), to follow it live.
    look_watch: shellman_core::watch::Watcher,
    stats: Stats,
}

impl TerminalApp {
    pub fn new(ctx: &egui::Context, look: Appearance, panel: GlassPanel, command: &str) -> Result<Self, String> {
        let ppp = ctx.pixels_per_point();
        let renderer = Renderer::new(font_px(ppp))?;
        let m = renderer.metrics();
        // A first size; the first frame fits it to the window.
        let size = Size { cols: 80, rows: 24, cell_w: m.cell_w as u16, cell_h: m.cell_h as u16 };
        let wake_ctx = ctx.clone();
        let wake = Arc::new(move || wake_ctx.request_repaint());
        let session = Session::start(command, size, wake).map_err(|e| format!("could not start {command}: {e}"))?;
        let look_watch = shellman_core::watch::Watcher::quick(ctx.clone());
        if let Some(dir) = shellman_ui::appearance::file().and_then(|f| f.parent().map(|p| p.to_path_buf())) {
            look_watch.set_dirs(vec![dir]);
        }
        Ok(Self {
            session,
            renderer,
            frame: Frame { cols: 0, rows: 0, cells: Vec::new(), cursor: None },
            panel,
            look,
            drawn_with: None,
            alt_key: None,
            wheel: 0.0,
            look_watch,
            stats: Stats::from_env(),
        })
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

    /// Keys, typed text and pastes go to the shell.
    fn input(&mut self, ctx: &egui::Context) {
        let (events, mode) = (ctx.input(|i| i.events.clone()), self.session.mode());
        let app_cursor = mode.contains(TermMode::APP_CURSOR);
        let bracketed = mode.contains(TermMode::BRACKETED_PASTE);
        let mut out = Vec::new();
        for event in events {
            match event {
                egui::Event::Text(text) => {
                    let alt = self.alt_key.take().is_some();
                    out.extend(keys::text_bytes(&text, alt));
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    // A key whose Alt text never came (Windows sends none for some): ESC and
                    // the key itself.
                    if let Some(c) = self.alt_key.take() {
                        out.extend(keys::text_bytes(&c.to_string(), true));
                    }
                    let mods = Mods { shift: modifiers.shift, ctrl: modifiers.ctrl, alt: modifiers.alt };
                    let Some(k) = map_key(key, mods) else { continue };
                    if let Key::Char(c) = k {
                        if mods.alt && !mods.ctrl {
                            self.alt_key = Some(if mods.shift { c } else { c.to_ascii_lowercase() });
                            continue;
                        }
                    }
                    match keys::local(k, mods) {
                        Some(Local::ScrollPage(dir)) => {
                            self.session.scroll(dir * (self.session.size().rows as i32 - 1).max(1));
                        }

                        None => out.extend(keys::key_bytes(k, mods, app_cursor).unwrap_or_default()),
                    }
                }
                egui::Event::Paste(text) => out.extend(keys::paste_bytes(&text, bracketed)),
                // egui turns Ctrl+C and Ctrl+X into copy and cut; the shell wants them as keys.
                // (Selection and copying come later.)
                egui::Event::Copy => {
                    let m = ctx.input(|i| i.modifiers);
                    if m.ctrl && !m.shift {
                        out.push(0x03);
                    }
                }
                egui::Event::Cut => {
                    let m = ctx.input(|i| i.modifiers);
                    if m.ctrl {
                        out.push(0x18);
                    } else {
                        // Shift+Delete.
                        out.extend(keys::key_bytes(Key::Delete, Mods { shift: true, ..Mods::NONE }, app_cursor).unwrap_or_default());
                    }
                }
                _ => {}
            }
        }
        if let Some(c) = self.alt_key.take() {
            out.extend(keys::text_bytes(&c.to_string(), true));
        }
        self.session.write(out);
    }

    /// The wheel moves through the history; on the alternate screen (`less`, `vim`) it sends
    /// arrow keys instead, as other terminals do.
    fn wheel(&mut self, ctx: &egui::Context, island: Rect, cell_h: f32) {
        let over = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| island.contains(p));
        // Read from the events, not egui's scroll delta, which turns Shift+wheel sideways.
        let lines: f32 = ctx.input(|i| {
            i.events
                .iter()
                .map(|e| match e {
                    // A notch is three lines, as in Windows.
                    egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta, .. } => delta.y * 3.0,
                    egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, .. } => delta.y / cell_h,
                    _ => 0.0,
                })
                .sum()
        });
        if !over || lines == 0.0 {
            return;
        }
        self.wheel += lines;
        let lines = self.wheel.trunc() as i32;
        self.wheel -= lines as f32;
        if lines == 0 {
            return;
        }
        let mode = self.session.mode();
        if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
            let key = if lines > 0 { Key::Up } else { Key::Down };
            let bytes = keys::key_bytes(key, Mods::NONE, mode.contains(TermMode::APP_CURSOR)).unwrap_or_default();
            self.session.write(bytes.repeat(lines.unsigned_abs() as usize));
        } else {
            self.session.scroll(lines);
        }
    }

    fn layout(&mut self, ctx: &egui::Context, cpu: Option<f32>) {
        self.panel.set_style(Style::from_theme(RADIUS));
        let ppp = ctx.pixels_per_point();
        if let Err(e) = self.renderer.set_px(font_px(ppp)) {
            eprintln!("terminal: {e}");
        }
        let mut list: Vec<Island> = Vec::new();
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
            let screen = ctx.screen_rect();
            let under = ui.painter().add(egui::Shape::Noop);
            islands::drag_gaps(ui, screen);

            let title = Rect::from_min_size(screen.min + vec2(MARGIN, MARGIN), vec2(screen.width() - 2.0 * MARGIN, TITLE_H));
            list.push(Island::pill(title));
            self.title(ui, title);

            let island = Rect::from_min_max(pos2(title.left(), title.bottom() + MARGIN), screen.max - vec2(MARGIN, MARGIN));
            list.push(Island::new(island, RADIUS));
            let grid = self.fit(island.shrink(PADDING), ppp);
            self.wheel(ctx, island, self.renderer.metrics().cell_h as f32 / ppp);
            // Clicks on the island are the terminal's, not the gaps' (which move the window).
            ui.interact(island, egui::Id::new("terminal-island"), egui::Sense::click());

            let started = Instant::now();
            let rows = self.redraw_if_changed(ctx);
            self.stats.frame(cpu, started.elapsed(), rows);
            ui.painter().add(self.renderer.paint(grid));

            if !self.panel.composed() {
                ui.painter().set(under, egui::Shape::Vec(islands::solid_shapes(&list, &self.panel.style())));
            }
        });
        self.panel.set_islands(&list, ctx.pixels_per_point());
    }

    /// Fit the grid to `room`: as many whole cells as fit, the grid starting on a whole pixel.
    /// Returns the grid's rectangle in points.
    fn fit(&mut self, room: Rect, ppp: f32) -> Rect {
        let m = self.renderer.metrics();
        let left = (room.left() * ppp).ceil();
        let top = (room.top() * ppp).ceil();
        let cols = (((room.right() * ppp).floor() - left) as i32 / m.cell_w).max(2);
        let rows = (((room.bottom() * ppp).floor() - top) as i32 / m.cell_h).max(1);
        self.session.resize(Size { cols: cols as u16, rows: rows as u16, cell_w: m.cell_w as u16, cell_h: m.cell_h as u16 });
        Rect::from_min_size(pos2(left / ppp, top / ppp), vec2((cols * m.cell_w) as f32 / ppp, (rows * m.cell_h) as f32 / ppp))
    }

    /// Take a new snapshot and send what changed, if the grid, its size or the look changed.
    fn redraw_if_changed(&mut self, ctx: &egui::Context) -> usize {
        let c = colors();
        let focused = ctx.input(|i| i.focused);
        let with = (c.light, c.accent, focused);
        let dirty = self.session.take_dirty();
        if !dirty && self.drawn_with == Some(with) && self.frame.cols != 0 {
            return 0;
        }
        self.drawn_with = Some(with);
        let rgb = |c: egui::Color32| [c.r(), c.g(), c.b()];
        let look = Look {
            palette: Palette::new(c.light, rgb(c.text), rgb(c.bg)),
            cursor: rgb(c.accent_fill),
            on_cursor: rgb(c.on_accent),
            focused,
        };
        self.session.with_term(|term| snapshot::take(term, &look, &mut self.frame));
        self.renderer.prepare(&self.frame)
    }

    /// The pill on top: "Terminal", what runs in it, and the window buttons.
    fn title(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let c = colors();
        let y = rect.center().y;
        let name = ui.painter().text(pos2(rect.left() + 22.0, y), Align2::LEFT_CENTER, "Terminal", semibold_font(15.0), c.text);
        let slash = ui.painter().text(pos2(name.right() + 10.0, y), Align2::LEFT_CENTER, "/", egui::FontId::proportional(15.0), c.muted);
        let buttons = Rect::from_min_max(pos2(rect.right() - 10.0 - BUTTON_H * 3.0, y - BUTTON_H / 2.0), pos2(rect.right() - 10.0, y + BUTTON_H / 2.0));
        let room = Rect::from_min_max(pos2(slash.right() + 10.0, rect.top()), pos2(buttons.left() - 12.0, rect.bottom()));
        shellman_ui::widgets::draw_text(ui, room, &self.session.title(), egui::FontId::proportional(14.0), c.muted, false);
        chips::window_buttons(ui, buttons);
    }
}

/// The grid font's size in pixels at `ppp` pixels per point.
fn font_px(ppp: f32) -> f32 {
    (FONT_PT * 96.0 / 72.0 * ppp).round()
}

/// egui's key as the terminal's: the keys with sequences of their own, and character keys
/// (which matter only with Ctrl or Alt; plain typing comes as text).
fn map_key(key: egui::Key, mods: Mods) -> Option<Key> {
    use egui::Key as E;
    Some(match key {
        E::Enter => Key::Enter,
        E::Backspace => Key::Backspace,
        E::Tab => Key::Tab,
        E::Escape => Key::Escape,
        // Plain Space types a space as text.
        E::Space if mods.ctrl => Key::Space,
        E::ArrowUp => Key::Up,
        E::ArrowDown => Key::Down,
        E::ArrowLeft => Key::Left,
        E::ArrowRight => Key::Right,
        E::Home => Key::Home,
        E::End => Key::End,
        E::PageUp => Key::PageUp,
        E::PageDown => Key::PageDown,
        E::Insert => Key::Insert,
        E::Delete => Key::Delete,
        E::F1 => Key::F(1),
        E::F2 => Key::F(2),
        E::F3 => Key::F(3),
        E::F4 => Key::F(4),
        E::F5 => Key::F(5),
        E::F6 => Key::F(6),
        E::F7 => Key::F(7),
        E::F8 => Key::F(8),
        E::F9 => Key::F(9),
        E::F10 => Key::F(10),
        E::F11 => Key::F(11),
        E::F12 => Key::F(12),
        _ if mods.ctrl || mods.alt => {
            let mut s = key.symbol_or_name().chars();
            match (s.next(), s.next()) {
                (Some(c), None) => Key::Char(c),
                _ => return None,
            }
        }
        _ => return None,
    })
}


impl eframe::App for TerminalApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.session.exited() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.track_system(ctx);
        shellman_ui::app_icon::sync_window_icon(ctx, shellman_ui::app_icon::Kind::Shellman);
        self.input(ctx);
        let cpu = self.stats.on().then(|| frame.info().cpu_usage).flatten();
        self.layout(ctx, cpu);
    }

    fn on_exit(&mut self, gl: Option<&egui_glow::glow::Context>) {
        if let Some(gl) = gl {
            self.renderer.destroy(gl);
        }
    }
}
