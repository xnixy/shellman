//! Live window previews above the dock: hover an icon that has windows and a glass panel shows
//! each window's live picture, with its title and a close button; slide along the dock to scrub
//! through the apps.
//!
//! The panel is its own window (the dock is only as tall as its icons), one per bar process,
//! shown over whichever monitor's dock is hovered. It is an egui viewport that exists only while
//! it is open: the host declares it in its frames while `tick` says so, and stops declaring it to
//! close it. So a closed panel is not a hidden window that gets no frames and would have to be
//! woken with tricks (as the switcher's must be): it does not exist, and costs nothing. The price
//! is creating the window when it opens, which the open delay hides. Scrubbing reuses the open
//! window: it is moved and resized, not recreated.
//!
//! No polling: the only timers are the open delay and the close grace, asked for from the host's
//! frames with `request_repaint_after_for` (a timer asked for from another viewport's frame
//! would run that viewport's frame instead).
//!
//! Pieces: `hover` (when to open and close), `layout` (tile and panel geometry), `draw` (painting),
//! `window` (Win32). This file ties them to the window and the live pictures.

mod draw;
mod hover;
mod layout;
mod window;

pub use draw::Action;
use crate::dock::Hover as DockHover;
use crate::{host, place, shape};
use egui::ViewportId;
use hover::{Key, Machine};
use layout::{Layout, MAX_WINDOWS};
use shellman_core::live_thumb::{fit, LiveThumb, PxRect};
use shellman_core::winlist::fullscreen::foreground_covers_monitor_of;
use shellman_core::winlist::AppWindow;
use shellman_ui::window::frost_window;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::time::Instant;
use windows::Win32::Foundation::HWND;

const TITLE: &str = "shellman preview";
/// Frames drawn before a newly shown panel is uncloaked: the first is complete, the second is
/// at the right scale. Capped, in case the scale never settles.
const WARM_FRAMES: u8 = 2;
const WARM_CAP: u8 = 20;
/// Space kept free at the monitor's sides, in points.
const EDGE: f32 = 8.0;

/// The open panel's window, for the fullscreen check; 0 when there is none.
static PANEL: AtomicIsize = AtomicIsize::new(0);
/// A fullscreen app covered the panel's monitor: close it at the next host frame.
static COVERED: AtomicBool = AtomicBool::new(false);

pub fn id() -> ViewportId {
    ViewportId::from_hash_of("shellman preview panel")
}

/// The panel's window as the host declares it. It starts hidden, far away and tiny: it is placed
/// and shown once it exists and its size is known.
pub fn builder() -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title(TITLE)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_visible(false)
        .with_position([-30000.0, -30000.0])
        .with_inner_size([8.0, 8.0])
}

/// A fullscreen app may have come to the front: close the panel if it covers the panel's monitor.
/// Called by the fullscreen check, which runs on the UI thread without access to the bars' state.
pub fn step_aside() {
    let panel = PANEL.load(Ordering::Acquire);
    if panel != 0 && foreground_covers_monitor_of(panel) {
        COVERED.store(true, Ordering::Release);
        host::request_frame();
    }
}

pub struct Preview {
    ctx: egui::Context,
    machine: Machine,
    /// The panel's window, once the host has declared it and it exists.
    hwnd: Option<isize>,
    /// The windows of the hovered app, front-most first (at most `MAX_WINDOWS`).
    windows: Vec<AppWindow>,
    layout: Layout,
    /// A live picture for each window that has one, registered with the panel's window.
    thumbs: Vec<LiveThumb>,
    /// The windows or the anchor changed since the panel was last placed.
    dirty: bool,
    /// Frames drawn since the panel was shown, while it is still cloaked.
    warm: Option<u8>,
}

impl Preview {
    pub fn new(ctx: egui::Context) -> Self {
        Self {
            ctx,
            machine: Machine::new(),
            hwnd: None,
            windows: Vec::new(),
            layout: Layout::default(),
            thumbs: Vec::new(),
            dirty: false,
            warm: None,
        }
    }

