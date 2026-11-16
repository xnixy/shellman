//! The arrange bar: one glass island at the bottom middle of the monitor Edit desktop… was
//! picked on, saying what to do and holding Reset, Cancel and Done, with a chip per piece.
//!
//! It is the one window of arrange mode that takes the keyboard focus (the pieces' windows
//! never activate): Tab goes through the chips and the buttons, the arrow keys move the piece
//! whose chip has the focus (10 pt, 1 pt with Shift; Left and Right put the shelf on that
//! edge), Enter on a chip selects it, Esc cancels. Every chip is named for screen readers with
//! the piece, its display and where it is now ("Today, main display: 40 pt from the right, 16 pt
//! from the top").
//!
//! eframe gives screen readers only its root window, so the bar has an AccessKit adapter of its
//! own (`access`) with the same names.
//!
//! An immediate viewport of the shelf's, made the first time arrange mode starts and kept: out
//! of arrange mode it shows nothing and takes no clicks and is cloaked (never hidden: a hidden
//! window makes the window library retry its frames without pause). Its glass is attached once.

use super::access::{Access, Asked, Control};
use super::{place, Item, Piece};
use crate::geometry::SHADOW;
use egui::{pos2, vec2, Key, Modifiers, Rect, Sense, UiBuilder};
use shellman_core::monitors::Monitor;
use shellman_ui::glass::{self, guard, GlassPanel, Island, Style};
use shellman_ui::theme;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongPtrW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, GWL_STYLE,
    HWND_NOTOPMOST, HWND_TOPMOST, STYLESTRUCT, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    SWP_NOZORDER, SWP_SHOWWINDOW, WM_NCACTIVATE, WM_NCPAINT, WM_STYLECHANGING, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_POPUP,
};

/// The island's size.
pub const W: f32 = 660.0;
pub const H: f32 = 136.0;
pub const RADIUS: f32 = 22.0;
/// How far the arrow keys move a piece, and with Shift.
const STEP: f32 = 10.0;
const FINE: f32 = 1.0;

/// What the bar asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Done,
    Cancel,
    Reset,
}

/// Where the bar's window goes on monitor `m`: the island 16 pt above the dock, in the middle;
/// the window is the island and its shadow, never the monitor. None on a monitor too small.
pub fn window_for(m: &Monitor) -> Option<(RECT, Rect, f32)> {
    if !crate::displays::usable(m) {
        return None;
    }
    let work = place::work(m);
    let s = m.scale;
    let island = Rect::from_min_size(pos2(work.center().x - W / 2.0, work.bottom() - place::MARGIN_Y - H), vec2(W, H));
    let win = island.expand(SHADOW).intersect(Rect::from_min_size(pos2(0.0, 0.0), vec2((m.rect.right - m.rect.left) as f32 / s, (m.rect.bottom - m.rect.top) as f32 / s)));
    let px = |v: f32| (v * s).round() as i32;
    let rect = RECT { left: m.rect.left + px(win.left()), top: m.rect.top + px(win.top()), right: m.rect.left + px(win.right()), bottom: m.rect.top + px(win.bottom()) };
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    if guard::window((w, h)) != Some((w, h)) || island.left() < work.left() || island.top() < work.top() {
        return None;
    }
    let origin = pos2(island.left() - (rect.left - m.rect.left) as f32 / s, island.top() - (rect.top - m.rect.top) as f32 / s);
    Some((rect, Rect::from_min_size(origin, island.size()), s))
}

pub struct Bar {
    id: egui::ViewportId,
    title: String,
    hwnd: Option<isize>,
    panel: Option<GlassPanel>,
    /// Declared from now on (arrange mode has started once).
    made: bool,
    /// Wanted on screen, and on screen.
    want: bool,
    shown: bool,
    monitor: Option<Monitor>,
    placed: Option<(RECT, Rect, f32)>,
    /// The window in front before the bar took the focus, to give it back.
    before: Option<isize>,
    /// Its tree for screen readers.
    access: Option<Access>,
}

