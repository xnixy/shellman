//! The icon font: its glyph codes, the font setup, and icon text.

use egui::RichText;

// ---- Icons: Segoe Fluent Icons on Windows 11, Segoe MDL2 Assets on Windows 10. ----

#[allow(dead_code)]
pub mod glyph {
    pub const BACK: char = '\u{E72B}';
    pub const FORWARD: char = '\u{E72A}';
    pub const UP: char = '\u{E74A}';
    pub const REFRESH: char = '\u{E72C}';
    pub const SEARCH: char = '\u{E721}';
    pub const HOME: char = '\u{E80F}';
    pub const PC: char = '\u{E7F8}';
    pub const DESKTOP: char = '\u{E7F4}';
    pub const DOCUMENTS: char = '\u{E8A5}';
    pub const DOWNLOADS: char = '\u{E896}';
    pub const PICTURES: char = '\u{EB9F}';
    pub const MUSIC: char = '\u{EC4F}';
    pub const VIDEOS: char = '\u{E8B2}';
    pub const DRIVE: char = '\u{EDA2}';
    pub const FOLDER: char = '\u{E8B7}';
    pub const FILE: char = '\u{E7C3}';
    pub const SETTINGS: char = '\u{E713}';
    pub const ADD: char = '\u{E710}';
    pub const CLOSE: char = '\u{E711}';
    pub const CHEVRON: char = '\u{E76C}';
    pub const NEW_FOLDER: char = '\u{E8F4}';
    pub const DELETE: char = '\u{E74D}';
    pub const UNDO: char = '\u{E7A7}';
    pub const RENAME: char = '\u{E8AC}';
    pub const COPY: char = '\u{E8C8}';
    pub const CUT: char = '\u{E8C6}';
    pub const PASTE: char = '\u{E77F}';
    pub const OPEN_EXTERNAL: char = '\u{E8A7}';
    pub const SORT_UP: char = '\u{E70E}';
    pub const SORT_DOWN: char = '\u{E70D}';
    pub const CHECK: char = '\u{E73E}';
    pub const MORE: char = '\u{E712}';
    pub const INFO: char = '\u{E946}';
    pub const VIEW_LIST: char = '\u{E8FD}';
    pub const VIEW_GRID: char = '\u{F0E2}';
    pub const PIN: char = '\u{E718}';
    pub const UNPIN: char = '\u{E77A}';
    pub const FOLDER_FILL: char = '\u{E8D5}';
    pub const IMAGE: char = '\u{EB9F}';
    pub const VIDEO: char = '\u{E714}';
    pub const AUDIO: char = '\u{E8D6}';
    pub const ARCHIVE: char = '\u{E7B8}';
    pub const CODE: char = '\u{E943}';
    pub const DOC: char = '\u{E8A5}';
    pub const APP: char = '\u{E756}';
    pub const MINIMIZE: char = '\u{E921}';
    pub const MAXIMIZE: char = '\u{E922}';
    pub const RESTORE: char = '\u{E923}';
    pub const CHROME_CLOSE: char = '\u{E8BB}';
    // System controls: the control centre, and the top bar's status.
    pub const WIFI: char = '\u{E701}';
    pub const BLUETOOTH: char = '\u{E702}';
    pub const ETHERNET: char = '\u{E839}';
    pub const GLOBE: char = '\u{E774}';
    pub const MOON: char = '\u{E708}';
    pub const VOLUME: char = '\u{E767}';
    pub const MUTED: char = '\u{E74F}';
    pub const BRIGHTNESS: char = '\u{E706}';
    pub const MICROPHONE: char = '\u{E720}';
    pub const MICROPHONE_OFF: char = '\u{EC54}';
    pub const PLAY: char = '\u{E768}';
    pub const PAUSE: char = '\u{E769}';
    pub const PREVIOUS: char = '\u{E892}';
    pub const NEXT: char = '\u{E893}';
    pub const LOCK: char = '\u{E72E}';
    pub const POWER: char = '\u{E7E8}';
    pub const RESTART: char = '\u{E777}';
    pub const SIGN_OUT: char = '\u{E748}';
    pub const BATTERY: char = '\u{E83F}';
    pub const CHARGING: char = '\u{E945}';
}

pub fn icon_font(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("icons".into()))
}

/// Whether a text font was installed (and with it the semibold family).
static HAS_SEMIBOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Text in a semibold weight, for names and headings. A variable font file cannot be asked for
/// a weight (egui always draws its default instance), so this uses the family's separate
/// Semibold (or Bold) file, and the regular face when it has neither.
pub fn semibold_font(size: f32) -> egui::FontId {
    if HAS_SEMIBOLD.load(std::sync::atomic::Ordering::Relaxed) {
        egui::FontId::new(size, egui::FontFamily::Name("semibold".into()))
    } else {
        egui::FontId::proportional(size)
    }
}

/// Text in a light weight, for large numerals (the Today widget's date and temperature): the
/// family's Light face, else Segoe UI Light, else the regular face.
pub fn light_font(size: f32) -> egui::FontId {
    if HAS_SEMIBOLD.load(std::sync::atomic::Ordering::Relaxed) {
        egui::FontId::new(size, egui::FontFamily::Name("light".into()))
    } else {
        egui::FontId::proportional(size)
    }
}

pub fn icon(c: char, size: f32) -> RichText {
    RichText::new(c.to_string()).font(icon_font(size))
}

/// Called once a text font is installed: the semibold family then exists.
pub(crate) fn mark_text_font_installed() {
    HAS_SEMIBOLD.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// egui's fonts for a new window: the saved font choice (SF Pro when installed, else Segoe UI
/// Variable) and the icon font. `Appearance::apply` swaps the text font later when the choice
/// changes; the icon font is never touched.
pub fn install_fonts(ctx: &egui::Context) {
    crate::fonts::install_saved(ctx);
}
