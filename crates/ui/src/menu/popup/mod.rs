//! Shellman's own right-click menu as a popup window of its own, on glass: for menus that cannot
//! live inside their app's window (the desktop's, the dock's).
//!
//! # Using it
//!
//! ```ignore
//! use shellman_ui::menu::popup::{self, Anchor, Entry, PopupMenu};
//! // Every frame of the app's root viewport (it keeps the menu window alive):
//! popup::declare(ctx);
//! // On a right-click, at a point in physical screen pixels:
//! self.menu.open(ctx, Anchor::at(x, y), vec![
//!     Entry::item(Action::Paste, "&Paste").icon(glyph::PASTE).enabled(can_paste),
//!     Entry::separator(),
//!     Entry::item(Action::Delete, "Delete").danger(),
//! ]);
//! // In any later frame of the viewport that opened it (it is woken when something is picked):
//! if let Some(action) = self.menu.take_pick() { ... }
//! ```
//!
//! The app's `clear_color` must be transparent (every shellman app with glass windows has it so).
//!
//! # The window
//!
//! One window per process, an egui viewport declared by [`declare`] from the first [`open`]
//! on and never closed after: it is created once (lazily), and its glass panel is attached once
//! when it is first found, while it is still hidden. Opening a menu only gives it its size and
//! place (while hidden and cloaked), lays the glass islands out, shows it and lets it out from
//! behind the cloak once a couple of frames are drawn at the monitor's scale. Closing cloaks it
//! with its glass, hides its glass pieces (an empty window region: nothing of it takes clicks)
//! and parks it shown-but-cloaked at its last size (a hidden window gets no paint messages, and
//! a frame asked for meanwhile would make the window library spin). Out of sight, it asks for no
//! frames and costs nothing.
//!
//! It is exactly as large as the menu, or the menu and its submenu side by side (the two are
//! islands of the same window, so the window never needs more than the two pieces' bounding
//! box); the glass's shadow lies outside it, in the glass's own backdrop window. Every size goes
//! through [`place::guard`] first: DWM fails fast on a composition surface of size 0, negative,
//! or above 16384 (the 12 Nov compositor crashes), and a monitor-sized popup makes the bar step
//! aside as for a fullscreen app.
//!
//! It is a topmost tool window (never in Alt+Tab, the dock or the window list). It takes the
//! keyboard focus while open (for Esc and the arrows) and gives it back to the window that had it
//! when closed by a pick or Esc. It closes on a pick, Esc, a click outside (`core::outside`), the
//! focus going elsewhere, or another menu of the process opening.
//!
//! [`open`]: PopupMenu::open

mod draw;
mod model;
mod nav;
pub mod place;
mod win;

pub use model::{Entry, Item};
pub use place::Px;

use crate::glass::{self, GlassPanel, Island, Style};
use egui::{Context, Pos2, Rect, ViewportId};
use model::Row;
use nav::{Key, Nav, Outcome};
use shellman_core::outside;
use std::cell::RefCell;
use std::sync::{Mutex, MutexGuard};

const TITLE: &str = "shellman menu";
/// Frames drawn behind the cloak before the menu is let out: the first complete, the next at the
/// monitor's scale. Capped, in case the scale never settles.
const WARM_FRAMES: u8 = 2;
const WARM_CAP: u8 = 20;

fn viewport_id() -> ViewportId {
    ViewportId::from_hash_of("shellman popup menu")
}

/// Where a menu opens, in physical screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub x: i32,
    pub y: i32,
    /// Above the point (its bottom-left corner there), as from a dock at the bottom.
    pub up: bool,
}

impl Anchor {
    /// The menu's top-left at the point (flipped at the monitor's edges).
    pub fn at(x: i32, y: i32) -> Self {
        Self { x, y, up: false }
    }

    /// The menu's bottom-left at the point.
    pub fn above(x: i32, y: i32) -> Self {
        Self { x, y, up: true }
    }
}