    /// The app whose windows the panel shows, while it is open.
    pub fn app_id(&self) -> Option<String> {
        self.machine.key().map(|k| k.app_id.clone())
    }

    /// What the pointer did on `device`'s dock window `dock` (drawn at `ppp`) this frame.
    pub fn hover_dock(&mut self, device: &str, hover: DockHover, dock: isize, ppp: f32) {
        let now = Instant::now();
        let (was_open, epoch) = (self.machine.is_open(), self.machine.epoch());
        match hover.target {
            Some(target) => {
                let key = Key { device: device.to_owned(), app_id: target.app_id };
                if hover.dismiss {
                    self.machine.dismiss(Some(key));
                } else {
                    let (center, top) = (target.center_x, target.top);
                    self.machine.hover(now, key, target.has_windows, || window::anchor(dock, center, top, ppp));
                }
            }
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
            self.ctx.request_repaint_of(id());
        }
        self.schedule(now);
    }

    fn schedule(&self, now: Instant) {
        if let Some(due) = self.machine.deadline(now) {
            self.ctx.request_repaint_after_for(due, ViewportId::ROOT);
        }
    }

    /// In the host's frame: let the timers run out. True while the panel should exist.
    pub fn tick(&mut self, now: Instant) -> bool {
        if COVERED.swap(false, Ordering::AcqRel) {
            self.machine.dismiss(None);
        }
        let was_open = self.machine.is_open();
        self.machine.tick(now);
        let open = self.machine.is_open();
        if open && !was_open {
            self.dirty = true;
        }
        if !open {
            self.release();
        }
        self.schedule(now);
        open
    }

    /// Let go of the window and the pictures; the host is about to stop declaring the viewport.
    fn release(&mut self) {
        if self.hwnd.is_none() && self.thumbs.is_empty() {
            return;
        }
        // Pictures first: they are registered with the window that is about to go.
        self.thumbs.clear();
        if let Some(hwnd) = self.hwnd.take() {
            window::hide(hwnd);
        }
        PANEL.store(0, Ordering::Release);
        self.windows.clear();
        self.layout = Layout::default();
        self.warm = None;
    }

    /// In the host's frame, after it declared the panel: find the window once it exists and give
    /// it its looks. False while it is still to come.
    pub fn adopt(&mut self, glass: bool) -> bool {
        if !self.machine.is_open() || self.hwnd.is_some() {
            return true;
        }
        let Some(hwnd) = shape::own_window(TITLE) else { return false };
        // Cloaked from the first moment: it is shown only once drawn.
        window::cloak(hwnd, true);
        if glass {
            frost_window(HWND(hwnd as *mut _));
        }
        shape::never_activate(hwnd);
        self.hwnd = Some(hwnd);
        PANEL.store(hwnd, Ordering::Release);
        self.warm = Some(0);
        self.dirty = true;
        self.ctx.request_repaint_of(id());
        true
    }

    /// Take the app's windows as they are now and lay the panel out for them. The host calls this
    /// when the window is first found: it is created hidden and gets no frames until it is shown,
    /// so it cannot lay itself out.
    pub fn update(&mut self, mut windows: Vec<AppWindow>) {
        windows.truncate(MAX_WINDOWS);
        if windows.is_empty() {
            // The app's last window is gone.
            let (was_open, epoch) = (self.machine.is_open(), self.machine.epoch());
            self.machine.dismiss(None);
            self.changed(was_open, epoch, Instant::now());
            return;
        }
        let same = self.windows.len() == windows.len()
            && self.windows.iter().zip(&windows).all(|(a, b)| a.hwnd == b.hwnd && a.minimized == b.minimized);
        self.dirty |= !same;
        self.windows = windows;
        self.settle();
    }

