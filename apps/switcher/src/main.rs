#![windows_subsystem = "windows"]
//! Shellman's window switcher: hold Alt and press Tab.
//!
//! A resident process with one window, hidden between uses. The keyboard hook (`keys`) wakes the
//! UI, which runs the switcher (`switcher`) and draws it (`panel`) only while it is open.

mod activate;
mod keys;
mod layout;
mod panel;
mod select;
mod shape;
mod switcher;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_ui::appearance::{self, Appearance};
use shellman_ui::{theme, window};
use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;

fn main() -> eframe::Result {
    // One switcher per session: a second would only fight the first for the keys.
    let _single = unsafe { CreateMutexW(None, false, w!("Local\\shellman-switcher")) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("shellman switcher")
            .with_inner_size([320.0, 200.0])
            // The window library shows the window once after its first frame; off screen, that
            // one frame is never seen, and the switcher hides it at once.
            .with_position([-32000.0, -32000.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_always_on_top()
            .with_taskbar(false),
        ..Default::default()
    };
    eframe::run_native(
        "shellman-switcher",
        options,
        Box::new(|cc| {
            theme::install_fonts(&cc.egui_ctx);
            theme::set_light(cc.egui_ctx.theme() == egui::Theme::Light);
            theme::apply(&cc.egui_ctx);
            let look = Appearance::load_or(Appearance::default());
            look.apply(&cc.egui_ctx);
            let look_watch = shellman_core::watch::Watcher::new(cc.egui_ctx.clone());
            if let Some(dir) = appearance::file().and_then(|f| f.parent().map(|p| p.to_path_buf())) {
                look_watch.set_dirs(vec![dir]);
            }
            let mut hwnd = 0;
            let mut glass = false;
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    hwnd = win32.hwnd.get();
                    glass = window::frost_window(windows::Win32::Foundation::HWND(hwnd as *mut _));
                    shape::never_activate(hwnd);
                }
            }
            let (tx, rx) = std::sync::mpsc::channel();
            let wake = cc.egui_ctx.clone();
            let hooked = keys::start(move |key| {
                let _ = tx.send(key);
                wake.request_repaint();
            });
            if !hooked {
                return Err("Windows refused the keyboard hook".into());
            }
            Ok(Box::new(App {
                switcher: switcher::Switcher::new(cc.egui_ctx.clone(), hwnd, glass, rx),
                look,
                look_watch,
            }))
        }),
    )
}

struct App {
    switcher: switcher::Switcher,
    look: Appearance,
    look_watch: shellman_core::watch::Watcher,
}

impl eframe::App for App {
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
        self.switcher.frame(ctx);
    }
}