const TITLE: &str = "Arrange the desktop";
const HELP: &str = "Drag the shelf or a widget to move it. Arrow keys move the chosen one, Shift for 1 pt. Esc cancels.";
/// The buttons, in the order screen readers get them.
const BUTTONS: [(&str, Action); 3] = [("Reset", Action::Reset), ("Cancel", Action::Cancel), ("Done", Action::Done)];

impl Bar {
    pub fn new() -> Self {
        Self {
            id: egui::ViewportId::from_hash_of("shellman arrange bar"),
            title: format!("{} arrange", shellman_core::desktop_menu::shelf_title()),
            hwnd: None,
            panel: None,
            made: false,
            want: false,
            shown: false,
            monitor: None,
            placed: None,
            before: None,
            access: None,
        }
    }

    /// Show the bar on monitor `m`.
    pub fn show(&mut self, m: Monitor) {
        self.made = true;
        self.want = true;
        self.monitor = Some(m);
        self.placed = None;
    }

    /// The monitors changed: place it again on the same monitor if it is still there, else on
    /// the first given.
    pub fn follow(&mut self, monitors: &[Monitor]) {
        let Some(old) = &self.monitor else { return };
        let same = monitors.iter().find(|m| m.device == old.device).or_else(|| monitors.iter().find(|m| m.primary)).or(monitors.first());
        self.monitor = same.cloned();
        self.placed = None;
    }

    pub fn hide(&mut self) {
        self.want = false;
    }

    fn builder(&self) -> egui::ViewportBuilder {
        egui::ViewportBuilder::default()
            .with_title(self.title.clone())
            .with_inner_size([W + 2.0 * SHADOW, H + 2.0 * SHADOW])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_taskbar(false)
            .with_visible(false)
    }

    /// In the shelf's frame: declare the window (once made) and draw it.
    pub fn frame(&mut self, ctx: &egui::Context) -> Option<Action> {
        if !self.made {
            return None;
        }
        let mut out = None;
        ctx.show_viewport_immediate(self.id, self.builder(), |vctx, _| {
            out = self.view(vctx);
            shellman_ui::font_atlas::end_frame(vctx);
        });
        out
    }

    fn adopt(&mut self, ctx: &egui::Context) -> bool {
        if self.hwnd.is_some() {
            return true;
        }
        let Some(hwnd) = crate::own_window(&self.title) else { return false };
        prepare(hwnd);
        // Before the window is first shown, as the adapter needs.
        self.access = Some(Access::attach(hwnd, ctx));
        let panel = GlassPanel::attach(hwnd, Style::from_theme(RADIUS));
        panel.set_islands_only(&[], 1.0, None);
        self.panel = Some(panel);
        self.hwnd = Some(hwnd);
        crate::log::line(format!("window for {}", self.title));
        true
    }

