//! The bar process's state and its windows' frames.
//!
//! One window list, one icon cache, one set of pins (`Shared`), drawn by many windows: a top bar
//! and a dock per monitor. Each bar window is a deferred egui viewport, so it repaints on its own
//! (a hover on one dock repaints that dock only); its frame locks the shared state and draws its
//! part. Viewports are declared by the host's frames, which run only when something changes
//! (monitors, settings, the look, a woken tray or window watcher), so the host costs nothing at
//! rest. Every frame runs on the UI thread, one at a time, so the lock never waits.

use crate::bars::{self, Bars, Kind};
use crate::monitors::{self, layout};
use crate::shape::surface;
use crate::wake::Wake;
use crate::workarea::HiddenMinimized;
use crate::tray::view::Place;
use crate::{dock, host, notify, preview, stress, topbar};
use shellman_core::settings::bar::{BarSettings, TrayMonitors};
use shellman_core::winlist::fullscreen::foreground_covers_monitor_of;
use shellman_ui::appearance::Appearance;
use shellman_ui::{font_atlas, theme};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Whether the bars draw in light mode: Windows' app mode, or `SHELLMAN_FORCE_LIGHT` to look at
/// the light palette without changing Windows' setting.
pub fn light(ctx: &egui::Context) -> bool {
    std::env::var_os("SHELLMAN_FORCE_LIGHT").is_some() || ctx.theme() == egui::Theme::Light
}

pub struct Shared {
    pub dock: dock::Dock,
    pub top: topbar::TopBar,
    pub bars: Bars,
    pub preview: preview::Preview,
    pub notify: notify::Notify,
}

impl Shared {
    fn top_frame(&mut self, ctx: &egui::Context, device: &str, primary: bool) {
        theme::set_light(light(ctx));
        self.dock.catch_up(ctx);
        let front = self.dock.front_window().cloned();
        let icon = front.as_ref().and_then(|_| self.dock.front_icon(ctx, 16.0 * ctx.pixels_per_point()));
        let hwnd = self.bars.top_hwnd(device).unwrap_or(0);
        surface::set_style(hwnd, bars::top_style());
        let origin = (hwnd != 0).then(|| bars::client_origin(hwnd)).flatten();
        let front_hwnd = front.as_ref().map(|w| w.hwnd);
        // The notifications list closes when another app comes forward.
        self.notify.front_is(front_hwnd);
        self.top.badge = self.notify.badge();
        let front = front.as_ref().map(|window| topbar::Front { window, icon });
        let place = Place { device, primary, origin };
        let islands = self.top.frame(ctx, front, surface::composed(hwnd), &place);
        // Notifications hang under the primary bar's clock island (the second of three).
        if let (true, Some((x, y)), Some(clock)) = (primary, origin, islands.get(1)) {
            let ppp = ctx.pixels_per_point();
            let centre = x + (clock.rect.center().x * ppp).round() as i32;
            self.notify.set_anchor(centre, y + (clock.rect.bottom() * ppp).round() as i32);
        }
        if self.top.take_badge_click() {
            self.notify.toggle_list(front_hwnd);
        }
        // The whole window stays the bar's: a click between the islands lands on it and does
        // nothing (the space is reserved anyway).
        surface::set_islands(hwnd, &islands, ctx.pixels_per_point(), false, None);
    }

    /// Lay the open preview panel out for its app's current windows.
    fn update_preview(&mut self) {
        if let Some(app) = self.preview.app_id() {
            self.preview.update(self.dock.windows_of(&app));
        }
    }

    /// A frame of the preview panel: the hovered app's windows, drawn, and what was clicked.
    fn preview_frame(&mut self, ctx: &egui::Context) {
        theme::set_light(light(ctx));
        self.dock.catch_up(ctx);
        let Some(app) = self.preview.app_id() else { return };
        let windows = self.dock.windows_of(&app);
        let dock = &mut self.dock;
        match self.preview.frame(ctx, windows, &mut |ctx, path| dock.icon(ctx, path)) {
            Some(preview::Action::Show(hwnd)) => {
                dock::Dock::show_window(hwnd);
                self.preview.dismiss();
            }
            Some(preview::Action::Close(hwnd)) => dock::close_window(hwnd),
            None => {}
        }
    }

    /// A frame of the notifications window.
    fn notify_frame(&mut self, ctx: &egui::Context) {
        theme::set_light(light(ctx));
        self.dock.catch_up(ctx);
        let dock = &mut self.dock;
        let windows: Vec<(String, isize)> = dock.front_windows();
        let front_of = |app: &str| windows.iter().find(|(a, _)| a == app).map(|(_, h)| *h);
        self.notify.frame(ctx, &mut |ctx, path| dock.icon(ctx, path), &front_of);
    }

    fn dock_frame(&mut self, ctx: &egui::Context, device: &str) {
        let Some(hwnd) = self.bars.dock_hwnd(device) else { return };
        let hover = self.dock.frame(ctx, hwnd, device);
        self.preview.hover_dock(device, hover, hwnd, ctx.pixels_per_point());
    }
}

