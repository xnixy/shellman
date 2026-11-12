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
}

impl Wake {
    pub fn new(ctx: egui::Context) -> Self {
        Self { ctx, ids: Arc::default() }
    }

    /// The bar viewports there are now.
    pub fn set(&self, ids: Vec<egui::ViewportId>) {
        *self.ids.lock().unwrap() = ids;
    }

    /// Repaint every bar. Safe from any thread.
    pub fn bars(&self) {
        for &id in self.ids.lock().unwrap().iter() {
            self.ctx.request_repaint_of(id);
        }
    }
}