    fn view(&mut self, ctx: &egui::Context) -> Option<Action> {
        crate::theme_for(ctx);
        if !self.adopt(ctx) {
            ctx.request_repaint();
            return None;
        }
        let hwnd = self.hwnd?;
        if !self.want {
            if self.shown {
                self.shown = false;
                if let Some(p) = &self.panel {
                    p.set_islands_only(&[], ctx.pixels_per_point(), None);
                }
                glass::cloak(hwnd, true);
                unsafe {
                    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
                    let _ = SetWindowPos(HWND(hwnd as *mut _), Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
                    if GetForegroundWindow().0 as isize == hwnd {
                        if let Some(b) = self.before.take() {
                            let _ = SetForegroundWindow(HWND(b as *mut _));
                        }
                    }
                }
            }
            return None;
        }
        if self.placed.is_none() {
            self.placed = self.monitor.as_ref().and_then(window_for);
            if let Some((rect, _, _)) = self.placed {
                put(hwnd, rect);
            }
        }
        if !self.shown && self.placed.is_some() {
            self.shown = true;
            glass::cloak(hwnd, false);
            let front = unsafe { GetForegroundWindow() };
            if front.0 as isize != hwnd && !front.is_invalid() {
                self.before = Some(front.0 as isize);
            }
            show_focused(hwnd);
        }
        let Some((_, island, scale)) = self.placed else {
            // No room for the bar on that monitor: without it there are no controls and no
            // keyboard, so arrange mode does not go on.
            crate::log::line("arrange: no room for the bar");
            return Some(Action::Cancel);
        };
        if (scale - ctx.pixels_per_point()).abs() > 0.01 {
            ctx.request_repaint();
            return None;
        }
        let islands = [Island::new(island, RADIUS)];
        let style = Style::from_theme(RADIUS);
        let composed = self.panel.as_ref().is_some_and(GlassPanel::composed);
        let mut out = None;
        // What a screen reader asked for: a button pressed, or a piece chosen (given the focus).
        let mut focus_chip = None;
        for asked in self.access.as_ref().map(Access::take).unwrap_or_default() {
            match asked {
                Asked::Button(i) => out = BUTTONS.get(i).map(|b| b.1),
                Asked::Chip(i) => focus_chip = Some(i),
            }
        }
        let mut drawn = Drawn::default();
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
            let under = ui.painter().add(egui::Shape::Noop);
            if let Some(a) = content(ui, island, focus_chip, &mut drawn) {
                out = Some(a);
            }
            if !composed {
                ui.painter().set(under, egui::Shape::Vec(glass::islands::solid_shapes(&islands, &style)));
            }
        });
        if let Some(access) = &mut self.access {
            access.update(TITLE, HELP, island, &drawn.buttons, &drawn.chips, ctx.pixels_per_point());
        }
        if let Some(p) = &self.panel {
            p.set_style(style);
            p.set_islands_only(&islands, ctx.pixels_per_point(), None);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            out = Some(Action::Cancel);
        }
        if out.is_none() && ctx.memory(|m| m.focused().is_none()) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            out = Some(Action::Done);
        }
        out
    }
}

/// The name a screen reader reads for a piece's chip.
pub fn chip_name(p: &Piece, several: bool) -> String {
    let display = if several { format!(", {}", p.display) } else { String::new() };
    let at = match p.item {
        Item::Shelf => {
            let side = if p.rect.center().x < p.work.center().x { "left edge" } else { "right edge" };
            let y = place::pin_for(p.rect.top(), p.work.top(), p.work.bottom(), p.rect.height());
            let up = match y.from {
                shellman_core::settings::desktop::Anchor::Middle if y.by == 0 => "centred".to_owned(),
                _ => place::describe(p.rect, p.work).split(", ").nth(1).unwrap_or("").to_owned(),
            };
            format!("{side}, {up}")
        }
        _ => place::describe(p.rect, p.work),
    };
    format!("{}{display}: {at}", p.item.label())
}

/// The controls as drawn, for screen readers.
#[derive(Default)]
struct Drawn {
    buttons: Vec<Control>,
    chips: Vec<Control>,
}

fn content(ui: &mut egui::Ui, island: Rect, focus_chip: Option<usize>, drawn: &mut Drawn) -> Option<Action> {
    let c = theme::colors();
    let mut out = None;
    let inner = island.shrink2(vec2(22.0, 16.0));
    ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(TITLE).font(theme::semibold_font(15.0)).color(c.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Right to left: Done first.
                for (name, action) in BUTTONS.iter().rev() {
                    let button = if *action == Action::Done { theme::primary_button(name) } else { theme::secondary_button(name) };
                    let mut resp = ui.add(button);
                    if *action == Action::Reset {
                        resp = resp.on_hover_text("Put everything back where it started");
                    }
                    if resp.clicked() {
                        out = Some(*action);
                    }
                    drawn.buttons.insert(0, Control { name: (*name).to_owned(), rect: resp.rect, chip: None, focused: resp.has_focus() });
                }
            });
        });
        ui.add_space(8.0);
        let pieces = super::pieces();
        let several = pieces.iter().any(|p| p.key != pieces[0].key);
        egui::ScrollArea::horizontal().id_salt("pieces").show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, p) in pieces.iter().enumerate() {
                    drawn.chips.push(chip(ui, p, several, focus_chip == Some(i)));
                }
            });
        });
        ui.add_space(6.0);
        ui.label(egui::RichText::new(HELP).size(12.0).color(c.muted));
    });
    out
}

