//! Live window previews above the dock: rest the pointer on an icon that has windows and a glass
//! island shows each window's live picture, with its title and a close button; slide along the
//! dock to switch between the apps at once.
//!
//! The panel is a window of its own (the dock is only as tall as its icons). Each dock gets one,
//! made the first time that dock shows a preview and kept from then on: closing a preview cloaks
//! the window (with its glass), takes its glass pieces away so it takes no clicks, and lets go of
//! the live pictures, but the window stays. It is never made per hover, so the compositor never
//! sees glass come and go at the pace of the pointer (a `dwm.exe` crash came from that). Its size
//! goes through `glass::guard` (never tiny, never past the work area) and changes at most every
//! `RESIZE_GAP`; only the latest size is applied. One panel per dock also means each stays on its
//! own monitor, at its scale.
//!
//! A parked panel is a cloaked window that is still shown (a hidden egui window spins on a
//! pending frame) and asks for no frames, so it costs nothing. The only timers are the open delay,
//! the close grace and the watchdog (while a preview shows), asked for from the host's frames.
//!
//! Pieces: `hover` (when to open and close), `close` (the watchdog's rule), `layout` (tile and
//! panel geometry), `draw` (painting), `window` (Win32). This file ties them to the windows and
//! the live pictures.

mod close;
mod draw;
mod hover;
mod layout;
mod window;

pub use draw::Action;
use crate::dock::Hover as DockHover;
use crate::shape::surface;
use crate::{host, place, shape};
use egui::ViewportId;
use hover::{Key, Machine};
use layout::{Layout, MAX_WINDOWS};
use shellman_core::live_thumb::{fit, LiveThumb, PxRect};
use shellman_core::winlist::fullscreen::foreground_covers_monitor_of;
use shellman_core::winlist::AppWindow;
use shellman_ui::glass::{self, guard, Island, Style};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::time::{Duration, Instant};

/// Frames drawn before a newly shown panel is uncloaked: the first is complete, the second is
/// at the right scale. Capped, in case the scale never settles.
const WARM_FRAMES: u8 = 2;
const WARM_CAP: u8 = 20;
/// Space kept free at the work area's sides, in points.
const EDGE: f32 = 8.0;
/// The panel's corner radius: a little tighter than the dock's, it is a smaller surface.
pub const RADIUS: f32 = 18.0;
/// The least time between two changes of a panel's size.
const RESIZE_GAP: Duration = Duration::from_millis(40);

/// The panel's glass in the current theme.
pub fn style() -> Style {
    Style::from_theme(RADIUS)
}

/// The panel showing a preview now, for the fullscreen check; 0 when none does.
static PANEL: AtomicIsize = AtomicIsize::new(0);
/// A fullscreen app covered the panel's monitor: close it at the next host frame.
static COVERED: AtomicBool = AtomicBool::new(false);

/// With `SHELLMAN_PREVIEW_LOG=<file>`, what the panels do is written there.
fn trace(what: impl FnOnce() -> String) {
    use std::io::Write;
    if let Some(path) = std::env::var_os("SHELLMAN_PREVIEW_LOG") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{:?} {}", std::time::SystemTime::now(), what());
        }
    }
}

fn title_for(device: &str) -> String {
    format!("shellman preview {device}")
}

/// The panel window of `device`'s dock as the host declares it. It starts hidden, far away and
/// small: it is placed and shown once it exists and its size is known.
pub fn builder(device: &str) -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title(title_for(device))
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_drag_and_drop(false)
        .with_visible(false)
        .with_position([-30000.0, -30000.0])
        .with_inner_size([64.0, 64.0])
}

/// A fullscreen app may have come to the front: close the preview if it covers its monitor.
/// Called by the fullscreen check, which runs on the UI thread without access to the bars' state.
pub fn step_aside() {
    let panel = PANEL.load(Ordering::Acquire);
    if panel != 0 && foreground_covers_monitor_of(panel) {
        // Out of sight now, whatever the host does next; the host frame then parks it properly.
        glass::cloak(panel, true);
        COVERED.store(true, Ordering::Release);
        host::request_frame();
    }
}

