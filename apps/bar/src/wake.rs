//! Repainting bar windows from anywhere.
//!
//! Each bar is its own egui viewport with its own frames; a change several of them show (a window
//! opened, the app in front, a pin) must repaint each of them. `request_repaint` from another
//! thread would only reach whichever window happens to be current, so wake-ups go through here.
//! A frame of a bar window costs several milliseconds, so each change wakes only the windows
//! that show it: the docks, the top bars, or the top bars with the tray.

use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Ids {
    tops: Vec<egui::ViewportId>,
    docks: Vec<egui::ViewportId>,
    /// The top bars that show the tray.
    tray: Vec<egui::ViewportId>,
    /// The preview panel and the notifications window, while they exist.
    others: Vec<egui::ViewportId>,
    preview: Option<egui::ViewportId>,
}

#[derive(Clone)]
pub struct Wake {
    ctx: egui::Context,
    ids: Arc<Mutex<Ids>>,
}

impl Wake {
    pub fn new(ctx: egui::Context) -> Self {
        Self { ctx, ids: Arc::default() }
    }

    /// The bar viewports there are now.
    pub fn set(&self, tops: Vec<egui::ViewportId>, docks: Vec<egui::ViewportId>) {
        let mut ids = self.ids.lock().unwrap();
        ids.tops = tops;
        ids.docks = docks;
    }

    /// The top bar viewports that show the tray.
    pub fn set_tray(&self, ids: Vec<egui::ViewportId>) {
        self.ids.lock().unwrap().tray = ids;
    }

    /// The preview panel (while open), and the notifications and calendar windows (once they exist).
    pub fn set_others(&self, preview: Option<egui::ViewportId>, notify: Option<egui::ViewportId>, calendar: Option<egui::ViewportId>) {
        let mut ids = self.ids.lock().unwrap();
        ids.preview = preview;
        ids.others = preview.into_iter().chain(notify).chain(calendar).collect();
    }

    fn repaint(&self, pick: impl Fn(&Ids) -> Vec<egui::ViewportId>) {
        let ids = pick(&self.ids.lock().unwrap());
        for id in ids {
            self.ctx.request_repaint_of(id);
        }
    }

    /// Repaint the top bars that show the tray, and nothing else. Safe from any thread.
    pub fn tray(&self) {
        self.repaint(|ids| ids.tray.clone());
    }

    /// Repaint the docks. Safe from any thread.
    pub fn docks(&self) {
        self.repaint(|ids| ids.docks.clone());
    }

    /// Repaint the top bars. Safe from any thread.
    pub fn tops(&self) {
        self.repaint(|ids| ids.tops.clone());
    }

    /// Repaint the preview panel, if it is open. Safe from any thread.
    pub fn preview(&self) {
        self.repaint(|ids| ids.preview.into_iter().collect());
    }

    /// Repaint every bar window, and the panels. Safe from any thread.
    pub fn bars(&self) {
        self.repaint(|ids| ids.tops.iter().chain(&ids.docks).chain(&ids.others).copied().collect());
    }
}
