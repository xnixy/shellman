//! The notification area: apps' tray icons, shown in the top bar.
//!
//! The tray only works while shellman owns the `Shell_TrayWnd` window, which means Explorer's
//! taskbar is not running; otherwise `start` returns None and the bar runs without a tray.

mod click;
mod data;
mod host;
mod icon;
pub mod view;

pub use click::Click;

use std::sync::{Arc, Mutex};
use windows::core::w;
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

/// One app's tray icon.
#[derive(Default, Clone)]
pub struct TrayIcon {
    pub hwnd: isize,
    pub uid: u32,
    pub guid: Option<[u8; 16]>,
    pub pid: u32,
    pub callback: u32,
    /// 4 when the app asked for the newer click messages (`NIM_SETVERSION`).
    pub version: u32,
    pub tip: String,
    pub hidden: bool,
    pub image: Option<Arc<egui::ColorImage>>,
    /// Bumped whenever the image changes, so the bar re-uploads only changed icons.
    pub rev: u64,
}

#[derive(Default)]
pub struct TrayState {
    pub icons: Vec<TrayIcon>,
}

pub struct Tray {
    state: Arc<Mutex<TrayState>>,
}

impl Tray {
    /// Become the tray, unless another one (Explorer's) is running.
    pub fn start(ctx: egui::Context) -> Option<Self> {
        if unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }.is_ok_and(|h| !h.is_invalid()) {
            return None;
        }
        let state = Arc::new(Mutex::new(TrayState::default()));
        let shared = state.clone();
        std::thread::spawn(move || host::run(shared, Box::new(move || ctx.request_repaint())));
        Some(Self { state })
    }

    /// The visible icons, in the order apps added them.
    pub fn icons(&self) -> Vec<TrayIcon> {
        let mut state = self.state.lock().unwrap();
        // Drop icons whose app has gone without removing them.
        state.icons.retain(|i| click::alive(i.hwnd));
        state.icons.iter().filter(|i| !i.hidden).cloned().collect()
    }
}
