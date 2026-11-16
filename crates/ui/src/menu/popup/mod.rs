//! Shellman's own right-click menu as a popup window of its own, on glass: every menu of every
//! shellman app (the desktop's, the dock's, Files', the dropdowns in Settings) is this one.
//!
//! # Using it
//!
//! ```ignore
//! use shellman_ui::menu::popup::{self, Anchor, Entry, PopupMenu};
//! // Every frame of the app's root viewport (it keeps the menu window alive):
//! popup::declare(ctx);
//! // On a right-click, at a point in physical screen pixels (or `Anchor::in_window` for a point
//! // of the app's window, `Anchor::under` for a dropdown under a button):
//! self.menu.open(ctx, Anchor::at(x, y), vec![
//!     Entry::item(Action::Paste, "&Paste").icon(glyph::PASTE).hint("Ctrl+V").enabled(can_paste),
//!     Entry::separator(),
//!     Entry::item(Action::Delete, "Delete").danger(),
//! ]);
//! // In any later frame of the viewport that opened it (it is woken when something is picked):
//! if let Some(action) = self.menu.take_pick() { ... }
//! ```
//!
//! Entries can carry a glyph or a picture, a hint (a shortcut), a tick, be disabled or
//! destructive; there are separators, headings, quiet notes ("Loading…"), pictures drawn by
//! someone else, and submenus three levels deep, which may be filled on demand
//! ([`Entry::on_open`], [`PopupMenu::take_opened`]). While open, the caller can replace the
//! entries ([`PopupMenu::update`]): rows that arrive late appear in place. A level taller than
//! the monitor scrolls (wheel, or the keyboard following the lit row).
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
//! It is exactly as large as the bounding box of the menu and its open submenus (they are
//! islands of the same window); the glass's shadow lies outside it, in the glass's own backdrop
//! window. Every size goes through [`place::guard`] first: DWM fails fast on a composition
//! surface of size 0, negative, or above 16384 (the 12 Nov compositor crashes), and a
//! monitor-sized popup makes the bar step aside as for a fullscreen app. Every piece stays in
//! the work area of the monitor it opened on, at that monitor's scale.
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

pub use model::{Entry, Item, MAX_LEVELS};
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
    /// For a menu dropped from a button: where it goes when it flips to the other side (the
    /// button's top, or its bottom for one that opens upwards), so it never covers the button.
    pub flip: Option<i32>,
}

impl Anchor {
    /// The menu's top-left at the point (flipped at the monitor's edges).
    pub fn at(x: i32, y: i32) -> Self {
        Self { x, y, up: false, flip: None }
    }

    /// The menu's bottom-left at the point.
    pub fn above(x: i32, y: i32) -> Self {
        Self { x, y, up: true, flip: None }
    }

    /// A point of the current viewport (in its points), as physical screen pixels.
    pub fn screen_point(ctx: &Context, p: Pos2) -> (i32, i32) {
        let ppp = ctx.pixels_per_point();
        let origin = ctx.input(|i| i.viewport().inner_rect.map(|r| r.min)).unwrap_or(Pos2::ZERO);
        (((origin.x + p.x) * ppp).round() as i32, ((origin.y + p.y) * ppp).round() as i32)
    }

    /// At a point of the current viewport: where a right click landed, or the selection for the
    /// Menu key and Shift+F10.
    pub fn in_window(ctx: &Context, p: Pos2) -> Self {
        let (x, y) = Self::screen_point(ctx, p);
        Self::at(x, y)
    }

    /// Dropped from a button (`rect`, in the current viewport's points): under it, its left
    /// edges lined up, or above it when there is no room below.
    pub fn under(ctx: &Context, rect: Rect) -> Self {
        let (x, top) = Self::screen_point(ctx, rect.left_top());
        let (_, bottom) = Self::screen_point(ctx, rect.left_bottom());
        Self { x, y: bottom, up: false, flip: Some(top) }
    }

    /// Raised from a button near the bottom of a window: above it, or under it when there is
    /// no room above.
    pub fn over(ctx: &Context, rect: Rect) -> Self {
        let (x, top) = Self::screen_point(ctx, rect.left_top());
        let (_, bottom) = Self::screen_point(ctx, rect.left_bottom());
        Self { x, y: top, up: true, flip: Some(bottom) }
    }
}