/// One dock's panel window: declared from its first preview on, found once it exists.
pub struct Panel {
    pub device: String,
    pub id: ViewportId,
    hwnd: Option<isize>,
}

pub struct Preview {
    ctx: egui::Context,
    machine: Machine,
    panels: Vec<Panel>,
    /// The dock whose panel shows a preview now.
    current: Option<String>,
    /// The windows of the hovered app as last read, front-most first (at most `MAX_WINDOWS`).
    windows: Vec<AppWindow>,
    /// The windows the panel is laid out for and draws (they become `windows` when the panel
    /// is placed, which may wait for `RESIZE_GAP`).
    drawn: Vec<AppWindow>,
    layout: Layout,
    /// A live picture for each window that has one, registered with the panel's window.
    thumbs: Vec<LiveThumb>,
    /// The windows or the anchor changed since the panel was last placed.
    dirty: bool,
    pace: guard::Pace,
    /// Frames drawn since the panel was shown, while it is still cloaked.
    warm: Option<u8>,
    /// The hovered icon's dock window, for the watchdog.
    dock: Option<isize>,
    /// The panel has been shown for this preview.
    shown: bool,
    /// Since when the cursor has been outside the dock and the panel.
    outside: close::Outside,
    /// The foreground window when the panel opened.
    fg_at_open: isize,
    /// How many windows the app had at the last look; `None` before the first.
    app_windows: Option<usize>,
}

impl Preview {
    pub fn new(ctx: egui::Context) -> Self {
        Self {
            ctx,
            machine: Machine::new(),
            panels: Vec::new(),
            current: None,
            windows: Vec::new(),
            drawn: Vec::new(),
            layout: Layout::default(),
            thumbs: Vec::new(),
            dirty: false,
            pace: guard::Pace::new(RESIZE_GAP),
            warm: None,
            dock: None,
            shown: false,
            outside: close::Outside::default(),
            fg_at_open: 0,
            app_windows: None,
        }
    }

    /// The panel windows to declare: every dock that has shown a preview.
    pub fn panels(&self) -> impl Iterator<Item = &Panel> {
        self.panels.iter()
    }

    /// The panel showing a preview now, to wake when the app's windows change.
    pub fn current_id(&self) -> Option<ViewportId> {
        let device = self.current.as_deref()?;
        self.panels.iter().find(|p| p.device == device).map(|p| p.id)
    }

    /// Forget the panels of docks that are gone (their monitor went away); the host stops
    /// declaring them and their windows close.
    pub fn keep_docks(&mut self, devices: &[&str]) {
        if self.current.as_deref().is_some_and(|d| !devices.contains(&d)) {
            self.machine.dismiss(None);
            self.release();
        }
        self.panels.retain(|p| devices.contains(&p.device.as_str()));
    }

    /// The app whose windows the panel shows, while it is open.
    pub fn app_id(&self) -> Option<String> {
        self.machine.key().map(|k| k.app_id.clone())
    }

    /// What the pointer did on `device`'s dock window `dock` (drawn at `ppp`) this frame.
    /// `blocked`: a menu is open, so nothing is previewed.
    pub fn hover_dock(&mut self, device: &str, hover: DockHover, dock: isize, ppp: f32, blocked: bool) {
        let now = Instant::now();
        if hover.target.is_some() {
            trace(|| format!("hover {device} blocked={blocked} dismiss={}", hover.dismiss));
        }
        let (was_open, epoch) = (self.machine.is_open(), self.machine.epoch());
        match hover.target {
            Some(_) if blocked => self.machine.dismiss(None),
            Some(target) => {
                self.dock = Some(dock);
                let key = Key { device: device.to_owned(), app_id: target.app_id };
                if hover.dismiss {
                    self.machine.dismiss(Some(key));
                } else {
                    let (center, top) = (target.center_x, target.top);
                    self.machine.hover(now, key, target.has_windows, || window::anchor(dock, center, top, ppp));
                }
            }
            None if hover.dismiss => self.machine.dismiss(None),
            None if !hover.inside => self.machine.leave_dock(now, device),
            // Between two icons: nothing changes.
            None => {}
        }
        self.changed(was_open, epoch, now);
    }