/// A caller's handle: what it opened, and the values to hand back. Cheap; keep one per kind of
/// menu in the app's state.
pub struct PopupMenu<T> {
    ids: Vec<T>,
    token: u64,
}

impl<T> Default for PopupMenu<T> {
    fn default() -> Self {
        Self { ids: Vec::new(), token: 0 }
    }
}

impl<T: Clone> PopupMenu<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a menu of `entries` at `at`, in place of any menu this process has open.
    pub fn open(&mut self, ctx: &Context, at: Anchor, entries: Vec<Entry<T>>) {
        self.ids.clear();
        let rows = model::flatten(entries, &mut self.ids, 0);
        self.token = open_rows(ctx, at, rows);
    }

    /// What was picked from this handle's menu, once.
    pub fn take_pick(&mut self) -> Option<T> {
        let mut core = lock();
        match core.pick {
            Some((token, i)) if token == self.token && token != 0 => {
                core.pick = None;
                self.ids.get(i).cloned()
            }
            _ => None,
        }
    }

    /// Whether this handle's menu is open.
    pub fn is_open(&self) -> bool {
        self.token != 0 && lock().open.as_ref().is_some_and(|o| o.token == self.token)
    }

    /// Close this handle's menu, if open, without a pick.
    pub fn close(&self) {
        let mut core = lock();
        if core.open.as_ref().is_some_and(|o| o.token == self.token) {
            finish(&mut core, None, true);
        }
    }
}

/// Keep the menu window alive. Call in every frame of the app's root viewport; does nothing
/// until the first menu opens.
pub fn declare(ctx: &Context) {
    {
        let mut core = lock();
        if !core.started {
            return;
        }
        core.ctx = Some(ctx.clone());
    }
    let builder = egui::ViewportBuilder::default()
        .with_title(TITLE)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_visible(false)
        // A real size from the start, far away: it is only shown once placed and sized.
        .with_position([-30000.0, -30000.0])
        .with_inner_size([240.0, 200.0]);
    // Not under the lock: where viewports are embedded, egui runs the frame right here.
    ctx.show_viewport_deferred(viewport_id(), builder, |ctx, _| frame(ctx));
    let mut core = lock();
    if core.hwnd.is_none() {
        match win::own_window(TITLE) {
            Some(hwnd) => adopt(&mut core, hwnd),
            // Made at the end of this frame: look again in the next.
            None => ctx.request_repaint(),
        }
    }
    if core.hwnd.is_some() && core.open.as_ref().is_some_and(|o| !o.shown) {
        present(&mut core);
    }
}

/// For tests and the example: open the submenu of row `row` of the open menu.
#[doc(hidden)]
pub fn debug_open_submenu(row: usize) {
    let mut core = lock();
    let ctx = core.ctx.clone();
    if let Some(open) = core.open.as_mut() {
        if open.rows.get(row).and_then(Row::submenu).is_some() {
            open.nav.sub = Some(row);
            open.nav.hot = [Some(row), Some(0)];
            open.nav.level = 1;
        }
    }
    if let Some(ctx) = ctx {
        ctx.request_repaint_of(viewport_id());
    }
}

struct Core {
    /// Set by the first open: from then on the window is declared.
    started: bool,
    ctx: Option<Context>,
    hwnd: Option<isize>,
    open: Option<Open>,
    /// The token of the menu something was picked from, and the value's index.
    pick: Option<(u64, usize)>,
    next_token: u64,
}

struct Open {
    token: u64,
    rows: Vec<Row>,
    anchor: Anchor,
    work: Px,
    /// The monitor's pixels per point.
    scale: f32,
    nav: Nav,
    /// The viewport that opened it, woken when something is picked.
    opener: ViewportId,
    /// The window in front before the menu, to give the focus back to.
    before: isize,
    /// The window has been placed and shown for this menu.
    shown: bool,
    /// Frames drawn behind the cloak; None once let out.
    warm: Option<u8>,
    had_focus: bool,
    /// A button went down inside the menu (so its release may pick).
    pressed_inside: bool,
    last_pointer: Option<Pos2>,
    laid: Laid,
}

