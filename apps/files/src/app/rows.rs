//! Pieces of a file row: its icon and its right-click menu.

use super::actions::Action;
use super::tab::Loc;
use egui::{Align2, Color32, CornerRadius, Rect, TextureId, Ui, Vec2};
use shellman_core::fsops::{Entry, Op};
use shellman_core::thumbs;
use shellman_ui::theme::{self, glyph, icon_font};
use shellman_ui::widgets::folder_icon;
use shellman_ui::widgets::menu_item;
use std::path::Path;

/// The right-click menu of a file row.
pub(super) fn row_menu(ui: &mut Ui, actions: &mut Vec<Action>, is_dir: bool, loc: &Loc) {
    if menu_item(ui, glyph::OPEN_EXTERNAL, "Open", "Enter") {
        actions.push(Action::OpenSelected);
        ui.close_menu();
    }
    if is_dir && menu_item(ui, glyph::ADD, "Open in new tab", "") {
        actions.push(Action::NewTab(loc.clone()));
        ui.close_menu();
    }
    if menu_item(ui, glyph::FOLDER, "Show in Explorer", "") {
        actions.push(Action::Reveal);
        ui.close_menu();
    }
    ui.separator();
    if menu_item(ui, glyph::CUT, "Cut", "Ctrl+X") {
        actions.push(Action::Clip(Op::Move));
        ui.close_menu();
    }
    if menu_item(ui, glyph::COPY, "Copy", "Ctrl+C") {
        actions.push(Action::Clip(Op::Copy));
        ui.close_menu();
    }
    if menu_item(ui, glyph::COPY, "Copy path", "Ctrl+Shift+C") {
        actions.push(Action::CopyPath);
        ui.close_menu();
    }
    ui.separator();
    if menu_item(ui, glyph::RENAME, "Rename", "F2") {
        actions.push(Action::Rename);
        ui.close_menu();
    }
    if menu_item(ui, glyph::DELETE, "Move to Recycle Bin", "Del") {
        actions.push(Action::Recycle);
        ui.close_menu();
    }
    ui.separator();
    more_options(ui, actions);
}

/// The last entries of every menu: Properties, and the full Windows menu.
pub(super) fn more_options(ui: &mut Ui, actions: &mut Vec<Action>) {
    if menu_item(ui, glyph::INFO, "Properties", "Alt+Enter") {
        actions.push(Action::Properties);
        ui.close_menu();
    }
    if menu_item(ui, glyph::MORE, "Show more options", "Shift+F10") {
        actions.push(Action::ShellMenu { extended: false });
        ui.close_menu();
    }
}

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