/// A piece's chip: selects it on a click, Enter or Space; with the focus, the arrows move it.
fn chip(ui: &mut egui::Ui, p: &Piece, several: bool, focus: bool) -> Control {
    let c = theme::colors();
    let selected = super::is_selected(&p.key, p.item);
    let text = if several { format!("{} · {}", p.item.label(), p.display) } else { p.item.label().to_owned() };
    let galley = ui.painter().layout_no_wrap(text, egui::FontId::proportional(13.0), if selected { c.on_accent } else { c.text });
    let size = vec2(galley.size().x + 28.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if focus {
        resp.request_focus();
        super::select(&p.key, p.item);
    }
    let name = chip_name(p, several);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, name.clone()));
    let fill = if selected {
        c.accent_fill
    } else if resp.hovered() {
        c.row_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    let stroke = egui::Stroke::new(1.0_f32, if selected && !c.high_contrast { c.accent_fill } else { c.hairline });
    ui.painter().rect(rect, 15.0, fill, stroke, egui::StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, c.text);
    theme::focus_ring(ui, &resp, rect, 15.0);
    if resp.clicked() {
        super::select(&p.key, p.item);
        resp.request_focus();
    }
    if resp.has_focus() {
        // The arrows are this chip's while it has the focus, not egui's focus moves.
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(resp.id, egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, tab: false, escape: false })
        });
        if !selected {
            super::select(&p.key, p.item);
        }
        let step = if ui.input(|i| i.modifiers.shift) { FINE } else { STEP };
        let mut d = vec2(0.0, 0.0);
        ui.input_mut(|i| {
            for (key, v) in [(Key::ArrowLeft, vec2(-1.0, 0.0)), (Key::ArrowRight, vec2(1.0, 0.0)), (Key::ArrowUp, vec2(0.0, -1.0)), (Key::ArrowDown, vec2(0.0, 1.0))] {
                if i.consume_key(Modifiers::NONE, key) || i.consume_key(Modifiers::SHIFT, key) {
                    d += v;
                }
            }
        });
        if d != vec2(0.0, 0.0) {
            if p.item == Item::Shelf && d.x != 0.0 {
                super::set_edge(if d.x < 0.0 { shellman_core::settings::desktop::Edge::Left } else { shellman_core::settings::desktop::Edge::Right });
            }
            super::nudge(p, d * step);
            ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
        }
    }
    Control { name, rect, chip: Some(selected), focused: resp.has_focus() }
}

const SUBCLASS: usize = 0x6172_7267;

/// A tool window (never in Alt+Tab or the dock) without caption or frame that can take the
/// focus, kept so as the window library puts its styles back.
fn prepare(hwnd: isize) {
    let h = HWND(hwnd as *mut _);
    unsafe {
        let _ = SetWindowSubclass(h, Some(bar_proc), SUBCLASS, 0);
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        SetWindowLongPtrW(h, GWL_EXSTYLE, fix_ex(ex) as isize);
        let style = GetWindowLongPtrW(h, GWL_STYLE) as u32;
        SetWindowLongPtrW(h, GWL_STYLE, fix_style(style) as isize);
        let flags = SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
        let _ = SetWindowPos(h, None, 0, 0, 0, 0, flags);
        let none: u32 = 0xFFFF_FFFE;
        let _ = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(h, windows::Win32::Graphics::Dwm::DWMWA_BORDER_COLOR, (&raw const none).cast(), 4);
    }
    crate::window::extend_frame(h);
}

fn fix_ex(ex: u32) -> u32 {
    (ex & !WS_EX_APPWINDOW.0) | WS_EX_TOOLWINDOW.0
}

fn fix_style(style: u32) -> u32 {
    crate::window::fix_style(style) | WS_POPUP.0
}