/// The host window's app: it keeps the bar windows matched to the monitors and settings.
pub struct Host {
    shared: Arc<Mutex<Shared>>,
    wake: Wake,
    look: Appearance,
    look_watch: shellman_core::watch::Watcher,
    settings: BarSettings,
    _minimized: Option<HiddenMinimized>,
}

impl Host {
    pub fn new(
        shared: Shared,
        wake: Wake,
        look: Appearance,
        look_watch: shellman_core::watch::Watcher,
        minimized: Option<HiddenMinimized>,
    ) -> Self {
        Self {
            shared: Arc::new(Mutex::new(shared)),
            wake,
            look,
            look_watch,
            settings: BarSettings::load(),
            _minimized: minimized,
        }
    }

    /// Open (or keep open) every bar window. Windows not declared in a host frame are closed.
    fn declare(&self, ctx: &egui::Context, bars: &Bars, preview_open: bool, notify_open: bool) {
        for (view, device, primary) in bars.views() {
            let builder = egui::ViewportBuilder::default()
                .with_title(view.title.clone())
                .with_decorations(false)
                .with_transparent(true)
                .with_resizable(false)
                .with_always_on_top()
                .with_taskbar(false)
                // Shown by `Bars` once placed on its monitor.
                .with_visible(false);
            let shared = self.shared.clone();
            let device = device.to_owned();
            let kind = view.kind;
            ctx.show_viewport_deferred(view.id, builder, move |ctx, _| {
                let mut shared = shared.lock().unwrap();
                match kind {
                    Kind::Top => shared.top_frame(ctx, &device, primary),
                    Kind::Dock => shared.dock_frame(ctx, &device),
                }
                stress::glyphs(ctx);
                font_atlas::end_frame(ctx);
            });
        }
        // The preview panel exists only while open (see `preview`).
        if preview_open {
            let shared = self.shared.clone();
            ctx.show_viewport_deferred(preview::id(), preview::builder(), move |ctx, _| {
                shared.lock().unwrap().preview_frame(ctx);
                stress::glyphs(ctx);
                font_atlas::end_frame(ctx);
            });
        }
        // So does the notifications window (see `notify`).
        if notify_open {
            let shared = self.shared.clone();
            ctx.show_viewport_deferred(notify::id(), notify::builder(), move |ctx, _| {
                shared.lock().unwrap().notify_frame(ctx);
                font_atlas::end_frame(ctx);
            });
        }
    }
}

impl eframe::App for Host {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut shared = self.shared.lock().unwrap();
        let mut layout_needed = host::take_layout();
        if !self.look_watch.poll().is_empty() {
            // The same folder holds the dock's pins and the bar's settings.
            shared.dock.reload_pins();
            shared.top.reload_settings();
            let look = Appearance::load_or(self.look);
            if look != self.look {
                self.look = look;
                look.apply(ctx);
                // Each bar restyles its glass in its own frame.
                self.wake.bars();
            }
            let settings = BarSettings::load();
            if settings.docks != self.settings.docks {
                layout_needed = true;
            }
            self.settings = settings;
        }
        shared.dock.catch_up(ctx);
        // The dock's height follows its icon size, and its reserved band with it.
        let sizes = bars::sizes(shared.dock.height());
        let reassert = host::take_reassert();
        if layout_needed || reassert || sizes != shared.bars.sizes() {
            let slots = layout::plan(&monitors::all(), self.settings.docks, &sizes);
            shared.bars.update(slots, sizes, reassert);
        }
        if host::take_requery() {
            shared.bars.requery();
        }
        let preview_open = shared.preview.tick(Instant::now());
        let covered = shared.bars.primary_top().is_some_and(foreground_covers_monitor_of);
        let notify_open = shared.notify.tick(Instant::now(), self.settings.do_not_disturb, covered);        // The preview panel is woken with the bars, for when its app's windows change, but only
        // while it is open: a repaint asked for a viewport nobody declares makes egui open a
        // blank default window for it ("egui window"), which then spins the event loop.
        let mut ids = shared.bars.ids();
        if preview_open {
            ids.push(preview::id());
        }
        if notify_open {
            ids.push(notify::id());
        }
        self.wake.set(ids);
        let tray_tops = shared.bars.views().filter(|(v, _, primary)| {
            v.kind == Kind::Top && (*primary || self.settings.tray == TrayMonitors::All)
        });
        self.wake.set_tray(tray_tops.map(|(v, _, _)| v.id).collect());
        self.declare(ctx, &shared.bars, preview_open, notify_open);
        // New windows exist only after this frame; look again on the next one.
        let dock_style = shared.dock.style();
        let bars_ready = shared.bars.adopt(dock_style);
        let preview_ready = shared.preview.adopt();
        let notify_ready = shared.notify.adopt();
        shared.update_preview();
        if !(bars_ready && preview_ready && notify_ready) {
            ctx.request_repaint();
        }
        shared.bars.settle();
        drop(shared);
        // The host wakes for changes every bar shows (the look, icons, the tray).
        self.wake.bars();
        font_atlas::end_frame(ctx);
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.bars.release();
        }
    }
}
