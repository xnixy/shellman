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
use crate::wake::Wake;
use crate::workarea::HiddenMinimized;
use crate::{dock, host, preview, stress, topbar};
use shellman_core::settings::bar::BarSettings;
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
    pub glass: bool,
}

impl Shared {
    fn top_frame(&mut self, ctx: &egui::Context, primary: bool) {
        theme::set_light(light(ctx));
        self.dock.catch_up(ctx);
        let front = self.dock.front_window().cloned();
        self.top.frame(ctx, front.as_ref(), self.glass, primary);
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
        match self.preview.frame(ctx, windows, self.glass, &mut |ctx, path| dock.icon(ctx, path)) {
            Some(preview::Action::Show(hwnd)) => {
                dock::Dock::show_window(hwnd);
                self.preview.dismiss();
            }
            Some(preview::Action::Close(hwnd)) => dock::close_window(hwnd),
            None => {}
        }
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
    fn declare(&self, ctx: &egui::Context, bars: &Bars, preview_open: bool) {
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
                    Kind::Top => shared.top_frame(ctx, primary),
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
        if layout_needed || sizes != shared.bars.sizes() {
            let slots = layout::plan(&monitors::all(), self.settings.docks, &sizes);
            shared.bars.update(slots, sizes);
        }
        if host::take_requery() {
            shared.bars.requery();
        }
        let preview_open = shared.preview.tick(Instant::now());
        // The preview panel is woken with the bars, for when its app's windows change, but only
        // while it is open: a repaint asked for a viewport nobody declares makes egui open a
        // blank default window for it ("egui window"), which then spins the event loop.
        let mut ids = shared.bars.ids();
        if preview_open {
            ids.push(preview::id());
        }
        self.wake.set(ids);
        self.declare(ctx, &shared.bars, preview_open);
        let glass = shared.glass;
        // New windows exist only after this frame; look again on the next one.
        let dock_style = shared.dock.style();
        let bars_ready = shared.bars.adopt(glass, dock_style);
        let preview_ready = shared.preview.adopt(glass);
        shared.update_preview();
        if !(bars_ready && preview_ready) {
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
