//! Repainting every bar window at once.
//!
//! Each bar is its own egui viewport with its own frames; a change they all show (a window
//! opened, the app in front, a pin) must repaint each of them. `request_repaint` from another
//! thread would only reach whichever window happens to be current, so wake-ups go through here.

use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Wake {
    ctx: egui::Context,
    ids: Arc<Mutex<Vec<egui::ViewportId>>>,
    tray: Arc<Mutex<Vec<egui::ViewportId>>>,
}

impl Wake {
    pub fn new(ctx: egui::Context) -> Self {
        Self { ctx, ids: Arc::default(), tray: Arc::default() }
    }

    /// The bar viewports there are now.
    pub fn set(&self, ids: Vec<egui::ViewportId>) {
        *self.ids.lock().unwrap() = ids;
    }

    /// The top bar viewports that show the tray.
    pub fn set_tray(&self, ids: Vec<egui::ViewportId>) {
        *self.tray.lock().unwrap() = ids;
    }

    /// Repaint the top bars that show the tray, and nothing else. Safe from any thread.
    pub fn tray(&self) {
        for &id in self.tray.lock().unwrap().iter() {
            self.ctx.request_repaint_of(id);
        }
    }

    /// Repaint every bar. Safe from any thread.
    pub fn bars(&self) {
        for &id in self.ids.lock().unwrap().iter() {
            self.ctx.request_repaint_of(id);
        }
    }
}
