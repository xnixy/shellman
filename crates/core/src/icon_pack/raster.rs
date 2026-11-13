//! SVG to pixels, for pack icons. The shell cannot draw SVG, so this is the one image kind the
//! thumbnail workers draw themselves.

use egui::ColorImage;
use resvg::{tiny_skia, usvg};
use std::path::Path;

/// Is this an SVG file, going by its name?
pub fn is_svg(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg"))
}

/// The SVG file drawn to fit a `px` by `px` square, centred, on a transparent background.
pub fn svg_file(path: &Path, px: u32) -> Option<ColorImage> {
    // The built-in pack's icons are compiled in, not on disk.
    if let Some(data) = super::builtin::bytes(path, px) {
        return svg_data(data, px);
    }
    // A pack's icon is tens of kilobytes; anything far bigger is not an icon.
    if std::fs::metadata(path).ok()?.len() > 4 * 1024 * 1024 {
        return None;
    }
    svg_data(&std::fs::read(path).ok()?, px)
}

pub fn svg_data(data: &[u8], px: u32) -> Option<ColorImage> {
    let px = px.clamp(8, 1024);
    // No text: icons are paths, and loading system fonts would cost far more than the drawing.
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(data, &options).ok()?;
    let size = tree.size();
    if size.width() <= 0.0 || size.height() <= 0.0 {
        return None;
    }
    let scale = (px as f32 / size.width()).min(px as f32 / size.height());
    let (w, h) = (size.width() * scale, size.height() * scale);
    let mut pixmap = tiny_skia::Pixmap::new(px, px)?;
    let transform = tiny_skia::Transform::from_translate((px as f32 - w) / 2.0, (px as f32 - h) / 2.0).pre_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some(ColorImage::from_rgba_premultiplied([px as usize; 2], pixmap.data()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED_SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="32" viewBox="0 0 64 32"><rect width="64" height="32" fill="#ff0000"/></svg>"##;

    #[test]
    fn draws_fitted_and_centred() {
        let img = svg_data(RED_SQUARE.as_bytes(), 64).unwrap();
        assert_eq!(img.size, [64, 64]);
        let at = |x: usize, y: usize| img.pixels[y * 64 + x];
        assert_eq!(at(32, 32).r(), 255);
        assert_eq!(at(32, 32).a(), 255);
        // The 2:1 image leaves the top and bottom quarters empty.
        assert_eq!(at(32, 2).a(), 0);
        assert_eq!(at(32, 62).a(), 0);
    }

    #[test]
    fn rubbish_is_not_an_image() {
        assert!(svg_data(b"not an svg", 32).is_none());
        assert!(svg_data(b"", 32).is_none());
        assert!(svg_file(Path::new(r"Z:\no\such.svg"), 32).is_none());
    }

    #[test]
    fn svg_by_name() {
        assert!(is_svg(Path::new("a.SVG")));
        assert!(!is_svg(Path::new("a.png")));
    }
}
