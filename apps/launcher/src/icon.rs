//! An app's icon from the shell, or a lettered plate until the shell's image arrives.
//!
//! The shell is asked for a 256 px image (see `Thumbs::icons`), already cropped to what is
//! drawn. A real big icon is fitted into the box. A small old icon (the shell pads those to the
//! size asked for) is drawn at its own pixel size, centred on a soft plate, rather than blown up.

use egui::{Color32, Painter, Rect};
use shellman_core::apps_index::App;
use shellman_core::thumbs::Thumbs;
use shellman_ui::theme::colors;
use std::path::Path;

/// Icons whose longest side is under this many pixels count as small ones.
const SMALL_PX: f32 = 48.0;

/// A faint rounded plate in the text colour: it suits light and dark, and shows glass through.
pub fn plate(p: &Painter, rect: Rect) {
    p.rect_filled(rect, rect.width() * 0.22, colors().text.gamma_multiply(0.06));
}

/// Draw the app's icon in `rect`.
pub fn paint(p: &Painter, rect: Rect, app: &App, thumbs: &mut Thumbs) {
    let path = app.shell_path();
    let Some(texture) = thumbs.get(Path::new(&path), true) else {
        placeholder(p, rect, app);
        return;
    };
    let ppp = p.ctx().pixels_per_point();
    let px = texture.size_vec2();
    let shown = if px.max_elem() < SMALL_PX {
        plate(p, rect);
        // Whole-number scale up to a little over half the plate, so it stays sharp.
        let k = (rect.width() * 0.62 * ppp / px.max_elem()).floor().max(1.0);
        px * k / ppp
    } else {
        px * (rect.width() / px.max_elem())
    };
    p.image(
        texture.id(),
        Rect::from_center_size(rect.center(), shown),
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

/// The plate with the app's first letter, while the shell's image is on its way.
fn placeholder(p: &Painter, rect: Rect, app: &App) {
    plate(p, rect);
    let letter = app.name.chars().next().unwrap_or('?').to_uppercase().to_string();
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        egui::FontId::proportional(rect.width() * 0.42),
        colors().muted,
    );
}
