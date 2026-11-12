//! An app's icon from the shell, or a lettered tile until the shell's image arrives.

use shellman_core::apps_index::App;
use shellman_core::thumbs::Thumbs;
use shellman_ui::theme::colors;
use std::path::Path;

/// Draw the app's icon in `rect`. `large` asks for the shell's big image (the grid) rather than
/// the small one (result rows).
pub fn paint(ui: &egui::Ui, rect: egui::Rect, app: &App, large: bool, thumbs: &mut Thumbs) {
    let path = app.shell_path();
    if let Some(texture) = thumbs.get(Path::new(&path), large) {
        ui.painter().image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        return;
    }
    let c = colors();
    ui.painter()
        .rect_filled(rect.shrink(rect.width() * 0.08), rect.width() * 0.22, c.control);
    let letter = app.name.chars().next().unwrap_or('?').to_uppercase().to_string();
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        egui::FontId::proportional(rect.width() * 0.5),
        c.muted,
    );
}
