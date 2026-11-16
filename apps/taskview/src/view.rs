//! Drawing Task View and reading the mouse: the header pill, then one tile per window (its icon,
//! title and close button over its preview). Live previews are painted by the compositor on top
//! of the preview frames; where a window has none (minimized), its icon shows on a plate.
//!
//! Every tile and close button is a real egui widget: focusable, named for screen readers, with
//! the focus ring outside it. The selection is the focused tile.

use crate::layout::{Layout, Tile, HEADER_H};
use shellman_core::thumbs::Thumbs;
use shellman_core::winlist::AppWindow;
use shellman_ui::chips;
use shellman_ui::theme::{self, colors, glyph, icon_font, semibold_font};
use shellman_ui::widgets::draw_text;

const TILE_RADIUS: u8 = 16;
const PLATE_RADIUS: u8 = 8;
const BIG_ICON: f32 = 56.0;

pub struct View<'a> {
    pub windows: &'a [AppWindow],
    /// Each window's app, for its spoken name.
    pub apps: &'a [String],
    pub layout: &'a Layout,
    pub sel: usize,
    /// Which tiles have a live preview over them.
    pub live: &'a [bool],
    /// Windows left out for want of room.
    pub more: usize,
}

/// What the mouse (or Enter and Space on a focused widget) asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    /// Switch to this window.
    Pick(usize),
    /// Close this window (its close button, or a middle click on its tile).
    Close(usize),
    /// Leave Task View (a click beside the tiles, or its own close button).
    Dismiss,
}

pub fn tile_id(hwnd: isize) -> egui::Id {
    egui::Id::new(("taskview tile", hwnd))
}

pub fn close_id(hwnd: isize) -> egui::Id {
    egui::Id::new(("taskview close", hwnd))
}

/// The window's name as a screen reader says it.
pub fn spoken(window: &AppWindow, app: &str) -> String {
    let title = if window.title.trim().is_empty() { app } else { window.title.as_str() };
    let mut name = if app.is_empty() || app == title { title.to_owned() } else { format!("{title}, {app}") };
    if window.minimized {
        name.push_str(", minimized");
    }
    name
}

pub fn draw(ui: &mut egui::Ui, view: &View, icons: &mut Thumbs) -> Option<Action> {
    let c = colors();
    let mut action = None;
    // Beside the tiles: a click there leaves, as with Windows' own. Not a Tab stop.
    let back = ui.interact(ui.max_rect(), egui::Id::new("taskview backdrop"), egui::Sense::CLICK);
    if back.clicked() {
        action = Some(Action::Dismiss);
    }
    if header(ui, view) {
        action = Some(Action::Dismiss);
    }
    if view.windows.is_empty() {
        let mid = ui.max_rect().center();
        ui.painter().text(mid - egui::vec2(0.0, 20.0), egui::Align2::CENTER_CENTER, glyph::APP, icon_font(34.0), c.muted);
        ui.painter().text(mid + egui::vec2(0.0, 18.0), egui::Align2::CENTER_CENTER, "No windows are open", egui::FontId::proportional(14.0), c.muted);
        return action;
    }
    for (i, (window, tile)) in view.windows.iter().zip(&view.layout.tiles).enumerate() {
        let app = view.apps.get(i).map(String::as_str).unwrap_or("");
        let live = view.live.get(i).copied().unwrap_or(false);
        if let Some(a) = draw_tile(ui, window, app, tile, i == view.sel, live, icons) {
            action = Some(match a {
                TileAction::Pick => Action::Pick(i),
                TileAction::Close => Action::Close(i),
            });
        }
    }
    action
}

/// The header pill: "Task View", how many windows, and a button to leave. True: leave.
fn header(ui: &mut egui::Ui, view: &View) -> bool {
    let c = colors();
    let r = view.layout.header;
    let radius = HEADER_H / 2.0;
    pill(ui, r, radius, false, false);
    let name_at = egui::pos2(r.left() + 20.0, r.center().y);
    let name = ui.painter().text(name_at, egui::Align2::LEFT_CENTER, "Task View", semibold_font(17.0), c.text);
    let count = match (view.windows.len(), view.more) {
        (0, _) => String::new(),
        (1, 0) => "1 window".to_owned(),
        (n, 0) => format!("{n} windows"),
        (n, more) => format!("{n} of {} windows", n + more),
    };
    if !count.is_empty() {
        let at = egui::pos2(name.right() + 12.0, r.center().y);
        ui.painter().text(at, egui::Align2::LEFT_CENTER, count, egui::FontId::proportional(13.0), c.muted);
    }
    let close = egui::Rect::from_center_size(egui::pos2(r.right() - 8.0 - 15.0, r.center().y), egui::vec2(30.0, 30.0));
    let resp = chips::round_icon(ui, close, egui::Id::new("taskview leave"), glyph::CLOSE, true, false, "Close Task View");
    lock_arrows(ui, &resp);
    resp.on_hover_text("Close Task View (Esc)").clicked()
}

enum TileAction {
    Pick,
    Close,
}

