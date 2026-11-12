//! Keeps egui's shared font texture sound when one process has windows at different scales.
//!
//! egui 0.31 keeps a font atlas per pixels-per-point, but every window draws from the one font
//! texture. At the end of each window's frame egui first applies that window's new glyphs as a
//! partial update of the texture, and only then, when there are several scales, uploads the
//! window's whole atlas over it. The texture still has the size of the last window's atlas, so
//! when this window's atlas has grown taller (a 300% monitor's fills faster than a 150% one's)
//! the partial update lies outside it, and a debug build panics ("Partial texture update is
//! outside the bounds of texture Managed(0)").
//!
//! `end_frame` heads that off: when the window has new glyphs and its atlas is not the size of
//! the texture, it uploads the whole atlas itself, which leaves egui no partial update to make.

use egui::epaint::{textures::TextureOptions, ImageDelta, TextureAtlas};
use egui::TextureId;

/// Call last in every frame of every window of a process that has windows on monitors of
/// different scales.
pub fn end_frame(ctx: &egui::Context) {
    let atlas = ctx.fonts(|f| f.texture_atlas());
    let mut atlas = atlas.lock();
    let manager = ctx.tex_manager();
    let mut manager = manager.write();
    let Some(meta) = manager.meta(TextureId::default()) else { return };
    if meta.size == atlas.size() {
        return;
    }
    if atlas.take_delta().is_some() {
        let options: TextureOptions = TextureAtlas::texture_options();
        manager.set(TextureId::default(), ImageDelta::full(atlas.image().clone(), options));
    }
}