    /// In the panel's frame: take the app's windows as they are now, and draw. `icons` loads app
    /// icons. Returns what was clicked.
    pub fn frame(
        &mut self,
        ctx: &egui::Context,
        windows: Vec<AppWindow>,
        glass: bool,
        icons: draw::Icons,
    ) -> Option<Action> {
        let now = Instant::now();
        let (was_open, epoch) = (self.machine.is_open(), self.machine.epoch());
        self.machine.panel_pointer(now, ctx.pointer_hover_pos().is_some());
        self.changed(was_open, epoch, now);
        self.update(windows);
        let (Some(anchor), Some(_)) = (self.machine.anchor(), self.hwnd) else { return None };
        if self.layout.tiles.len() != self.windows.len() {
            return None;
        }
        let live: Vec<bool> = self.windows.iter().map(|w| self.thumb_of(w).is_some()).collect();
        let k = anchor.scale / ctx.pixels_per_point();
        let view = draw::View { windows: &self.windows, layout: &self.layout, live: &live, glass, k };
        let action = draw::draw(ctx, &view, icons);
        self.warm_up(ctx, (k - 1.0).abs() < 0.001);
        action
    }

    /// Uncloak once the panel has been drawn, at the right scale, a couple of times.
    fn warm_up(&mut self, ctx: &egui::Context, scaled: bool) {
        let (Some(n), Some(hwnd)) = (self.warm, self.hwnd) else { return };
        if n + 1 >= WARM_FRAMES && (scaled || n >= WARM_CAP) {
            window::cloak(hwnd, false);
            self.warm = None;
        } else {
            self.warm = Some(n + 1);
            ctx.request_repaint();
        }
    }

    fn thumb_of(&self, window: &AppWindow) -> Option<&LiveThumb> {
        self.thumbs.iter().find(|t| t.source() == window.hwnd && !window.minimized)
    }

    /// Lay the panel out for its windows and put the window and the pictures in place.
    fn settle(&mut self) {
        if !self.dirty {
            return;
        }
        let (Some(hwnd), Some(anchor)) = (self.hwnd, self.machine.anchor()) else { return };
        if self.windows.is_empty() {
            return;
        }
        self.dirty = false;
        // Pictures: keep those of windows still shown, add the new. A minimized window has none.
        let windows = &self.windows;
        self.thumbs.retain(|t| windows.iter().any(|w| w.hwnd == t.source() && !w.minimized));
        for w in windows.iter().filter(|w| !w.minimized) {
            if !self.thumbs.iter().any(|t| t.source() == w.hwnd) {
                self.thumbs.extend(LiveThumb::register(hwnd, w.hwnd));
            }
        }
        let sizes: Vec<_> = self
            .windows
            .iter()
            .map(|w| layout::preview_size(self.thumb_of(w).and_then(|t| t.source_size())))
            .collect();
        let monitor = anchor.monitor;
        self.layout = layout::arrange(&sizes, layout::max_width(monitor.right - monitor.left, anchor.scale));
        let edge = (EDGE * anchor.scale).round() as i32;
        let rect = layout::panel_rect(
            anchor.center_x,
            anchor.bottom,
            layout::size_px(&self.layout, anchor.scale),
            monitor,
            edge,
        );
        place::put(hwnd, rect);
        // Each picture over its tile, in the panel's own pixels.
        let s = anchor.scale;
        for (w, tile) in self.windows.iter().zip(&self.layout.tiles) {
            let Some(thumb) = self.thumb_of(w) else { continue };
            let area = PxRect {
                x: tile.preview.min.x * s,
                y: tile.preview.min.y * s,
                w: tile.preview.width() * s,
                h: tile.preview.height() * s,
            };
            match thumb.source_size() {
                Some(size) => thumb.show(fit(size, area)),
                None => thumb.hide(),
            }
        }
        window::show(hwnd);
        self.ctx.request_repaint_of(id());
    }
}
