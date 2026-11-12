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
}

pub fn icon_font(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("icons".into()))
}

pub fn icon(c: char, size: f32) -> RichText {
    RichText::new(c.to_string()).font(icon_font(size))
}

/// egui's fonts, Segoe UI for text when present, and the icon font.
pub fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontFamily};
    use std::sync::Arc;
    let mut fonts = egui::FontDefinitions::default();
    // Segoe UI Variable on Windows 11, Segoe UI before it.
    let text_font = [
        r"C:\Windows\Fonts\SegUIVar.ttf",
        r"C:\Windows\Fonts\segoeui.ttf",
    ]
    .into_iter()
    .find_map(|p| std::fs::read(p).ok());
    if let Some(bytes) = text_font {
        fonts
            .font_data
            .insert("segoe".into(), Arc::new(FontData::from_owned(bytes)));
        if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
            family.insert(0, "segoe".into());
        }
    }
    for path in [
        r"C:\Windows\Fonts\SegoeIcons.ttf",
        r"C:\Windows\Fonts\segmdl2.ttf",
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("icons".into(), Arc::new(FontData::from_owned(bytes)));
            fonts
                .families
                .insert(FontFamily::Name("icons".into()), vec!["icons".into()]);
            break;
        }
    }
    ctx.set_fonts(fonts);
}
