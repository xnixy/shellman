#![windows_subsystem = "windows"]
//! Task View: every open window at once, as live previews over the whole monitor (Win+Tab).
//!
//! Run once, it stays resident with its window out of sight; run again, or signalled by the
//! switcher's keyboard hook on Win+Tab, it shows (or hides). `--background` starts the resident
//! process without showing it. Unlike the switcher, it takes the foreground while showing, so its
//! keys come to it directly and screen readers follow its focus.

mod app;
mod layout;
mod select;
mod summon;
mod view;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_ui::appearance::{self, Appearance};
use shellman_ui::{theme, window};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Whether the window has Windows' acrylic behind it; without it the window paints a solid page.
/// The overlay is one surface edge to edge, so the window's own backdrop does what the floating
/// glass under the other apps' islands does, as for the switcher, with no second window.
pub static GLASS: AtomicBool = AtomicBool::new(false);

fn main() -> eframe::Result {
    let background = std::env::args().any(|a| a == "--background");
    let instance = match summon::claim(background) {
        summon::Claim::Signalled => return Ok(()),
        summon::Claim::Resident(instance) => instance,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("shellman task view")
            .with_inner_size([800.0, 600.0])
            .with_visible(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_always_on_top()
            .with_taskbar(false),
        ..Default::default()
    };
    eframe::run_native(
        "shellman-taskview",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            theme::set_light(cc.egui_ctx.theme() == egui::Theme::Light);
            theme::apply(&cc.egui_ctx);
            let look = Appearance::load_or(Appearance::default());
            look.apply(&cc.egui_ctx);
            let mut hwnd = 0isize;
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    hwnd = win32.hwnd.get();
                }
            }
            let frosted = hwnd != 0 && window::frost_window(windows::Win32::Foundation::HWND(hwnd as *mut _));
            if hwnd != 0 {
                summon::prepare(hwnd);
            }
            let shared = Arc::new(summon::Shared::default());
            instance.serve(hwnd, cc.egui_ctx.clone(), shared.clone());
            if !background {
                instance.summon();
            }
            let look_watch = shellman_core::watch::Watcher::new(cc.egui_ctx.clone());
            if let Some(dir) = appearance::file().and_then(|f| f.parent().map(|p| p.to_path_buf())) {
                look_watch.set_dirs(vec![dir]);
            }
            Ok(Box::new(Shell {
                inner: app::TaskView::new(cc.egui_ctx.clone(), hwnd, shared),
                hwnd,
                frosted,
                light: None,
                look,
                look_watch,
                _instance: instance,
            }))
        }),
    )
}

struct Shell {
    inner: app::TaskView,
    hwnd: isize,
    frosted: bool,
    /// The mode the window's frame was last painted for.
    light: Option<bool>,
    look: Appearance,
    look_watch: shellman_core::watch::Watcher,
    _instance: summon::Instance,
}

impl eframe::App for Shell {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.look_watch.poll().is_empty() {
            let look = Appearance::load_or(self.look);
            if look != self.look {
                self.look = look;
                look.apply(ctx);
            }
        }
        // SHELLMAN_THEME / SHELLMAN_FORCE_LIGHT, else Windows.
        let want = theme::env::preference();
        if ctx.options(|o| o.theme_preference) != want {
            ctx.set_theme(want);
        }
        let light = want == egui::ThemePreference::Light || ctx.theme() == egui::Theme::Light;
        theme::set_light(light);
        if self.light != Some(light) {
            // The acrylic takes its light or dark base from the frame's mode.
            self.light = Some(light);
            window::paint_frame(self.hwnd);
            summon::edge_to_edge(self.hwnd);
        }
        let glass = self.frosted && !theme::env::no_glass() && !theme::colors().high_contrast;
        GLASS.store(glass, std::sync::atomic::Ordering::Relaxed);
        self.inner.frame(ctx);
    }
}
