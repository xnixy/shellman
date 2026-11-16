//! The shelf windows: the process's first window carries the primary monitor's shelf, and with
//! "All displays" every other monitor gets a window of its own, an egui viewport drawn with the
//! first (immediate), so all of them share one set of stacks, thumbnails and settings.
//!
//! Each window is made once for its monitor and kept while that monitor is there: moved,
//! rescaled or re-arranged, it is only placed again. Its glass is attached once, when the
//! window is first found, and its drop target registered then. A monitor that goes away (or a
//! setting that no longer gives it a shelf) has its window's glass, drop target and manners
//! taken off and the window closed (it is no longer declared). See `displays` for the sizes.

use crate::displays::{self, Placement};
use crate::{drop, geometry, log, window};
use shellman_core::monitors::Monitor;
use shellman_core::settings::desktop::{Edge, Pin};
use shellman_ui::glass::{GlassPanel, Style};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};

/// One shelf window.
pub struct View {
    pub id: egui::ViewportId,
    /// The monitor it is for, by Windows' name; None for the first window, which follows the
    /// primary monitor.
    pub device: Option<String>,
    title: String,
    pub hwnd: Option<isize>,
    pub panel: Option<GlassPanel>,
    /// What its frames and its drop target share.
    pub drop: Arc<Mutex<drop::Shared>>,
    /// The ctx its drop target wakes.
    ctx: Option<egui::Context>,
    /// The monitor it shows a shelf on now; None shows none.
    pub monitor: Option<Monitor>,
    /// Where the window was put for that monitor, and for which edge, stacks and shelf place.
    pub placed: Option<Placement>,
    pub placed_for: Option<(Edge, usize, Option<Pin>)>,
    /// Shown after its first placing (the first window is shown at the start).
    shown: bool,
}

impl View {
    /// The first window, made by the window library at the start and given its looks there.
    pub fn first(hwnd: isize, panel: GlassPanel, drop: Arc<Mutex<drop::Shared>>) -> Self {
        Self {
            id: egui::ViewportId::ROOT,
            device: None,
            title: shellman_core::desktop_menu::shelf_title(),
            hwnd: Some(hwnd),
            panel: Some(panel),
            drop,
            ctx: None,
            monitor: None,
            placed: None,
            placed_for: None,
            shown: true,
        }
    }

    /// Another monitor's window, made when first declared.
    pub fn other(device: &str, ctx: &egui::Context) -> Self {
        Self {
            id: egui::ViewportId::from_hash_of(("shellman shelf", device)),
            device: Some(device.to_owned()),
            // Never the first window's title, which is how the desktop finds the shelf.
            title: format!("{} {device}", shellman_core::desktop_menu::shelf_title()),
            hwnd: None,
            panel: None,
            drop: Arc::new(Mutex::new(drop::Shared::default())),
            ctx: Some(ctx.clone()),
            monitor: None,
            placed: None,
            placed_for: None,
            shown: false,
        }
    }

    /// How the window library makes it: like the first window, hidden until placed.
    pub fn builder(&self) -> egui::ViewportBuilder {
        egui::ViewportBuilder::default()
            .with_title(self.title.clone())
            .with_inner_size([geometry::window_width(), 600.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_taskbar(false)
            .with_visible(false)
    }

    /// Find the window once made and give it its manners, its glass and its drop target, once.
    /// False while it does not exist yet.
    pub fn adopt(&mut self) -> bool {
        if self.hwnd.is_some() {
            return true;
        }
        let Some(hwnd) = crate::own_window(&self.title) else { return false };
        window::prepare_other(hwnd);
        let panel = GlassPanel::attach(hwnd, Style::from_theme(geometry::RADIUS));
        // Nothing shows or takes clicks until a frame has laid the shelf out.
        panel.set_islands_only(&[], 1.0, None);
        self.panel = Some(panel);
        if let Some(ctx) = &self.ctx {
            let target = drop::Target { shared: self.drop.clone(), ctx: ctx.clone() };
            if let Err(e) = shellman_core::transfer::target::register(hwnd, Box::new(target)) {
                log::line(format!("no drop target on {}: {e}", self.title));
            }
        }
        self.hwnd = Some(hwnd);
        log::line(format!("window for {}", self.title));
        true
    }

    /// Put the window on its monitor's edge, sized for its shelf at `pin` (None: centred); then
    /// shown, if not yet.
    pub fn place(&mut self, edge: Edge, stacks: usize, pin: Option<Pin>) {
        let Some(hwnd) = self.hwnd else { return };
        self.placed_for = Some((edge, stacks, pin));
        let Some(placement) = self.monitor.as_ref().and_then(|m| displays::place(m, edge, stacks, pin)) else {
            self.placed = None;
            return;
        };
        window::place(hwnd, placement.rect);
        self.placed = Some(placement);
        if !self.shown {
            self.shown = true;
            unsafe {
                let _ = ShowWindow(HWND(hwnd as *mut _), SW_SHOWNOACTIVATE);
            }
        }
    }
}

impl Drop for View {
    fn drop(&mut self) {
        // Only other monitors' windows close while the process runs.
        if self.device.is_none() {
            return;
        }
        // The glass first, then the window's drop target and manners; the window itself closes
        // when the frame no longer declares it.
        self.panel = None;
        if let Some(hwnd) = self.hwnd {
            unsafe {
                let _ = windows::Win32::System::Ole::RevokeDragDrop(HWND(hwnd as *mut _));
            }
            window::forget(hwnd);
        }
        log::line(format!("closed {}", self.title));
    }
}

/// Bring the windows in line with the monitors: the first window takes `primary` (or shows no
/// shelf), every monitor in `others` keeps or gets its own window, and the windows of monitors
/// not there any more close. Every window is placed again.
pub fn sync(views: &mut Vec<View>, primary: Option<Monitor>, others: Vec<Monitor>, ctx: &egui::Context) {
    let have: Vec<String> = views.iter().filter_map(|v| v.device.clone()).collect();
    let want: Vec<String> = others.iter().map(|m| m.device.clone()).collect();
    let plan = displays::reconcile(&have, &want);
    views.retain(|v| v.device.as_ref().is_none_or(|d| !plan.close.contains(d)));
    for device in &plan.open {
        views.push(View::other(device, ctx));
    }
    for v in views.iter_mut() {
        v.monitor = match &v.device {
            None => primary.clone(),
            Some(d) => others.iter().find(|m| &m.device == d).cloned(),
        };
        v.placed = None;
    }
}