fn draw_tile(ui: &mut egui::Ui, window: &AppWindow, app: &str, tile: &Tile, selected: bool, live: bool, icons: &mut Thumbs) -> Option<TileAction> {
    let c = colors();
    let resp = ui.interact(tile.outer, tile_id(window.hwnd), egui::Sense::click());
    let name = spoken(window, app);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &name));
    lock_arrows(ui, &resp);
    pill(ui, tile.outer, TILE_RADIUS as f32, selected, resp.hovered());
    theme::focus_ring(ui, &resp, tile.outer, TILE_RADIUS as f32);
    let icon = window.icon_source().and_then(|p| shellman_ui::app_icon::get(ui.ctx(), &p, true, icons));
    if !live {
        // Minimized, or not previewable: the app's icon on a plate.
        ui.painter().rect_filled(tile.preview, PLATE_RADIUS, c.well);
        if c.high_contrast {
            ui.painter().rect_stroke(tile.preview, PLATE_RADIUS, egui::Stroke::new(1.0_f32, c.muted), egui::StrokeKind::Inside);
        }
        paint_icon(ui, icon, tile.preview.center(), BIG_ICON.min(tile.preview.height() * 0.5));
        if window.minimized {
            let at = egui::pos2(tile.preview.center().x, tile.preview.bottom() - 14.0);
            ui.painter().text(at, egui::Align2::CENTER_CENTER, "Minimized", egui::FontId::proportional(11.5), c.muted);
        }
    }
    paint_icon(ui, icon, tile.icon.center(), tile.icon.width());
    let title = if window.title.trim().is_empty() { app } else { window.title.as_str() };
    draw_text(ui, tile.title, title, egui::FontId::proportional(13.0), c.text, false);
    let close = chips::round_icon(ui, tile.close, close_id(window.hwnd), glyph::CLOSE, true, true, &format!("Close {title}"));
    lock_arrows(ui, &close);
    if close.clicked() {
        return Some(TileAction::Close);
    }
    // No tooltip: it would open over the preview, which the compositor paints on top of.
    if resp.middle_clicked() {
        return Some(TileAction::Close);
    }
    if resp.clicked() && !close.hovered() {
        return Some(TileAction::Pick);
    }
    None
}

/// A tile's or the header's island: a card on the glass with its hairline; hovered, a wash;
/// selected, the accent's wash and outline. In high contrast: the window colour outlined in the
/// text colour, the selection a thick highlight outline, no washes.
fn pill(ui: &egui::Ui, r: egui::Rect, radius: f32, selected: bool, hovered: bool) {
    let c = colors();
    let radius = radius.round().clamp(0.0, 255.0) as u8;
    let p = ui.painter();
    p.rect_filled(r, radius, c.card);
    if c.high_contrast {
        let stroke = if selected { egui::Stroke::new(3.0_f32, c.row_selected) } else { egui::Stroke::new(1.0_f32, c.text) };
        p.rect_stroke(r, radius, stroke, egui::StrokeKind::Inside);
        return;
    }
    if selected {
        p.rect_filled(r, radius, c.row_selected);
        p.rect_stroke(r, radius, egui::Stroke::new(2.0_f32, c.accent_fill), egui::StrokeKind::Inside);
    } else {
        if hovered {
            p.rect_filled(r, radius, c.row_hover);
        }
        p.rect_stroke(r, radius, egui::Stroke::new(1.0_f32, c.hairline), egui::StrokeKind::Inside);
    }
}

/// Arrow keys are Task View's own (moving between tiles), not egui's focus moves; Esc too.
fn lock_arrows(ui: &egui::Ui, resp: &egui::Response) {
    if resp.has_focus() {
        let filter = egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: true };
        ui.memory_mut(|m| m.set_focus_lock_filter(resp.id, filter));
    }
}

/// An app icon of `size` points centred on `center`, or a generic glyph while it loads.
fn paint_icon(ui: &egui::Ui, icon: Option<(egui::TextureId, egui::Vec2)>, center: egui::Pos2, size: f32) {
    match icon {
        Some((id, px)) => {
            let scale = (size / px.x).min(size / px.y);
            egui::Image::from_texture((id, px)).paint_at(ui, egui::Rect::from_center_size(center, px * scale));
        }
        None => {
            ui.painter().text(center, egui::Align2::CENTER_CENTER, glyph::APP, icon_font(size * 0.6), colors().muted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(title: &str, minimized: bool) -> AppWindow {
        AppWindow { hwnd: 1, title: title.into(), app_id: String::new(), exe: None, minimized }
    }

    #[test]
    fn spoken_names_say_the_title_the_app_and_minimized() {
        assert_eq!(spoken(&window("Notes.txt - Notepad", false), "Notepad"), "Notes.txt - Notepad, Notepad");
        assert_eq!(spoken(&window("Calculator", false), "Calculator"), "Calculator");
        assert_eq!(spoken(&window("Inbox", true), "Mail"), "Inbox, Mail, minimized");
        assert_eq!(spoken(&window("  ", false), "Paint"), "Paint", "an untitled window is named by its app");
        assert_eq!(spoken(&window("Doc", false), ""), "Doc");
    }
}
