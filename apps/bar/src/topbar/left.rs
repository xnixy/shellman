//! The top bar's left side: the launcher mark, the app in front, and its window title.

use super::{highlight, open_start};
use shellman_core::system::app_name;
use shellman_core::winlist::AppWindow;
use shellman_ui::theme::{colors, semibold_font, with_alpha};
use shellman_ui::widgets::draw_text;

/// The launcher mark's size and the round highlight behind it, in points.
const MARK: f32 = 14.0;
const ROUND: f32 = 28.0;
/// Where the mark's centre sits from the left edge; the click area runs from the edge to past it.
const MARK_X: f32 = 20.0;
const NAME_GAP: f32 = 14.0;
const TITLE_GAP: f32 = 10.0;

/// The launcher mark, then the app in front and its window title, none of it past `limit`.
pub fn draw(ui: &mut egui::Ui, bar: egui::Rect, front: Option<&AppWindow>, limit: f32) {
    let c = colors();
    // The click area reaches the screen edge, so a flick into the corner hits it.
    let button = egui::Rect::from_min_size(bar.min, egui::vec2(MARK_X + ROUND / 2.0 + 2.0, bar.height()));
    let center = egui::pos2(bar.left() + MARK_X, bar.center().y);
    let resp = ui
        .interact(button, egui::Id::new("launcher"), egui::Sense::click())
        .on_hover_text("Start (right-click for settings)");
    if resp.hovered() || resp.is_pointer_button_down_on() {
        let round = egui::Rect::from_center_size(center, egui::Vec2::splat(ROUND));
        highlight(ui, round, &resp, (ROUND / 2.0) as u8);
    }
    // The text colour, never the accent: a dark accent would make the mark vanish.
    paint_launcher_mark(ui.painter(), center, MARK / 12.5, with_alpha(c.text, 230));
    if resp.clicked() {
        open_start();
    }
    if resp.secondary_clicked() {
        shellman_core::sibling::start("settings");
    }
    let Some(w) = front else { return };
    let name = app_name::display_name(&w.app_id, w.exe.as_deref());
    let font = semibold_font(13.0);
    let name_w = ui.fonts(|f| f.layout_no_wrap(name.clone(), font.clone(), c.text)).size().x;
    let x = center.x + MARK / 2.0 + NAME_GAP;
    let name_rect = egui::Rect::from_min_max(egui::pos2(x, bar.top()), egui::pos2((x + name_w).min(limit), bar.bottom()));
    draw_text(ui, name_rect, &name, font, c.text, false);
    if let Some(title) = clean_title(&w.title, &name) {
        let left = name_rect.right() + TITLE_GAP;
        let r = egui::Rect::from_min_max(egui::pos2(left, bar.top()), egui::pos2(limit, bar.bottom()));
        // Cut with an ellipsis when it does not fit.
        draw_text(ui, r, title, egui::FontId::proportional(13.0), c.muted, false);
    }
}

/// The window title worth showing next to the app's name: none when it only repeats the name,
/// and without a trailing " - Name" (windows often end their title with the app's name).
fn clean_title<'a>(title: &'a str, name: &str) -> Option<&'a str> {
    let title = title.trim();
    if title.is_empty() || title.eq_ignore_ascii_case(name) {
        return None;
    }
    for dash in [" - ", " \u{2013} ", " \u{2014} "] {
        let cut = title.len().checked_sub(dash.len() + name.len());
        if let Some(cut) = cut.filter(|&i| title.is_char_boundary(i)) {
            let (rest, tail) = title.split_at(cut);
            if tail.len() == dash.len() + name.len()
                && tail.starts_with(dash)
                && tail[dash.len()..].eq_ignore_ascii_case(name)
            {
                return Some(rest.trim()).filter(|r| !r.is_empty() && !r.eq_ignore_ascii_case(name));
            }
        }
    }
    Some(title)
}

/// Four small squares: the launcher's mark, drawn so it doesn't depend on a font glyph. `scale`
/// 1.0 is 12.5 points across.
pub fn paint_launcher_mark(painter: &egui::Painter, center: egui::Pos2, scale: f32, color: egui::Color32) {
    for (dx, dy) in [(-3.5, -3.5), (3.5, -3.5), (-3.5, 3.5), (3.5, 3.5)] {
        let sq = egui::Rect::from_center_size(center + egui::vec2(dx, dy) * scale, egui::Vec2::splat(5.5 * scale));
        painter.rect_filled(sq, egui::CornerRadius::same((2.0 * scale).round() as u8), color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_that_repeat_the_name_are_dropped() {
        assert_eq!(clean_title("Files", "Files"), None);
        assert_eq!(clean_title("  files ", "Files"), None);
        assert_eq!(clean_title("", "Files"), None);
        assert_eq!(clean_title("Files - Files", "Files"), None);
    }

    #[test]
    fn a_trailing_app_name_is_cut() {
        assert_eq!(clean_title("iCloud Drive - Files", "Files"), Some("iCloud Drive"));
        assert_eq!(clean_title("Inbox \u{2014} Mail", "Mail"), Some("Inbox"));
        assert_eq!(clean_title("Profiles", "Files"), Some("Profiles"));
        assert_eq!(clean_title("Untitled - Notepad", "Editor"), Some("Untitled - Notepad"));
    }
}
