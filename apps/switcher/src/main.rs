#![windows_subsystem = "windows"]
//! Shellman's window switcher: hold Alt and press Tab. Its keyboard hook also turns a lone Win
//! key into the launcher (`winkey`), and Ctrl+Shift+Esc into shellman's Task Manager (`taskkey`).
//!
//! A resident process with one window, hidden between uses. The keyboard hook (`keys`) wakes the
//! UI, which runs the switcher (`switcher`) and draws it (`panel`) only while it is open.

mod activate;
mod dockkey;
mod keys;
mod notifykey;
mod layout;
mod panel;
mod select;
mod shape;
mod switcher;
mod taskkey;
mod taskviewkey;
mod winkey;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_ui::appearance::{self, Appearance};
use shellman_ui::{theme, window};
use windows::core::HSTRING;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;

fn main() -> eframe::Result {
    // One switcher per session: a second would only fight the first for the keys.
    // SHELLMAN_INSTANCE names a test copy, which then has its own mutex (and see `demo`).
    let instance = std::env::var("SHELLMAN_INSTANCE").ok().filter(|s| !s.is_empty());
    let suffix = instance.as_deref().map(|i| format!("-{i}")).unwrap_or_default();
    let mutex = HSTRING::from(format!("Local\\shellman-switcher{suffix}"));
    let _single = unsafe { CreateMutexW(None, false, &mutex) };
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
            let sink = std::sync::Arc::new(move |key| {
                let _ = tx.send(key);
                // A hidden window is never painted, so asking for a frame is not enough to open.
                // Other keys only matter once it is open, and so awake.
                if matches!(key, keys::Key::Next | keys::Key::Prev) {
                    shape::post_wake(hwnd);
                }
                wake.request_repaint();
            });
            // A test copy (SHELLMAN_INSTANCE) installs no keyboard hook, which would fight the
            // real switcher's for Alt+Tab; it opens and closes itself on a timer instead.
            let hooked = if instance.is_some() {
                switcher::DEMO.store(true, std::sync::atomic::Ordering::Relaxed);
                demo(sink);
                true
            } else {
                keys::start(move |key| sink(key))
            };
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

/// Stand-in for the keyboard in a test copy: twice, a few seconds apart, as if Alt+Tab were
/// pressed twice and then Esc; after that the process is left idle to be measured.
fn demo(sink: std::sync::Arc<impl Fn(keys::Key) + Send + Sync + 'static>) {
    std::thread::spawn(move || {
        for _ in 0..2 {
            std::thread::sleep(std::time::Duration::from_secs(4));
            sink(keys::Key::Next);
            std::thread::sleep(std::time::Duration::from_millis(300));
            sink(keys::Key::Next);
            std::thread::sleep(std::time::Duration::from_millis(600));
            sink(keys::Key::Cancel);
        }
    });
}
