//! The mouse over the grid: selecting (drag, double-click words, triple-click lines, Shift+click
//! extends, Alt+drag a block), copying and pasting with the right button, links (Ctrl+click),
//! the wheel, and reports to programs that track the mouse (Shift takes it back from them).

use crate::keys::Mods;
use crate::mouse::{self, Action, Button, Modes};
use crate::select::{self, Kind};
use crate::snapshot::Hover;
use alacritty_terminal::index::Side;
use alacritty_terminal::term::TermMode;
use egui::{Pos2, Rect};
use std::time::{Duration, Instant};

/// A button held down on the grid.
#[derive(Clone, Copy)]
struct Down {
    button: Button,
    /// The press went to the program, so moves and the release do too.
    reported: bool,
    /// The press started a selection that moves follow.
    selecting: bool,
}

#[derive(Default)]
pub struct Pointer {
    down: Option<Down>,
    /// The last left press: when, where, and how many clicks it made in a row.
    last_click: Option<(Instant, (usize, usize), u8)>,
    /// The cell of the last motion reported, so a report goes only when it changes.
    reported_cell: Option<(usize, usize)>,
    /// Wheel movement not yet a whole line, in lines.
    wheel: f32,
    /// The link under the pointer and where to open it.
    link: Option<String>,
    /// The hover label naming the OSC 8 link under the pointer (its id), while one shows.
    label: Option<egui::Id>,
}

impl Pointer {
    /// Forget a press (the tab changed under it).
    pub fn reset(&mut self) {
        self.down = None;
        self.reported_cell = None;
        self.link = None;
    }
}

/// Where a point falls on the grid: the cell (held inside it), which half of the cell, and how
/// far above (negative) or below (positive) the grid it is, in rows.
struct Spot {
    col: usize,
    row: usize,
    side: Side,
    beyond: i32,
}

impl super::TerminalApp {
    /// The cell under `pos` in the shown grid.
    fn spot(&self, ctx: &egui::Context, pos: Pos2) -> Spot {
        let ppp = ctx.pixels_per_point();
        let m = self.renderer.metrics();
        let size = self.tabs.current().session.size();
        let x = ((pos.x - self.grid.left()) * ppp).max(0.0);
        let y = (pos.y - self.grid.top()) * ppp;
        let col = ((x / m.cell_w as f32) as usize).min(size.cols as usize - 1);
        let side = if x - (col as i32 * m.cell_w) as f32 >= m.cell_w as f32 / 2.0 { Side::Right } else { Side::Left };
        let r = (y / m.cell_h as f32).floor() as i32;
        let rows = size.rows as i32;
        let beyond = if r < 0 { r } else if r >= rows { r - rows + 1 } else { 0 };
        Spot { col, row: r.clamp(0, rows - 1) as usize, side, beyond }
    }

    fn modes(&self) -> Modes {
        let s = &self.tabs.current().session;
        let m = s.mode();
        Modes {
            x10: s.x10(),
            click: m.contains(TermMode::MOUSE_REPORT_CLICK),
            drag: m.contains(TermMode::MOUSE_DRAG),
            motion: m.contains(TermMode::MOUSE_MOTION),
            sgr: m.contains(TermMode::SGR_MOUSE),
            utf8: m.contains(TermMode::UTF8_MOUSE),
        }
    }

    fn send_report(&mut self, button: Button, action: Action, mods: Mods, spot: &Spot) {
        let modes = self.modes();
        if modes.wants(button, action, self.pointer.down.is_some()) {
            if let Some(bytes) = mouse::report(button, action, mods, spot.col, spot.row, &modes) {
                self.tabs.current().session.report(&bytes);
            }
        }
    }

    /// Everything the mouse did this frame over the island.
    pub(super) fn pointer(&mut self, ctx: &egui::Context, island: Rect) {
        if self.strip.menu_open() || self.grid.width() <= 0.0 {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        let modes = self.modes();
        for e in &events {
            match *e {
                egui::Event::PointerButton { pos, button, pressed, modifiers } => {
                    let mods = Mods { shift: modifiers.shift, ctrl: modifiers.ctrl, alt: modifiers.alt };
                    let button = match button {
                        egui::PointerButton::Primary => Button::Left,
                        egui::PointerButton::Secondary => Button::Right,
                        egui::PointerButton::Middle => Button::Middle,
                        _ => continue,
                    };
                    if pressed {
                        if island.contains(pos) && self.over_grid_layer(ctx, pos) {
                            self.press(ctx, button, mods, pos, &modes);
                        }
                    } else {
                        self.release(ctx, button, mods, pos);
                    }
                }
                egui::Event::PointerMoved(pos) => {
                    let mods = ctx.input(|i| i.modifiers);
                    let mods = Mods { shift: mods.shift, ctrl: mods.ctrl, alt: mods.alt };
                    self.moved(ctx, pos, mods, &modes);
                }
                egui::Event::MouseWheel { unit, delta, modifiers } => {
                    let over = ctx.input(|i| i.pointer.hover_pos()).filter(|p| island.contains(*p));
                    if let Some(pos) = over {
                        self.wheel(ctx, unit, delta, modifiers, pos, &modes);
                    }
                }
                _ => {}
            }
        }
        // A selection held past the grid's top or bottom keeps scrolling.
        if let Some(Down { selecting: true, .. }) = self.pointer.down {
            if let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) {
                let spot = self.spot(ctx, pos);
                if spot.beyond != 0 {
                    let lines = -spot.beyond.signum() * (spot.beyond.abs().min(10));
                    let s = &self.tabs.current().session;
                    s.with_term_mut(|t| {
                        t.scroll_display(alacritty_terminal::grid::Scroll::Delta(lines));
                        let p = select::point_at(t, spot.col, spot.row);
                        select::extend(t, p, spot.side);
                    });
                    ctx.request_repaint_after(Duration::from_millis(40));
                }
            }
        }
        self.hover(ctx, island, &modes);
    }

