//! Pieces of a file row: its icon. (Its right-click menu is in `menu`.)

use egui::{Align2, Color32, CornerRadius, Rect, TextureId, Ui, Vec2};
use shellman_core::fsops::Entry;
use shellman_core::thumbs;
use shellman_ui::theme::{self, glyph, icon_font};
use shellman_ui::widgets::folder_icon;
use std::path::Path;

/// A glyph and color for a file, by its type.
pub(super) fn file_icon(e: &Entry, c: &theme::Colors) -> (char, Color32) {
    if e.is_dir {
        return (glyph::FOLDER_FILL, c.accent_text);
    }
    let ext = Path::new(&e.name)
        .extension()
        .map(|x| x.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let (g, rgb): (char, [u8; 3]) = match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "heic" | "ico" | "tif"
        | "tiff" | "avif" => (glyph::IMAGE, [214, 92, 168]),
        "mp4" | "mkv" | "mov" | "avi" | "webm" | "wmv" | "m4v" => (glyph::VIDEO, [226, 96, 72]),
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "opus" => (glyph::AUDIO, [222, 160, 40]),
        "zip" | "7z" | "rar" | "tar" | "gz" | "xz" | "bz2" | "zst" | "cab" | "iso" => {
            (glyph::ARCHIVE, [184, 132, 76])
        }
        "rs" | "js" | "ts" | "tsx" | "jsx" | "py" | "c" | "h" | "cpp" | "hpp" | "cs" | "go"
        | "java" | "kt" | "swift" | "rb" | "php" | "sh" | "ps1" | "bat" | "cmd" | "json"
        | "toml" | "yaml" | "yml" | "xml" | "html" | "css" | "scss" | "lua" | "sql" | "lock" => {
            (glyph::CODE, [56, 170, 160])
        }
        "pdf" => (glyph::DOC, [220, 72, 72]),
        "doc" | "docx" | "txt" | "md" | "rtf" | "odt" | "log" | "ini" | "cfg" => {
            (glyph::DOC, [72, 132, 220])
        }
        "xls" | "xlsx" | "csv" | "ods" => (glyph::DOC, [40, 160, 96]),
        "ppt" | "pptx" | "odp" => (glyph::DOC, [222, 112, 48]),
        "exe" | "msi" | "appx" | "msix" | "dll" => (glyph::APP, [120, 128, 220]),
        _ => return (glyph::FILE, c.muted),
    };
    let col = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    let col = if c.light {
        theme::mix(col, Color32::BLACK, 0.12)
    } else {
        theme::mix(col, Color32::WHITE, 0.15)
    };
    (g, col)
}

/// The icon of a list row, in one 20pt slot centred on `center` so names line up whatever the
/// file is: a folder drawing, a cropped photo, a fitted program icon, or just a coloured glyph.
pub(super) fn list_icon(
    ui: &Ui,
    center: egui::Pos2,
    e: &Entry,
    thumb: Option<(TextureId, Vec2)>,
    c: &theme::Colors,
    dim: impl Fn(Color32) -> Color32,
) {
    let slot = Rect::from_center_size(center, Vec2::splat(20.0));
    if e.is_dir {
        let icon = Rect::from_center_size(center, Vec2::new(18.0, 14.0));
        folder_icon(ui.painter(), icon, c.accent_text, dim);
    } else if let Some((id, size)) = thumb {
        let image = egui::Image::from_texture((id, size)).tint(dim(Color32::WHITE));
        if thumbs::is_program(&e.name) {
            let fitted = size * (20.0 / size.x).min(20.0 / size.y);
            image.paint_at(ui, Rect::from_center_size(center, fitted));
        } else {
            // Centre crop, so photos of any shape fill the same square.
            let (u, v) = (
                (1.0 - size.y / size.x).max(0.0) / 2.0,
                (1.0 - size.x / size.y).max(0.0) / 2.0,
            );
            let uv = Rect::from_min_max(egui::pos2(u, v), egui::pos2(1.0 - u, 1.0 - v));
            image
                .uv(uv)
                .corner_radius(CornerRadius::same(4))
                .paint_at(ui, slot);
        }
    } else {
        let (g, gcol) = file_icon(e, c);
        ui.painter()
            .text(center, Align2::CENTER_CENTER, g, icon_font(15.0), dim(gcol));
    }
}