unsafe extern "system" fn bar_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    match msg {
        WM_STYLECHANGING if wparam.0 as i32 == GWL_EXSTYLE.0 => {
            let change = &mut *(lparam.0 as *mut STYLESTRUCT);
            change.styleNew = fix_ex(change.styleNew);
        }
        WM_STYLECHANGING if wparam.0 as i32 == GWL_STYLE.0 => {
            let change = &mut *(lparam.0 as *mut STYLESTRUCT);
            change.styleNew = fix_style(change.styleNew);
        }
        // No frame to paint when it becomes active (Windows draws a caption otherwise).
        WM_NCACTIVATE => return LRESULT(1),
        WM_NCPAINT => return LRESULT(0),
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// On `rect`, topmost, checked once more after a move across scales.
fn put(hwnd: isize, rect: RECT) {
    let h = HWND(hwnd as *mut _);
    for _ in 0..2 {
        let mut now = RECT::default();
        if unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(h, &mut now) }.is_ok() && now == rect {
            return;
        }
        unsafe {
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, SWP_NOACTIVATE | SWP_NOOWNERZORDER);
        }
    }
}

/// Shown on top with the keyboard focus (this process was just picked from its menu, so
/// Windows lets it; if not, one press of an unused key makes it the last input's sender).
fn show_focused(hwnd: isize) {
    let h = HWND(hwnd as *mut _);
    unsafe {
        let flags = SWP_SHOWWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER;
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
        let _ = SetForegroundWindow(h);
        if GetForegroundWindow() != h {
            shellman_core::foreground::send_mask();
            let _ = SetForegroundWindow(h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellman_core::settings::desktop::Edge;

    fn mon(left: i32, top: i32, w: i32, h: i32, scale: f32) -> Monitor {
        let r = RECT { left, top, right: left + w, bottom: top + h };
        let work = RECT { top: r.top + (60.0 * scale) as i32, bottom: r.bottom - (102.0 * scale) as i32, ..r };
        Monitor { device: "x".into(), rect: r, work, scale, primary: true }
    }

    #[test]
    fn the_bar_sits_above_the_dock_in_the_middle_and_is_small() {
        for m in [mon(0, 0, 3840, 2160, 1.5), mon(-1920, -1080, 1920, 1080, 1.0), mon(3746, 2160, 3840, 2160, 3.0)] {
            let (r, island, s) = window_for(&m).expect("a bar");
            assert!(r.left >= m.rect.left && r.right <= m.rect.right && r.top >= m.rect.top && r.bottom <= m.rect.bottom);
            assert!((r.right - r.left) < (m.rect.right - m.rect.left) && (r.bottom - r.top) * 3 < m.rect.bottom - m.rect.top);
            let bottom_px = r.top as f32 + island.bottom() * s;
            assert!((m.work.bottom as f32 - bottom_px - place::MARGIN_Y * s).abs() <= 1.0);
            let mid_px = r.left as f32 + island.center().x * s;
            assert!((mid_px - (m.work.left + m.work.right) as f32 / 2.0).abs() <= 1.0);
        }
        // Narrower than the bar: none.
        assert_eq!(window_for(&mon(0, 0, 1280, 800, 2.0)), None);
    }

    #[test]
    fn chips_say_what_where_and_which_display() {
        let work = Rect::from_min_max(pos2(0.0, 40.0), pos2(2560.0, 1372.0));
        let shelf = Rect::from_min_size(pos2(20.0, work.center().y - 200.0), vec2(104.0, 400.0));
        let p = Piece { key: "primary".into(), item: Item::Shelf, display: "main display".into(), rect: shelf, work };
        assert_eq!(chip_name(&p, false), "Shelf: left edge, centred");
        let t = Rect::from_min_size(pos2(2560.0 - 40.0 - 372.0, 56.0), vec2(372.0, 250.0));
        let p = Piece { key: "x".into(), item: Item::Today, display: "display 2".into(), rect: t, work };
        assert_eq!(chip_name(&p, true), "Today, display 2: 40 pt from the right, 16 pt from the top");
        let _ = Edge::Left;
    }
}