    /// Whether `pos` is on the window's own layer (not a popup over it).
    fn over_grid_layer(&self, ctx: &egui::Context, pos: Pos2) -> bool {
        ctx.layer_id_at(pos).is_none_or(|l| l.order == egui::Order::Background)
    }

    fn press(&mut self, ctx: &egui::Context, button: Button, mods: Mods, pos: Pos2, modes: &Modes) {
        let spot = self.spot(ctx, pos);
        // A program that tracks the mouse gets it, unless Shift is held.
        if modes.tracking() && !mods.shift {
            self.pointer.down = Some(Down { button, reported: true, selecting: false });
            self.pointer.reported_cell = Some((spot.col, spot.row));
            self.send_report(button, Action::Press, mods, &spot);
            return;
        }
        let session = &self.tabs.current().session;
        match button {
            Button::Left => {
                if mods.ctrl {
                    if let Some(uri) = self.pointer.link.clone() {
                        crate::clip::open(&uri);
                        return;
                    }
                }
                let cell = (spot.col, spot.row);
                let now = Instant::now();
                let count = match self.pointer.last_click {
                    Some((t, c, n)) if c == cell && now - t <= double_click_time() => n % 3 + 1,
                    _ => 1,
                };
                self.pointer.last_click = Some((now, cell, count));
                let extend = mods.shift && session.with_term(|t| t.selection.is_some());
                session.with_term_mut(|t| {
                    let p = select::point_at(t, spot.col, spot.row);
                    if extend {
                        select::extend(t, p, spot.side);
                    } else {
                        let kind = match count {
                            2 => Kind::Words,
                            3 => Kind::Lines,
                            _ if mods.alt => Kind::Block,
                            _ => Kind::Cells,
                        };
                        select::start(t, kind, p, spot.side);
                    }
                });
                self.pointer.down = Some(Down { button, reported: false, selecting: true });
            }
            // Windows Terminal's right button: copy a selection, else paste.
            Button::Right => {
                if self.copy_selection(ctx) {
                    session.with_term_mut(|t| t.selection = None);
                } else if let Some(text) = crate::clip::text() {
                    let bracketed = session.mode().contains(TermMode::BRACKETED_PASTE);
                    session.write(crate::keys::paste_bytes(&text.replace("\r\n", "\n"), bracketed));
                }
            }
            _ => {}
        }
    }

    fn release(&mut self, ctx: &egui::Context, button: Button, mods: Mods, pos: Pos2) {
        let Some(down) = self.pointer.down else { return };
        if down.button != button {
            return;
        }
        let spot = self.spot(ctx, pos);
        if down.reported {
            self.send_report(button, Action::Release, mods, &spot);
        } else if down.selecting {
            let s = &self.tabs.current().session;
            if s.with_term(select::has) {
                if self.config.copy_on_select {
                    self.copy_selection(ctx);
                }
            } else if s.with_term(|t| t.selection.is_some()) {
                // A click without a drag selects nothing.
                s.with_term_mut(|t| t.selection = None);
            }
        }
        self.pointer.down = None;
    }

    fn moved(&mut self, ctx: &egui::Context, pos: Pos2, mods: Mods, modes: &Modes) {
        let spot = self.spot(ctx, pos);
        match self.pointer.down {
            Some(Down { reported: true, button, .. }) => {
                if self.pointer.reported_cell != Some((spot.col, spot.row)) {
                    self.pointer.reported_cell = Some((spot.col, spot.row));
                    self.send_report(button, Action::Motion, mods, &spot);
                }
            }
            Some(Down { selecting: true, .. }) => {
                if spot.beyond == 0 {
                    self.tabs.current().session.with_term_mut(|t| {
                        let p = select::point_at(t, spot.col, spot.row);
                        select::extend(t, p, spot.side);
                    });
                }
            }
            _ => {
                // Any-motion tracking hears moves with no button held, over the grid.
                if modes.motion && !mods.shift && self.grid.contains(pos) && self.pointer.reported_cell != Some((spot.col, spot.row)) {
                    self.pointer.reported_cell = Some((spot.col, spot.row));
                    self.send_report(Button::None, Action::Motion, mods, &spot);
                }
            }
        }
    }