/// Where everything is, in physical pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Laid {
    main: Px,
    sub: Option<Px>,
    window: Px,
}

static CORE: Mutex<Core> = Mutex::new(Core { started: false, ctx: None, hwnd: None, open: None, pick: None, next_token: 1 });

thread_local! {
    /// The menu window's glass, made once with the window (a panel belongs to the UI thread).
    static PANEL: RefCell<Option<GlassPanel>> = const { RefCell::new(None) };
}

fn lock() -> MutexGuard<'static, Core> {
    CORE.lock().unwrap_or_else(|e| e.into_inner())
}

fn style() -> Style {
    Style::from_theme(draw::RADIUS)
}

fn open_rows(ctx: &Context, at: Anchor, rows: Vec<Row>) -> u64 {
    let mut core = lock();
    let before = match core.open.take() {
        // Another menu was open: this one takes its place, and the focus goes back where that
        // one would have given it.
        Some(old) => {
            outside::stop();
            if let Some(hwnd) = core.hwnd {
                put_away(hwnd);
            }
            old.before
        }
        None => 0,
    };
    let token = core.next_token;
    core.next_token += 1;
    core.started = true;
    core.ctx = Some(ctx.clone());
    let Some((work, scale)) = win::monitor_at(at.x, at.y) else { return token };
    let level = draw::measure_with(ctx, &rows);
    let Some(laid) = lay_out(at, work, scale, &level, None) else { return token };
    core.open = Some(Open {
        token,
        rows,
        anchor: at,
        work,
        scale,
        nav: Nav::default(),
        opener: ctx.viewport_id(),
        before,
        shown: false,
        warm: Some(0),
        had_focus: false,
        pressed_inside: false,
        last_pointer: None,
        laid,
    });
    if core.hwnd.is_some() {
        present(&mut core);
    } else {
        // The root's next frame declares the window; it is shown once found.
        ctx.request_repaint_of(ViewportId::ROOT);
    }
    token
}

/// Place the menu (and the submenu, `sub`: its row's top in the menu, in points, and its level)
/// for the anchor, in pixels. None when a size is not fit to give a window.
fn lay_out(at: Anchor, work: Px, scale: f32, main: &draw::Level, sub: Option<(f32, &draw::Level)>) -> Option<Laid> {
    let limit = (work.width(), work.height());
    let size = place::guard(main.size.x, main.size.y, scale, limit)?;
    let main_px = place::place_menu((at.x, at.y), size, at.up, work);
    let sub_px = match sub {
        Some((row_top, level)) => {
            let size = place::guard(level.size.x, level.size.y, scale, limit)?;
            let px = |pt: f32| (pt * scale).round() as i32;
            Some(place::place_submenu(main_px, main_px.top + px(row_top), px(draw::PAD), size, px(draw::SUB_GAP), work))
        }
        None => None,
    };
    let window = sub_px.map_or(main_px, |s| main_px.union(&s));
    place::sane(&window, &work).then_some(Laid { main: main_px, sub: sub_px, window })
}

/// The window was found: make it a tool window and give it its glass, once, while hidden.
fn adopt(core: &mut Core, hwnd: isize) {
    win::prepare(hwnd);
    glass::cloak(hwnd, true);
    PANEL.with(|p| {
        let panel = GlassPanel::attach(hwnd, style());
        // Nothing shows or takes clicks until a menu is laid out.
        panel.set_islands_only(&[], 1.0, None);
        *p.borrow_mut() = Some(panel);
    });
    core.hwnd = Some(hwnd);
}

