//! One widget's window on one monitor: an egui viewport of its own (deferred: drawn when it asks,
//! not with every frame of the shelves), with the shelf windows' manners (desktop level, never
//! activated, not in Alt+Tab, steps aside for a fullscreen app). Each widget has its own window,
//! so each can be put anywhere on its monitor (arrange mode) without a window growing towards
//! the monitor's size. Made once for its monitor, its glass attached once when first found,
//! moved only when where it should be changes; dropped (glass first) when its monitor goes.

use super::place;
use crate::arrange::place::WidgetPlacement;
use crate::arrange::Item;
use crate::geometry::SHADOW;
use crate::{log, window};
use shellman_core::monitors::Monitor;
use shellman_ui::glass::{GlassPanel, Style};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};

pub struct WidgetView {
    pub id: egui::ViewportId,
    pub device: String,
    /// Today or Now playing.
    pub kind: Item,
    title: String,
    pub hwnd: Option<isize>,
    pub panel: Option<GlassPanel>,
    pub monitor: Monitor,
    pub placed: Option<WidgetPlacement>,
    shown: bool,
}

/// The tallest a widget of `kind` grows.
pub fn max_height(kind: Item) -> f32 {
    match kind {
        Item::Playing => place::playing_height(),
        _ => place::today_max(),
    }
}

impl WidgetView {
    pub fn new(monitor: Monitor, kind: Item) -> Self {
        let device = monitor.device.clone();
        let name = if kind == Item::Playing { "playing" } else { "today" };
        Self {
            id: egui::ViewportId::from_hash_of(("shellman widgets", &device, name)),
            // Never the shelf's own title, which is how the desktop finds the shelf.
            title: format!("{} {name} {device}", shellman_core::desktop_menu::shelf_title()),
            device,
            kind,
            hwnd: None,
            panel: None,
            monitor,
            placed: None,
            shown: false,
        }
    }

    /// Hidden until placed; sized for its widget, never the monitor.
    pub fn builder(&self) -> egui::ViewportBuilder {
        let mut b = egui::ViewportBuilder::default()
            .with_title(self.title.clone())
            .with_inner_size([place::WIDTH + 2.0 * SHADOW, max_height(self.kind) + 2.0 * SHADOW])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_taskbar(false)
            .with_visible(false);
        // Made on its own monitor, so it never appears elsewhere first.
        let r = self.monitor.work;
        if self.monitor.scale.is_finite() && self.monitor.scale > 0.0 {
            b = b.with_position([r.left as f32 / self.monitor.scale, r.top as f32 / self.monitor.scale]);
        }
        b
    }

    /// Find the window once made; give it its manners and its glass, once.
    pub fn adopt(&mut self) -> bool {
        if self.hwnd.is_some() {
            return true;
        }
        let Some(hwnd) = crate::own_window(&self.title) else { return false };
        window::prepare_other(hwnd);
        let panel = GlassPanel::attach(hwnd, Style::from_theme(place::RADIUS));
        panel.set_islands_only(&[], 1.0, None);
        self.panel = Some(panel);
        self.hwnd = Some(hwnd);
        log::line(format!("window for {}", self.title));
        true
    }

    /// Put the window where its widget is to be (`want`); shown the first time.
    pub fn place(&mut self, want: Option<WidgetPlacement>) {
        let Some(hwnd) = self.hwnd else { return };
        let Some(p) = want else {
            self.placed = None;
            return;
        };
        if self.placed == Some(p) {
            return;
        }
        window::place(hwnd, p.rect);
        self.placed = Some(p);
        if !self.shown {
            self.shown = true;
            unsafe {
                let _ = ShowWindow(HWND(hwnd as *mut _), SW_SHOWNOACTIVATE);
            }
        }
    }
}

impl Drop for WidgetView {
    fn drop(&mut self) {
        // The glass first, then the window's manners; the window closes when no longer declared.
        self.panel = None;
        if let Some(hwnd) = self.hwnd {
            window::forget(hwnd);
        }
        log::line(format!("closed {}", self.title));
    }
}