    /// Close at once (a preview was clicked).
    pub fn dismiss(&mut self) {
        let (was_open, epoch) = (self.machine.is_open(), self.machine.epoch());
        self.machine.dismiss(None);
        self.changed(was_open, epoch, Instant::now());
    }

    /// After something moved the machine: wake the host if the panel must appear or go, and the
    /// panel if it must change; and ask to be woken when a timer is due.
    fn changed(&mut self, was_open: bool, epoch: u32, now: Instant) {
        if self.machine.is_open() != was_open {
            host::request_frame();
        } else if self.machine.is_open() && self.machine.epoch() != epoch {
            self.dirty = true;
            host::request_frame();
        }
        self.schedule(now);
    }

    fn schedule(&self, now: Instant) {
        if let Some(due) = self.machine.deadline(now) {
            self.ctx.request_repaint_after_for(due, ViewportId::ROOT);
        }
    }

    /// In the host's frame: let the timers run out, and make sure the open preview's dock has a
    /// panel window to declare. True while a preview shows.
    pub fn tick(&mut self, now: Instant) -> bool {
        if COVERED.swap(false, Ordering::AcqRel) {
            self.machine.dismiss(None);
        }
        let was_open = self.machine.is_open();
        self.machine.tick(now);
        let open = self.machine.is_open();
        if open && !was_open {
            trace(|| format!("open {:?}", self.machine.key()));
            self.dirty = true;
            self.fg_at_open = window::foreground();
            self.outside.reset();
            self.app_windows = None;
        }
        // The authoritative check, on its own timer: it does not depend on any pointer event or
        // on a frame of a window that may be hidden.
        let open = open && !self.watchdog(now);
        let device = self.machine.key().map(|k| k.device.clone());
        if !open || self.current.is_some() && self.current != device {
            // Closed, or moved on to another monitor's dock.
            self.release();
        }
        if let (true, Some(device)) = (open, device) {
            if !self.panels.iter().any(|p| p.device == device) {
                let id = ViewportId::from_hash_of(("shellman preview panel", &device));
                self.panels.push(Panel { device: device.clone(), id, hwnd: None });
            }
            if self.current.as_deref() != Some(device.as_str()) {
                self.dirty = true;
                self.current = Some(device);
            }
        }
        self.schedule(now);
        if open {
            // Only while a preview shows; a parked panel costs nothing.
            self.ctx.request_repaint_after_for(close::WATCH_PERIOD, ViewportId::ROOT);
        }
        open
    }

    /// Read the facts and close the panel if they say so. True if it was closed.
    fn watchdog(&mut self, now: Instant) -> bool {
        if !self.machine.is_open() {
            return false;
        }
        let dock_shown = self.dock.is_some_and(window::is_shown);
        let hwnd = self.hwnd();
        let mut zones = Vec::with_capacity(2);
        zones.extend(self.dock.and_then(window::rect_of));
        if self.shown {
            zones.extend(hwnd.and_then(window::rect_of));
        }
        let outside_for = match window::cursor() {
            Some(p) => self.outside.update(now, close::inside(&zones, p)),
            None => None,
        };
        let fg = window::foreground();
        let monitor_of = self.dock.filter(|_| dock_shown).or(hwnd).unwrap_or(0);
        let facts = close::Facts {
            outside_for,
            dock_hidden: !dock_shown,
            covered: monitor_of != 0 && foreground_covers_monitor_of(monitor_of),
            windows_gone: self.app_windows == Some(0),
            foreground_changed: fg != 0 && self.fg_at_open != 0 && fg != self.fg_at_open,
            panel_hidden: self.shown && !hwnd.is_some_and(window::is_shown),
        };
        let Some(reason) = close::decide(&facts, hover::GRACE) else { return false };
        trace(|| format!("watchdog closes: {reason:?}"));
        self.machine.dismiss(None);
        true
    }