/// A caller's handle: what it opened, and the values to hand back. Cheap; keep one per kind of
/// menu in the app's state.
#[derive(Clone)]
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

    /// Replace the open menu's entries (rows that arrived late, a submenu filled on demand),
    /// keeping its place and what is lit where it still exists. Does nothing when this handle's
    /// menu is not open, and costs no frame when nothing changed. Take a pick first: a pick's
    /// value is looked up among the entries as they are when it is taken.
    pub fn update(&mut self, entries: Vec<Entry<T>>) {
        let mut core = lock();
        let token = self.token;
        let Some(open) = core.open.as_mut().filter(|o| o.token == token && token != 0) else { return };
        let mut ids = Vec::with_capacity(self.ids.len());
        let rows = model::flatten(entries, &mut ids, 0);
        self.ids = ids;
        if rows != open.rows {
            open.rows = rows;
            open.nav.fit(&open.rows);
            if let Some(ctx) = &core.ctx {
                ctx.request_repaint_of(viewport_id());
            }
        }
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

    /// The `on_open` value of a submenu of this handle's menu that just opened, once.
    pub fn take_opened(&mut self) -> Option<T> {
        let mut core = lock();
        match core.opened {
            Some((token, i)) if token == self.token && token != 0 => {
                core.opened = None;
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

/// Whether any menu of this process is open (an app's own keys wait while one is).
pub fn any_open() -> bool {
    lock().open.is_some()
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
    ctx.show_viewport_deferred(viewport_id(), builder, |ctx, _| {
        frame(ctx);
        // The menu is never the root window: it names its rows to screen readers itself.
        crate::access::publish(ctx);
    });
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
    debug_open_path(&[row]);
}

/// For tests and the example: open the submenus along `path` (a row of the menu, then a row of
/// that submenu, ...).
#[doc(hidden)]
pub fn debug_open_path(path: &[usize]) {
    let mut core = lock();
    let ctx = core.ctx.clone();
    if let Some(open) = core.open.as_mut() {
        let mut nav = Nav::default();
        for (level, &row) in path.iter().enumerate() {
            let is_sub = nav::level_rows(&open.rows, &nav.chain, level).and_then(|r| r.get(row)).is_some_and(|r| r.submenu().is_some());
            if !is_sub || level + 1 >= MAX_LEVELS {
                break;
            }
            nav.chain.push(row);
            nav.hot[level] = Some(row);
            nav.hot.push(None);
            nav.level = level + 1;
        }
        open.nav = nav;
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
    /// The token of the menu a submenu with an `on_open` value opened in, and the value's index.
    opened: Option<(u64, usize)>,
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
    /// Each open level's scroll, in points, and the submenus they were for.
    scroll: Vec<f32>,
    scroll_chain: Vec<usize>,
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
#[derive(Clone, PartialEq, Debug)]
struct Laid {
    /// The menu, then each open submenu.
    levels: Vec<Px>,
    window: Px,
}

static CORE: Mutex<Core> =
    Mutex::new(Core { started: false, ctx: None, hwnd: None, open: None, pick: None, opened: None, next_token: 1 });

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

/// The tallest a level may be, in points: its monitor's work area.
fn max_height(work: &Px, scale: f32) -> f32 {
    work.height() as f32 / scale.max(0.25)
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
    let level = draw::measure_with(ctx, &rows, max_height(&work, scale));
    let Some(laid) = lay_out(at, work, scale, &[&level], &[]) else { return token };
    core.open = Some(Open {
        token,
        rows,
        anchor: at,
        work,
        scale,
        nav: Nav::default(),
        scroll: vec![0.0],
        scroll_chain: Vec::new(),
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

/// Place the menu and its open submenus for the anchor, in pixels: `levels` from the menu down,
/// `tops[k]` the top (points, in level `k` as scrolled) of the row of level `k` that opened
/// level `k + 1`. None when a size is not fit to give a window.
fn lay_out(at: Anchor, work: Px, scale: f32, levels: &[&draw::Level], tops: &[f32]) -> Option<Laid> {
    let limit = (work.width(), work.height());
    let first = levels.first()?;
    let size = place::guard(first.size.x, first.size.y, scale, limit)?;
    let mut out = vec![place::place_menu((at.x, at.y), size, at.up, at.flip, work)];
    let px = |pt: f32| (pt * scale).round() as i32;
    let mut leftward = false;
    for (k, level) in levels.iter().enumerate().skip(1) {
        let size = place::guard(level.size.x, level.size.y, scale, limit)?;
        let parent = out[k - 1];
        let row_top = parent.top + px(*tops.get(k - 1)?).clamp(0, parent.height());
        let (piece, left) = place::place_submenu(parent, row_top, px(draw::PAD), size, px(draw::SUB_GAP), work, leftward);
        leftward = left;
        out.push(piece);
    }
    let window = out.iter().fold(out[0], |a, b| a.union(b));
    place::sane(&window, &work).then_some(Laid { levels: out, window })
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
    laid.levels.iter().map(|r| Island::new(to_pt(r), draw::RADIUS)).collect()
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
    let rects: Vec<_> = laid.levels.iter().map(|r| (r.left, r.top, r.right, r.bottom)).collect();
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
    /// The wheel, in points (positive: content moves down, so the view goes up).
    wheel: f32,
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
                        K::Home | K::PageUp => Key::Home,
                        K::End | K::PageDown => Key::End,
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
            wheel: i.raw_scroll_delta.y,
            now: i.time,
        }
    })
}

/// Each open level's scroll, kept for the same submenus and cleared for new ones.
fn sync_scroll(scroll: &mut Vec<f32>, was: &mut Vec<usize>, chain: &[usize], levels: &[draw::Level]) {
    let same = was.iter().zip(chain).take_while(|(a, b)| a == b).count();
    scroll.truncate(same + 1);
    scroll.resize(levels.len(), 0.0);
    for (s, level) in scroll.iter_mut().zip(levels) {
        *s = s.clamp(0.0, level.max_scroll());
    }
    *was = chain.to_vec();
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

    // Lay out at this frame's fonts: the menu and the submenus the navigation has open.
    open.nav.fit(&open.rows);
    let max_h = max_height(&open.work, open.scale);
    let level_rows: Vec<&[Row]> =
        (0..open.nav.levels()).map_while(|l| nav::level_rows(&open.rows, &open.nav.chain, l)).collect();
    let levels: Vec<draw::Level> = level_rows.iter().map(|r| draw::measure_with(ctx, r, max_h)).collect();
    sync_scroll(&mut open.scroll, &mut open.scroll_chain, &open.nav.chain, &levels);
    let tops: Vec<f32> = open
        .nav
        .chain
        .iter()
        .zip(&levels)
        .zip(&open.scroll)
        .map(|((&row, level), &s)| level.rows.get(row).map_or(0.0, |&(top, _)| top - s))
        .collect();
    let refs: Vec<&draw::Level> = levels.iter().collect();
    let Some(laid) = lay_out(open.anchor, open.work, open.scale, &refs, &tops) else {
        // Not a size to give a window: no menu rather than a risk to the compositor.
        finish(&mut core, None, true);
        return;
    };
    if laid != open.laid || win::rect_of(hwnd) != Some(laid.window) {
        win::put(hwnd, laid.window);
        set_glass(&laid, open.scale);
        set_outside(&laid);
        open.laid = laid.clone();
    }
    let scaled = (ppp - open.scale).abs() < 0.01;
    let composed = PANEL.with(|p| p.borrow().as_ref().is_some_and(GlassPanel::composed));

    // Where the levels are, in this frame's points.
    let origin = |r: &Px| Pos2::new((r.left - laid.window.left) as f32 / ppp, (r.top - laid.window.top) as f32 / ppp);
    let at: Vec<Pos2> = laid.levels.iter().map(origin).collect();

    // The pointer: the deepest level under it.
    let before = (open.nav.clone(), open.scroll.clone());
    let mut outcome = Outcome::Nothing;
    let hit = |p: Pos2, scroll: &[f32]| -> Option<(usize, Option<usize>)> {
        (0..levels.len()).rev().find_map(|l| draw::row_at(&levels[l], at[l], scroll[l], p).map(|row| (l, row)))
    };
    if let Some(p) = input.pointer {
        if input.wheel != 0.0 {
            if let Some((l, _)) = hit(p, &open.scroll) {
                open.scroll[l] = (open.scroll[l] - input.wheel).clamp(0.0, levels[l].max_scroll());
                // The row under the pointer changed: light the new one.
                open.last_pointer = None;
            }
        }
        let target = hit(p, &open.scroll);
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
    let typed = !input.keys.is_empty();
    for key in &input.keys {
        if outcome == Outcome::Nothing {
            outcome = open.nav.key(&open.rows, *key);
        }
    }
    if typed {
        // The keyboard's row in view.
        let l = open.nav.level;
        if let (Some(Some(hot)), Some(level), Some(s)) = (open.nav.hot.get(l), levels.get(l), open.scroll.get_mut(l)) {
            *s = level.reveal(*hot, *s);
        }
    }
    if let Some(wait) = open.nav.tick(&open.rows, input.now) {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait.max(0.01)));
    }
    // A submenu that opened with a value for its caller (to fill it).
    let mut opened = None;
    if open.nav.chain != before.0.chain {
        let same = before.0.chain.iter().zip(&open.nav.chain).take_while(|(a, b)| a == b).count();
        for l in same..open.nav.chain.len() {
            let row = nav::level_rows(&open.rows, &open.nav.chain, l).and_then(|r| r.get(open.nav.chain[l]));
            if let Some(id) = row.and_then(|r| r.opens) {
                opened = Some(id);
            }
        }
    }
    if open.nav != before.0 || open.scroll != before.1 {
        // Laid out again for what changed.
        ctx.request_repaint();
    }
    open.had_focus |= input.focused;
    let lost_focus = open.had_focus && !input.focused;

    // Paint, for the levels laid out above.
    let st = style();
    egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
        let painter = ui.painter();
        if !composed {
            let shapes = islands(&laid, ppp);
            painter.extend(glass::islands::solid_shapes(&shapes, &st));
        }
        for (l, rows) in level_rows.iter().enumerate() {
            let lit = draw::Lit {
                hot: open.nav.hot.get(l).copied().flatten(),
                open: open.nav.chain.get(l).copied(),
            };
            let scroll = open.scroll.get(l).copied().unwrap_or(0.0);
            let hot = lit.hot;
            draw::paint(painter, at[l], &levels[l], rows, lit, scroll);
            draw::speak(ui, at[l], &levels[l], rows, hot, scroll, l);
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
    if let Some(id) = opened {
        let (token, opener) = (open.token, open.opener);
        core.opened = Some((token, id));
        if let Some(c) = &core.ctx {
            c.request_repaint_of(opener);
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
        draw::Level { size: Vec2::new(w, h), content: h, rows: vec![(8.0, 40.0), (48.0, 40.0)], icons: true }
    }

    #[test]
    fn the_window_is_the_menu_or_the_menu_and_its_submenus() {
        let work = Px { left: 3840, top: 60, right: 7680, bottom: 2020 };
        let at = Anchor::at(4000, 500);
        let main = level(300.0, 400.0);
        let laid = lay_out(at, work, 1.5, &[&main], &[]).unwrap();
        assert_eq!(laid.window, Px::new(4000, 500, 450, 600));
        let sub = level(240.0, 150.0);
        let laid = lay_out(at, work, 1.5, &[&main, &sub], &[48.0]).unwrap();
        assert_eq!(laid.levels[1].left, 4000 + 450 + 9);
        assert_eq!(laid.window.width(), 450 + 9 + 360);
        assert_eq!(laid.window.height(), 600);
        // At 300 % on the left monitor, near its right edge: the submenu goes left, the window
        // stays the pieces' box.
        let work = Px { left: -3840, top: 0, right: 0, bottom: 2160 };
        let laid = lay_out(Anchor::at(-1000, 300), work, 3.0, &[&main, &sub], &[48.0]).unwrap();
        assert!(laid.levels[1].right <= laid.levels[0].left);
        assert!(laid.window.right <= 0 && laid.window.left >= -3840);
        assert_eq!(laid.window.height(), 1200);
        // Three levels near the right edge cascade left, each beside the one before.
        let laid = lay_out(Anchor::at(-500, 300), work, 1.5, &[&main, &sub, &sub, &sub], &[48.0, 8.0, 8.0]).unwrap();
        for k in 1..4 {
            assert!(laid.levels[k].right <= laid.levels[k - 1].left, "{laid:?}");
        }
        assert!(laid.window.left >= work.left && laid.window.right <= work.right);
    }

    #[test]
    fn a_submenu_follows_its_row_as_the_menu_scrolls() {
        let work = Px { left: 0, top: 0, right: 3840, bottom: 2160 };
        let main = level(300.0, 400.0);
        let sub = level(240.0, 150.0);
        let a = lay_out(Anchor::at(100, 100), work, 1.0, &[&main, &sub], &[200.0]).unwrap();
        let b = lay_out(Anchor::at(100, 100), work, 1.0, &[&main, &sub], &[120.0]).unwrap();
        assert_eq!(a.levels[1].top - b.levels[1].top, 80);
        // Scrolled out of view above: it stays level with the menu's top, not above it.
        let c = lay_out(Anchor::at(100, 100), work, 1.0, &[&main, &sub], &[-500.0]).unwrap();
        assert_eq!(c.levels[1].top, 100 - 8);
    }

    #[test]
    fn no_window_for_nonsense_sizes() {
        let work = Px { left: 0, top: 0, right: 3840, bottom: 2160 };
        assert!(lay_out(Anchor::at(10, 10), work, f32::NAN, &[&level(300.0, 400.0)], &[]).is_none());
        assert!(lay_out(Anchor::at(10, 10), work, 1.5, &[&level(0.0, 400.0)], &[]).is_none());
        assert!(lay_out(Anchor::at(10, 10), work, 1.5, &[], &[]).is_none());
        // A submenu without the row it opens from is no layout.
        assert!(lay_out(Anchor::at(10, 10), work, 1.5, &[&level(300.0, 400.0), &level(300.0, 400.0)], &[]).is_none());
        // Too large is cut to the work area, never more.
        let laid = lay_out(Anchor::at(10, 10), work, 1.5, &[&level(5000.0, 5000.0)], &[]).unwrap();
        assert_eq!((laid.window.width(), laid.window.height()), (3840, 2160));
    }

    #[test]
    fn islands_come_back_to_the_same_pixels() {
        let laid = Laid {
            levels: vec![Px::new(-3000, 100, 900, 1200), Px::new(-2073, 208, 720, 450)],
            window: Px::new(-3000, 100, 1647, 1200),
        };
        for island in islands(&laid, 3.0) {
            let (px, _) = glass::islands::to_pixels(&island, 3.0);
            assert!(px[2] - px[0] >= place::MIN_PX && px[3] - px[1] >= place::MIN_PX);
        }
        let list = islands(&laid, 3.0);
        assert_eq!(glass::islands::to_pixels(&list[1], 3.0).0, [927, 108, 1647, 558]);
    }

    #[test]
    fn scrolls_are_kept_for_the_same_submenus_only() {
        let lv = |h: f32| draw::Level { size: Vec2::new(300.0, 100.0), content: h, rows: vec![], icons: false };
        let levels = vec![lv(500.0), lv(500.0), lv(500.0)];
        let mut scroll = vec![50.0, 60.0, 70.0];
        let mut was = vec![1, 2];
        sync_scroll(&mut scroll, &mut was, &[1, 2], &levels);
        assert_eq!(scroll, [50.0, 60.0, 70.0]);
        // Another submenu from the first level: its own and deeper scrolls start at the top.
        sync_scroll(&mut scroll, &mut was, &[1, 3], &levels);
        assert_eq!(scroll, [50.0, 60.0, 0.0]);
        // Never past the end.
        let short = vec![lv(120.0)];
        sync_scroll(&mut scroll, &mut was, &[], &short);
        assert_eq!(scroll, [20.0]);
    }
}