    fn wheel(&mut self, ctx: &egui::Context, unit: egui::MouseWheelUnit, delta: egui::Vec2, m: egui::Modifiers, pos: Pos2, modes: &Modes) {
        let cell_h = self.renderer.metrics().cell_h as f32 / ctx.pixels_per_point();
        // A notch is three lines, as in Windows.
        let lines = match unit {
            egui::MouseWheelUnit::Line => delta.y * 3.0,
            egui::MouseWheelUnit::Point => delta.y / cell_h,
            egui::MouseWheelUnit::Page => delta.y * 24.0,
        };
        if m.ctrl {
            // Ctrl and the wheel zoom, a point a notch.
            self.pointer.wheel += lines / 3.0;
            let steps = self.pointer.wheel.trunc();
            self.pointer.wheel -= steps;
            if steps != 0.0 {
                self.zoom(Some(steps));
            }
            return;
        }
        self.pointer.wheel += lines;
        let whole = self.pointer.wheel.trunc() as i32;
        self.pointer.wheel -= whole as f32;
        if whole == 0 {
            return;
        }
        let session = &self.tabs.current().session;
        let mode = session.mode();
        let mods = Mods { shift: m.shift, ctrl: m.ctrl, alt: m.alt };
        if modes.tracking() && !m.shift {
            // One report a notch (three lines), as xterm sends.
            let spot = self.spot(ctx, pos);
            let button = if whole > 0 { Button::WheelUp } else { Button::WheelDown };
            let notches = ((whole.unsigned_abs() as usize) + 2) / 3;
            if let Some(bytes) = mouse::report(button, Action::Press, mods, spot.col, spot.row, modes) {
                session.report(&bytes.repeat(notches));
            }
        } else if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) && !m.shift {
            // `less`, `man`, `vim` without the mouse: arrow keys.
            let key = if whole > 0 { crate::keys::Key::Up } else { crate::keys::Key::Down };
            let bytes = crate::keys::key_bytes(key, Mods::NONE, mode.contains(TermMode::APP_CURSOR)).unwrap_or_default();
            session.report(&bytes.repeat(whole.unsigned_abs() as usize));
        } else {
            session.scroll(whole);
        }
    }

    /// The link under the pointer: an OSC 8 hyperlink underlines on hover; a URL in the text
    /// only while Ctrl is held. Ctrl over a link shows the hand. Redraws only when it changes.
    fn hover(&mut self, ctx: &egui::Context, island: Rect, modes: &Modes) {
        let (pos, ctrl, shift) = ctx.input(|i| (i.pointer.hover_pos(), i.modifiers.ctrl, i.modifiers.shift));
        let on_grid = pos.filter(|p| self.grid.contains(*p) && self.over_grid_layer(ctx, *p));
        let mut hover = Hover::None;
        let mut link = None;
        let mut label = None;
        if let (Some(pos), None) = (on_grid, self.pointer.down) {
            let spot = self.spot(ctx, pos);
            let session = &self.tabs.current().session;
            let found = session.with_term(|t| {
                let p = select::point_at(t, spot.col, spot.row);
                if let Some((id, uri)) = select::hyperlink_at(t, p) {
                    return Some((Hover::Link(id), uri));
                }
                if ctrl {
                    let (a, b, url) = select::url_at(t, p)?;
                    return Some((Hover::Span(a, b), url));
                }
                None
            });
            if let Some((h, uri)) = found {
                // An OSC 8 link's text can say anything: a label shows where it really goes.
                if matches!(h, Hover::Link(_)) {
                    let ppp = ctx.pixels_per_point();
                    let m = self.renderer.metrics();
                    let min = egui::pos2(self.grid.left() + (spot.col as i32 * m.cell_w) as f32 / ppp, self.grid.top() + (spot.row as i32 * m.cell_h) as f32 / ppp);
                    let cell = Rect::from_min_size(min, egui::vec2(m.cell_w as f32 / ppp, m.cell_h as f32 / ppp));
                    let id = egui::Id::new(("terminal-link-label", &uri));
                    shellman_ui::hover::label_at(ctx, id, cell, true, &crate::clip::describe(&uri));
                    label = Some(id);
                }
                hover = h;
                link = Some(uri);
            }
        }
        if let Some(gone) = self.pointer.label.filter(|g| Some(*g) != label) {
            shellman_ui::hover::label_at(ctx, gone, Rect::NOTHING, false, "");
        }
        self.pointer.label = label;
        if hover != self.view.hover {
            self.view.hover = hover;
            self.tabs.current().session.mark_dirty();
        }
        self.pointer.link = link;
        if let Some(pos) = pos.filter(|p| island.contains(*p)) {
            let icon = if ctrl && self.pointer.link.is_some() {
                egui::CursorIcon::PointingHand
            } else if modes.tracking() && !shift {
                egui::CursorIcon::Default
            } else if self.grid.contains(pos) {
                egui::CursorIcon::Text
            } else {
                egui::CursorIcon::Default
            };
            ctx.set_cursor_icon(icon);
        }
    }
}

/// Windows' double-click time.
fn double_click_time() -> Duration {
    let ms = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime() };
    Duration::from_millis(if (100..=5000).contains(&ms) { ms as u64 } else { 500 })
}