    /// The window of the panel showing a preview now.
    fn hwnd(&self) -> Option<isize> {
        let device = self.current.as_deref()?;
        self.panels.iter().find(|p| p.device == device)?.hwnd
    }

    /// Park the panel: let go of the pictures, cloak the window with its glass and take its glass
    /// pieces away (so it takes no clicks), keeping the window and its size.
    fn release(&mut self) {
        if self.current.is_none() && self.thumbs.is_empty() {
            return;
        }
        trace(|| "park".into());
        // Pictures first: they are registered with the window.
        self.thumbs.clear();
        if let Some(hwnd) = self.hwnd() {
            glass::cloak(hwnd, true);
            surface::set_islands(hwnd, &[], 1.0, true, None);
        }
        PANEL.store(0, Ordering::Release);
        self.current = None;
        self.shown = false;
        self.windows.clear();
        self.drawn.clear();
        self.layout = Layout::default();
        self.warm = None;
    }

    /// In the host's frame, after it declared the panels: find windows that were just made and
    /// give them their looks, cloaked and with no glass shown. False while one is still to come.
    pub fn adopt(&mut self) -> bool {
        let mut all = true;
        for panel in self.panels.iter_mut().filter(|p| p.hwnd.is_none()) {
            let Some(hwnd) = shape::own_window(&title_for(&panel.device)) else {
                all = false;
                continue;
            };
            // Cloaked from the first moment, glass and all.
            glass::cloak(hwnd, true);
            shape::never_activate(hwnd);
            shape::see_through(hwnd);
            surface::attach(hwnd, style());
            surface::set_islands(hwnd, &[], 1.0, true, None);
            window::show(hwnd);
            panel.hwnd = Some(hwnd);
            trace(|| format!("adopted {hwnd:#x} for {}", panel.device));
            self.dirty = true;
        }
        all
    }

    /// Take the app's windows as they are now and lay the panel out for them. The host calls this
    /// in each of its frames while a preview shows.
    pub fn update(&mut self, mut windows: Vec<AppWindow>) {
        if !self.machine.is_open() {
            return;
        }
        windows.truncate(MAX_WINDOWS);
        self.app_windows = Some(windows.len());
        if windows.is_empty() {
            // The app's last window is gone.
            let (was_open, epoch) = (self.machine.is_open(), self.machine.epoch());
            self.machine.dismiss(None);
            self.changed(was_open, epoch, Instant::now());
            return;
        }
        let same = self.drawn.len() == windows.len()
            && self.drawn.iter().zip(&windows).all(|(a, b)| a.hwnd == b.hwnd && a.minimized == b.minimized);
        self.dirty |= !same;
        // Titles may change without a new layout.
        if same {
            self.drawn.clone_from(&windows);
        }
        self.windows = windows;
        self.settle();
    }

