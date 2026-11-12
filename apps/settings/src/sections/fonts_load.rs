//! Faces the Fonts page draws with besides the window's own: a sample of each family in view, and
//! the preview's replacement faces. They are added to egui's fonts one by one (`add_font`), which
//! keeps the window's own fonts; a face added during a frame can be drawn with from a later one,
//! and egui panics on a family it does not know, so a face is used only once egui lists it.

use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::{FontData, FontFamily};
use std::collections::HashSet;

#[derive(Default)]
pub struct FontLoader {
    /// Asked of the reader and not answered yet.
    asked: HashSet<String>,
    /// Handed to egui, by key.
    sent: HashSet<String>,
    /// Keys egui has, as of the last check.
    live: HashSet<String>,
    /// Keys that could not be loaded (too big, unreadable): drawn in the window's font.
    failed: HashSet<String>,
}

impl FontLoader {
    pub fn mark_asked(&mut self, key: &str) {
        self.asked.insert(key.to_owned());
    }

    /// Hand egui a face, unless it already has it. Empty bytes mark a face that will not load.
    pub fn insert(&mut self, ctx: &egui::Context, key: String, bytes: &[u8], index: u32) {
        self.asked.remove(&key);
        if bytes.is_empty() {
            self.failed.insert(key);
            return;
        }
        if !self.sent.insert(key.clone()) {
            return;
        }
        let mut data = FontData::from_owned(bytes.to_vec());
        data.index = index;
        ctx.add_font(FontInsert {
            name: key.clone(),
            data,
            families: vec![InsertFontFamily { family: FontFamily::Name(key.as_str().into()), priority: FontPriority::Highest }],
        });
        ctx.request_repaint();
    }

    /// Whether the key is asked for, handed over, or known not to load: no need to ask again.
    pub fn known(&self, key: &str) -> bool {
        self.asked.contains(key) || self.sent.contains(key) || self.failed.contains(key)
    }

    /// Call once a frame: learn which handed-over faces egui has now. Another part of the window
    /// replacing egui's fonts wholesale drops them, and they are asked for again.
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        if self.sent.is_empty() {
            return;
        }
        let families: HashSet<String> = ctx
            .fonts(|f| f.families())
            .into_iter()
            .filter_map(|f| match f {
                FontFamily::Name(name) => Some(name.to_string()),
                _ => None,
            })
            .collect();
        let settled = self.sent.len() == self.live.len();
        self.live = self.sent.intersection(&families).cloned().collect();
        // All were there last frame and some are gone: the fonts were reset. Forget those.
        if settled && self.live.len() < self.sent.len() {
            self.sent = self.live.clone();
        }
    }

    /// The family to draw `key` with, once egui has it.
    pub fn family(&self, key: &str) -> Option<FontFamily> {
        self.live.contains(key).then(|| FontFamily::Name(key.into()))
    }
}