/// The islands of a layout, in points of the window at `scale` pixels per point.
fn islands(laid: &Laid, scale: f32) -> Vec<Island> {
    let w = laid.window;
    let to_pt = |r: &Px| {
        Rect::from_min_max(
            Pos2::new((r.left - w.left) as f32 / scale, (r.top - w.top) as f32 / scale),
            Pos2::new((r.right - w.left) as f32 / scale, (r.bottom - w.top) as f32 / scale),
        )
    };
    std::iter::once(&laid.main).chain(laid.sub.as_ref()).map(|r| Island::new(to_pt(r), draw::RADIUS)).collect()
}

/// Give the glass the layout's islands (and the window its region).
fn set_glass(laid: &Laid, scale: f32) {
    let list = islands(laid, scale);
    PANEL.with(|p| {
        if let Some(panel) = p.borrow().as_ref() {
            panel.set_style(style());
            panel.set_islands_only(&list, scale, None);
        }
    });
}

fn set_outside(laid: &Laid) {
    let rect = |r: &Px| (r.left, r.top, r.right, r.bottom);
    let mut rects = vec![rect(&laid.main)];
    rects.extend(laid.sub.as_ref().map(rect));
    outside::set_rects(&rects);
}

/// Show the open menu: sized and placed while hidden and cloaked, then shown with the focus.
fn present(core: &mut Core) {
    let (Some(hwnd), Some(ctx)) = (core.hwnd, core.ctx.clone()) else { return };
    let Some(open) = core.open.as_mut() else { return };
    if !place::sane(&open.laid.window, &open.work) {
        core.open = None;
        return;
    }
    let fg = win::foreground();
    if open.before == 0 && fg != hwnd {
        open.before = fg;
    }
    glass::cloak(hwnd, true);
    // Its real size first, while hidden; only then shown.
    win::put(hwnd, open.laid.window);
    set_glass(&open.laid, open.scale);
    set_outside(&open.laid);
    open.shown = true;
    open.warm = Some(0);
    let wake = ctx.clone();
    outside::start(move || wake.request_repaint_of(viewport_id()));
    win::show_focused(hwnd);
    ctx.request_repaint_of(viewport_id());
}

/// Out of sight until the next menu: cloaked with its glass, its glass pieces hidden (so nothing
/// of it takes clicks), and parked (see [`win::park`]), keeping its last size: a window and its
/// glass are never shrunk to nothing.
fn put_away(hwnd: isize) {
    glass::cloak(hwnd, true);
    PANEL.with(|p| {
        if let Some(panel) = p.borrow().as_ref() {
            panel.set_islands_only(&[], 1.0, None);
        }
    });
    win::park(hwnd);
}

/// Close the open menu, handing `pick` to its caller. With `give_back`, the window that had the
/// focus before gets it back, if the menu still has it.
fn finish(core: &mut Core, pick: Option<usize>, give_back: bool) {
    let Some(open) = core.open.take() else { return };
    outside::stop();
    if let Some(hwnd) = core.hwnd {
        if give_back && open.before != 0 && win::foreground() == hwnd {
            win::set_foreground(open.before);
        }
        put_away(hwnd);
    }
    core.pick = pick.map(|i| (open.token, i));
    if let Some(ctx) = &core.ctx {
        ctx.request_repaint_of(open.opener);
        if open.opener != ViewportId::ROOT {
            ctx.request_repaint_of(ViewportId::ROOT);
        }
    }
}

/// The input of one frame, as the menu reads it.
struct Input {
    keys: Vec<Key>,
    pointer: Option<Pos2>,
    pressed: bool,
    released: bool,
    focused: bool,
    now: f64,
}