    /// In `device`'s panel frame: draw, if it shows a preview. `icons` loads app icons. Returns
    /// what was clicked.
    pub fn frame(&mut self, ctx: &egui::Context, device: &str, icons: draw::Icons) -> Option<Action> {
        let mine = self.current.as_deref() == Some(device);
        let hwnd = self.panels.iter().find(|p| p.device == device).and_then(|p| p.hwnd);
        let (Some(hwnd), Some(anchor), true) = (hwnd, self.machine.anchor(), mine) else {
            // A parked panel draws nothing.
            egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |_| {});
            return None;
        };
        let now = Instant::now();
        let (was_open, epoch) = (self.machine.is_open(), self.machine.epoch());
        self.machine.panel_pointer(now, ctx.pointer_hover_pos().is_some());
        self.changed(was_open, epoch, now);
        if !self.shown || self.layout.tiles.len() != self.drawn.len() {
            return None;
        }
        let live: Vec<bool> = self.drawn.iter().map(|w| self.thumb_of(w).is_some()).collect();
        let k = anchor.scale / ctx.pixels_per_point();
        surface::set_style(hwnd, style());
        let composed = surface::composed(hwnd);
        let view = draw::View { windows: &self.drawn, layout: &self.layout, live: &live, composed, k };
        let action = draw::draw(ctx, &view, icons);
        // The glass is the whole window, as one island (none while parked).
        let full = ctx.screen_rect();
        surface::set_islands(hwnd, &[Island::new(full, RADIUS)], ctx.pixels_per_point(), true, None);
        self.warm_up(ctx, hwnd, (k - 1.0).abs() < 0.001);
        action
    }

    /// Uncloak once the panel has been drawn, at the right scale, a couple of times.
    fn warm_up(&mut self, ctx: &egui::Context, hwnd: isize, scaled: bool) {
        let Some(n) = self.warm else { return };
        if n + 1 >= WARM_FRAMES && (scaled || n >= WARM_CAP) {
            glass::cloak(hwnd, false);
            PANEL.store(hwnd, Ordering::Release);
            self.warm = None;
        } else {
            self.warm = Some(n + 1);
            ctx.request_repaint();
        }
    }

    /// The live picture of a window, if the compositor gives one worth showing. A minimized
    /// window has none: Windows keeps no picture of it, only a caption-sized stub (unless its
    /// app supplies an iconic bitmap, which then comes through the same thumbnail).
    fn thumb_of(&self, window: &AppWindow) -> Option<&LiveThumb> {
        self.thumbs
            .iter()
            .find(|t| t.source() == window.hwnd)
            .filter(|t| layout::worth_showing(t.source_size()))
    }

    /// Lay the panel out for its windows and put the window and the pictures in place.
    fn settle(&mut self) {
        if !self.dirty {
            return;
        }
        let (Some(hwnd), Some(anchor)) = (self.hwnd(), self.machine.anchor()) else { return };
        if self.windows.is_empty() {
            return;
        }
        // Pictures: keep those of windows still shown, add the new.
        let windows = &self.windows;
        self.thumbs.retain(|t| windows.iter().any(|w| w.hwnd == t.source()));
        for w in windows.iter() {
            if !self.thumbs.iter().any(|t| t.source() == w.hwnd) {
                self.thumbs.extend(LiveThumb::register(hwnd, w.hwnd));
            }
        }
        let sizes: Vec<_> = self
            .windows
            .iter()
            .map(|w| layout::preview_size(self.thumb_of(w).and_then(|t| t.source_size())))
            .collect();
        let work = anchor.work;
        let new_layout = layout::arrange(&sizes, layout::max_width(work.right - work.left, anchor.scale));
        let edge = (EDGE * anchor.scale).round() as i32;
        let Some(size) = guard::popup(
            layout::size_px(&new_layout, anchor.scale),
            (work.right - work.left, work.bottom - work.top),
        ) else {
            return;
        };
        let rect = layout::panel_rect(anchor.center_x, anchor.bottom, size, work, edge);
        if window::rect_of(hwnd) != Some(rect) {
            if let Err(wait) = self.pace.take(Instant::now()) {
                // Too soon after the last change: try again then, with whatever is latest.
                self.ctx.request_repaint_after_for(wait, ViewportId::ROOT);
                return;
            }
            place::put(hwnd, rect);
            trace(|| format!("placed {hwnd:#x} at {rect:?}"));
        }
        self.dirty = false;
        self.layout = new_layout;
        self.drawn.clone_from(&self.windows);
        // Each picture over its tile, in the panel's own pixels.
        let s = anchor.scale;
        for thumb in &self.thumbs {
            let tile = self.drawn.iter().position(|w| w.hwnd == thumb.source()).map(|i| self.layout.tiles[i]);
            match (tile, thumb.source_size().filter(|&s| layout::worth_showing(Some(s)))) {
                (Some(tile), Some(size)) => {
                    let area = PxRect {
                        x: tile.preview.min.x * s,
                        y: tile.preview.min.y * s,
                        w: tile.preview.width() * s,
                        h: tile.preview.height() * s,
                    };
                    thumb.show(fit(size, area));
                }
                _ => thumb.hide(),
            }
        }
        window::show(hwnd);
        if !self.shown {
            self.shown = true;
            self.warm = Some(0);
        }
        if let Some(id) = self.current_id() {
            self.ctx.request_repaint_of(id);
        }
    }
}
