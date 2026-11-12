//! The notification area: apps' tray icons, shown in the top bar.
//!
//! The tray only works while shellman owns the `Shell_TrayWnd` window, which means Explorer's
//! taskbar is not running; otherwise `start` returns None and the bar runs without a tray.

mod click;
mod data;
pub mod demo;
mod host;
mod icon;
pub(crate) mod log;
pub mod view;

pub use click::Click;
pub use host::{forget_device, set_rect, touch};

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
    /// Where each monitor's top bar last drew it. Answers `Shell_NotifyIconGetRect`, which many
    /// apps (Tauri, `tray-icon`) need before reacting.
    pub rects: Vec<IconRect>,
    /// The monitor whose copy of this icon was last hovered or clicked.
    pub last_device: Option<String>,
}

/// An icon's place on one monitor's top bar: left, top, right, bottom in physical screen pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct IconRect {
    pub device: String,
    pub primary: bool,
    pub rect: [i32; 4],
}

impl TrayIcon {
    /// Remember where `device`'s top bar drew this icon. True when that changed anything.
    pub fn record(&mut self, device: &str, primary: bool, rect: [i32; 4]) -> bool {
        match self.rects.iter_mut().find(|r| r.device == device) {
            Some(r) if r.rect == rect && r.primary == primary => false,
            Some(r) => {
                r.rect = rect;
                r.primary = primary;
                true
            }
            None => {
                self.rects.push(IconRect { device: device.into(), primary, rect });
                true
            }
        }
    }

    /// Note that the user is working with the copy on `device`.
    pub fn touch(&mut self, device: &str) {
        if self.last_device.as_deref() != Some(device) && self.rects.iter().any(|r| r.device == device) {
            self.last_device = Some(device.into());
        }
    }

    /// Forget `device`'s copy (its bar stopped drawing the tray, or went away).
    pub fn forget(&mut self, device: &str) {
        self.rects.retain(|r| r.device != device);
        if self.last_device.as_deref() == Some(device) {
            self.last_device = None;
        }
    }

    /// What `Shell_NotifyIconGetRect` answers: the copy on the monitor last touched, else the
    /// primary's, else any.
    pub fn answer_rect(&self) -> Option<[i32; 4]> {
        let last = self.last_device.as_deref().and_then(|d| self.rects.iter().find(|r| r.device == d));
        last.or_else(|| self.rects.iter().find(|r| r.primary)).or(self.rects.first()).map(|r| r.rect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_with_the_monitor_last_touched() {
        let mut i = TrayIcon::default();
        assert_eq!(i.answer_rect(), None);
        assert!(i.record("A", true, [3000, 4, 3026, 30]));
        assert!(i.record("B", false, [-800, 4, -774, 30]));
        assert!(!i.record("B", false, [-800, 4, -774, 30]), "same place, no change");
        assert_eq!(i.answer_rect(), Some([3000, 4, 3026, 30]), "primary before any interaction");
        i.touch("B");
        assert_eq!(i.answer_rect(), Some([-800, 4, -774, 30]));
        i.touch("nowhere");
        assert_eq!(i.last_device.as_deref(), Some("B"), "an unknown monitor is ignored");
        assert!(i.record("B", false, [-700, 4, -674, 30]));
        assert_eq!(i.answer_rect(), Some([-700, 4, -674, 30]), "follows a moved icon");
    }

    #[test]
    fn forgetting_a_monitor_falls_back_to_the_primary() {
        let mut i = TrayIcon::default();
        i.record("A", true, [1, 2, 3, 4]);
        i.record("B", false, [5, 6, 7, 8]);
        i.touch("B");
        i.forget("B");
        assert_eq!((i.answer_rect(), i.last_device.clone()), (Some([1, 2, 3, 4]), None));
        i.forget("A");
        assert_eq!(i.answer_rect(), None);
        // No primary known: any copy beats none.
        i.record("C", false, [9, 9, 9, 9]);
        assert_eq!(i.answer_rect(), Some([9, 9, 9, 9]));
    }
}

#[derive(Default)]
pub struct TrayState {
    pub icons: Vec<TrayIcon>,
}

pub struct Tray {
    state: Arc<Mutex<TrayState>>,
}

impl Tray {
    /// Become the tray, unless another one (Explorer's) is running. `wake` is called (from the
    /// tray's thread) when icons change.
    pub fn start(wake: impl Fn() + Send + Sync + 'static) -> Option<Self> {
        if unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }.is_ok_and(|h| !h.is_invalid()) {
            return None;
        }
        let state = Arc::new(Mutex::new(TrayState::default()));
        let shared = state.clone();
        std::thread::spawn(move || host::run(shared, Box::new(wake)));
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