fn read_input(ctx: &Context) -> Input {
    ctx.input(|i| {
        let mut keys = Vec::new();
        for e in &i.events {
            match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } if !modifiers.ctrl && !modifiers.alt => {
                    use egui::Key as K;
                    keys.push(match key {
                        K::ArrowUp => Key::Up,
                        K::ArrowDown => Key::Down,
                        K::Home => Key::Home,
                        K::End => Key::End,
                        K::ArrowLeft => Key::Left,
                        K::ArrowRight => Key::Right,
                        K::Enter | K::Space => Key::Enter,
                        K::Escape => Key::Escape,
                        _ => continue,
                    });
                }
                egui::Event::Text(t) => {
                    let mut chars = t.chars();
                    if let (Some(c), None) = (chars.next(), chars.next()) {
                        if c.is_alphanumeric() {
                            keys.push(Key::Letter(c));
                        }
                    }
                }
                _ => {}
            }
        }
        Input {
            keys,
            pointer: i.pointer.latest_pos(),
            pressed: i.pointer.any_pressed(),
            released: i.pointer.any_released(),
            focused: i.viewport().focused.unwrap_or(false),
            now: i.time,
        }
    })
}

/// One frame of the menu window.
fn frame(ctx: &Context) {
    let mut core = lock();
    let Some(hwnd) = core.hwnd else { return };
    let Some(open) = core.open.as_mut().filter(|o| o.shown) else {
        // Nothing open: a frame still pending from the last menu, drawn out of sight.
        return;
    };
    let ppp = ctx.pixels_per_point();
    let input = read_input(ctx);

    // Lay out at this frame's fonts; the submenu as the navigation says.
    let main = draw::measure_with(ctx, &open.rows);
    let sub_rows = open.nav.sub_rows(&open.rows).map(<[Row]>::to_vec);
    let sub_level = sub_rows.as_ref().map(|r| draw::measure_with(ctx, r));
    let sub = match (open.nav.sub, &sub_level) {
        (Some(i), Some(level)) => main.rows.get(i).map(|&(top, _)| (top, level)),
        _ => None,
    };
    let Some(laid) = lay_out(open.anchor, open.work, open.scale, &main, sub) else {
        // Not a size to give a window: no menu rather than a risk to the compositor.
        finish(&mut core, None, true);
        return;
    };
    if laid != open.laid || win::rect_of(hwnd) != Some(laid.window) {
        open.laid = laid;
        win::put(hwnd, laid.window);
        set_glass(&laid, open.scale);
        set_outside(&laid);
    }
    let scaled = (ppp - open.scale).abs() < 0.01;
    let composed = PANEL.with(|p| p.borrow().as_ref().is_some_and(GlassPanel::composed));

    // Where the levels are, in this frame's points.
    let origin = |r: &Px| Pos2::new((r.left - laid.window.left) as f32 / ppp, (r.top - laid.window.top) as f32 / ppp);
    let main_at = origin(&laid.main);
    let sub_at = laid.sub.as_ref().map(origin);

    // The pointer.
    let mut outcome = Outcome::Nothing;
    let hit = |p: Pos2| -> Option<(usize, Option<usize>)> {
        if let (Some(at), Some(level)) = (sub_at, &sub_level) {
            if let Some(row) = draw::row_at(level, at, p) {
                return Some((1, row));
            }
        }
        draw::row_at(&main, main_at, p).map(|row| (0, row))
    };
    if let Some(p) = input.pointer {
        let target = hit(p);
        if open.last_pointer != Some(p) {
            open.last_pointer = Some(p);
            if let Some((level, row)) = target {
                open.nav.hover(&open.rows, level, row, input.now);
            }
        }
        if input.pressed && target.is_some() {
            open.pressed_inside = true;
        }
        if input.released && open.pressed_inside {
            if let Some((level, Some(row))) = target {
                outcome = open.nav.click(&open.rows, level, row);
            }
        }
    }
    for key in &input.keys {
        if outcome == Outcome::Nothing {
            outcome = open.nav.key(&open.rows, *key);
        }
    }
    if let Some(wait) = open.nav.tick(&open.rows, input.now) {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait.max(0.01)));
    }
    open.had_focus |= input.focused;
    let lost_focus = open.had_focus && !input.focused;

    // Paint.
    let lit_main = draw::Lit { hot: open.nav.hot[0], open: open.nav.sub };
    let lit_sub = draw::Lit { hot: open.nav.hot[1], open: None };
    let st = style();
    egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
        let painter = ui.painter();
        if !composed {
            let shapes = islands(&laid, ppp);
            painter.extend(glass::islands::solid_shapes(&shapes, &st));
        }
        draw::paint(painter, main_at, &main, &open.rows, lit_main);
        if let (Some(at), Some(level), Some(rows)) = (sub_at, &sub_level, &sub_rows) {
            draw::paint(painter, at, level, rows, lit_sub);
        }
    });

    // Let it out once drawn at the monitor's scale.
    if let Some(n) = open.warm {
        if n + 1 >= WARM_FRAMES && (scaled || n >= WARM_CAP) {
            glass::cloak(hwnd, false);
            open.warm = None;
        } else {
            open.warm = Some(n + 1);
            ctx.request_repaint();
        }
    }

    // Closing.
    match outcome {
        Outcome::Pick(i) => finish(&mut core, Some(i), true),
        Outcome::Close => finish(&mut core, None, true),
        Outcome::Nothing if outside::take_click() || lost_focus => finish(&mut core, None, false),
        Outcome::Nothing => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Vec2;

    fn level(w: f32, h: f32) -> draw::Level {
        draw::Level { size: Vec2::new(w, h), rows: vec![(8.0, 40.0), (48.0, 40.0)], icons: true }
    }

    #[test]
    fn the_window_is_the_menu_or_the_menu_and_its_submenu() {
        let work = Px { left: 3840, top: 60, right: 7680, bottom: 2020 };
        let at = Anchor::at(4000, 500);
        let laid = lay_out(at, work, 1.5, &level(300.0, 400.0), None).unwrap();
        assert_eq!(laid.window, Px::new(4000, 500, 450, 600));
        let sub = level(240.0, 150.0);
        let laid = lay_out(at, work, 1.5, &level(300.0, 400.0), Some((48.0, &sub))).unwrap();
        assert_eq!(laid.sub.unwrap().left, 4000 + 450 + 9);
        assert_eq!(laid.window.width(), 450 + 9 + 360);
        assert_eq!(laid.window.height(), 600);
        // At 300 % on the left monitor, near its right edge: the submenu goes left, the window
        // stays the two pieces' box.
        let work = Px { left: -3840, top: 0, right: 0, bottom: 2160 };
        let laid = lay_out(Anchor::at(-1000, 300), work, 3.0, &level(300.0, 400.0), Some((48.0, &sub))).unwrap();
        assert!(laid.sub.unwrap().right <= laid.main.left);
        assert!(laid.window.right <= 0 && laid.window.left >= -3840);
        assert_eq!(laid.window.height(), 1200);
    }

    #[test]
    fn no_window_for_nonsense_sizes() {
        let work = Px { left: 0, top: 0, right: 3840, bottom: 2160 };
        assert!(lay_out(Anchor::at(10, 10), work, f32::NAN, &level(300.0, 400.0), None).is_none());
        assert!(lay_out(Anchor::at(10, 10), work, 1.5, &level(0.0, 400.0), None).is_none());
        // Too large is cut to the work area, never more.
        let laid = lay_out(Anchor::at(10, 10), work, 1.5, &level(5000.0, 5000.0), None).unwrap();
        assert_eq!((laid.window.width(), laid.window.height()), (3840, 2160));
    }

    #[test]
    fn islands_come_back_to_the_same_pixels() {
        let laid = Laid {
            main: Px::new(-3000, 100, 900, 1200),
            sub: Some(Px::new(-2073, 208, 720, 450)),
            window: Px::new(-3000, 100, 1647, 1200),
        };
        for island in islands(&laid, 3.0) {
            let (px, _) = glass::islands::to_pixels(&island, 3.0);
            assert!(px[2] - px[0] >= place::MIN_PX && px[3] - px[1] >= place::MIN_PX);
        }
        let list = islands(&laid, 3.0);
        assert_eq!(glass::islands::to_pixels(&list[1], 3.0).0, [927, 108, 1647, 558]);
    }
}
