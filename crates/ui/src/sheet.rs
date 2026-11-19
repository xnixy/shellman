//! An app window as one sheet of glass, Glasshouse's window: the sidebar on the window's own
//! glass at the left, a header bar across the rest with the page's title in its middle and
//! Windows' own buttons flush in the corner, and the content pane under it, a shade denser than
//! the sidebar so the two read as two panes.
//!
//! The window is one island (see [`Sheet::island`]): glass in every window style, painted solid
//! where there is no glass. Its corners are Windows 11's, and square when maximized.

use crate::chips;
use crate::glass::Island;
use crate::theme::{colors, semibold_font, shape};
use egui::{pos2, vec2, Align2, CornerRadius, Id, Rect, Sense, Stroke, Ui};

/// The header bar's height, and the window buttons' (Windows' tall title bar).
pub const HEADER_H: f32 = 48.0;
/// Windows 11's window corners.
pub const RADIUS: f32 = 8.0;
/// How far in from the sidebar's left its name starts.
const NAME_X: f32 = 20.0;

/// Where a sheet window's parts are, for a frame.
#[derive(Clone, Copy, Debug)]
pub struct Sheet {
    pub window: Rect,
    /// The window's corner radius: 0 when maximized.
    pub radius: f32,
    /// The left column, the window's whole height.
    pub sidebar: Rect,
    /// The bar across the top of the pane right of the sidebar.
    pub header: Rect,
    /// The pane under the header.
    pub body: Rect,
}

impl Sheet {
    /// The parts of a window filling `screen`, with a sidebar `sidebar_w` wide.
    pub fn new(ctx: &egui::Context, screen: Rect, sidebar_w: f32) -> Self {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let split = (screen.left() + sidebar_w).min(screen.right());
        Self {
            window: screen,
            radius: if maximized { 0.0 } else { RADIUS },
            sidebar: Rect::from_min_max(screen.min, pos2(split, screen.bottom())),
            header: Rect::from_min_max(pos2(split, screen.top()), pos2(screen.right(), screen.top() + HEADER_H)),
            body: Rect::from_min_max(pos2(split, screen.top() + HEADER_H), screen.max),
        }
    }

    /// The window's one piece of glass.
    pub fn island(&self) -> Island {
        Island::new(self.window, self.radius)
    }

    /// The content pane's denser shade (the header included) and the hairline between it and the
    /// sidebar. High contrast: no shade, the line in the text colour.
    pub fn paint(&self, ui: &Ui) {
        let c = colors();
        let p = ui.painter();
        let pane = Rect::from_min_max(pos2(self.sidebar.right(), self.window.top()), self.window.max);
        let r = self.radius.round() as u8;
        if !c.high_contrast {
            p.rect_filled(pane, CornerRadius { nw: 0, ne: r, sw: 0, se: r }, c.page_glass);
        }
        // A window without a sidebar (Terminal, the installer) has no line at its left edge.
        if self.sidebar.width() >= 1.0 {
            let line = if c.high_contrast { c.text } else { c.hairline };
            p.vline(self.sidebar.right(), self.window.y_range(), Stroke::new(1.0_f32, line));
        }
    }

    /// The header bar: dragging it moves the window and a double click maximizes it; `title` in
    /// its middle; the window buttons flush in the corner. Returns the room left of the title, for
    /// the page's own tools (a search box).
    pub fn header(&self, ui: &mut Ui, title: &str) -> Rect {
        let c = colors();
        let bar = self.header;
        let drag = ui.interact(bar, Id::new("sheet-header"), Sense::click_and_drag());
        crate::widgets::window_drag(ui, &drag);
        let buttons = Rect::from_min_size(pos2(self.window.right() - chips::CAPTIONS_W, self.window.top()), vec2(chips::CAPTIONS_W, HEADER_H));
        chips::window_buttons_flush(ui, buttons, self.radius);
        let galley = ui.fonts(|f| f.layout_no_wrap(title.to_owned(), semibold_font(15.0), c.text));
        // In the middle of the bar, moved left only as far as it must to clear the buttons.
        let half = galley.size().x / 2.0;
        let x = bar.center().x.min(buttons.left() - 16.0 - half).max(bar.left() + 16.0 + half);
        let at = pos2(x, bar.center().y);
        ui.painter().galley(at - galley.size() / 2.0, galley.clone(), c.text);
        Rect::from_min_max(pos2(bar.left() + 12.0, bar.top()), pos2((x - half - 16.0).max(bar.left() + 12.0), bar.bottom()))
    }

    /// The sidebar's head, as tall as the header: the app's `name` (when `name` is not empty).
    /// Dragging it moves the window. Returns the sidebar under it.
    pub fn sidebar_head(&self, ui: &mut Ui, name: &str) -> Rect {
        let c = colors();
        let head = Rect::from_min_size(self.sidebar.min, vec2(self.sidebar.width(), HEADER_H));
        let drag = ui.interact(head, Id::new("sheet-sidebar-head"), Sense::click_and_drag());
        crate::widgets::window_drag(ui, &drag);
        if !name.is_empty() {
            ui.painter().text(pos2(head.left() + NAME_X, head.center().y), Align2::LEFT_CENTER, name, semibold_font(15.0), c.text);
        }
        Rect::from_min_max(pos2(self.sidebar.left(), head.bottom()), self.sidebar.max)
    }
}

/// The colours of a box on the sheet: the card fill and a hairline (the window colour inside a
/// text-coloured outline in high contrast).
fn card_colors() -> (egui::Color32, egui::Color32) {
    let c = colors();
    if c.high_contrast { (c.bg, c.text) } else { (c.card, c.hairline) }
}

/// A box on the sheet, Glasshouse's boxed list: the card fill with a hairline and the island's
/// rounded corners. It is no glass of its own: it sits on the window's.
pub fn card(ui: &Ui, rect: Rect) {
    let (fill, edge) = card_colors();
    ui.painter().rect(rect, shape::corner(shape::ISLAND), fill, Stroke::new(1.0_f32, edge), egui::StrokeKind::Inside);
}

/// A [`card`] as a frame around content, with `margin` inside it.
pub fn card_frame(margin: egui::Margin) -> egui::Frame {
    let (fill, edge) = card_colors();
    egui::Frame::new().fill(fill).stroke(Stroke::new(1.0_f32, edge)).corner_radius(shape::corner(shape::ISLAND)).inner_margin(margin)
}

/// The box behind a text field an app lays out itself (a search box, a path bar): the field fill
/// rounded as a field. High contrast outlines it in the text colour, as egui's own fields are.
pub fn field(ui: &Ui, rect: Rect) {
    let c = colors();
    if c.high_contrast {
        ui.painter().rect(rect, shape::corner(shape::FIELD), c.bg, Stroke::new(1.0_f32, c.text), egui::StrokeKind::Inside);
    } else {
        ui.painter().rect_filled(rect, shape::corner(shape::FIELD), c.field);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parts_tile_the_window() {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 700.0));
        let s = Sheet::new(&ctx, screen, 240.0);
        assert_eq!(s.sidebar.right(), s.header.left());
        assert_eq!(s.header.left(), s.body.left());
        assert_eq!(s.header.bottom(), s.body.top());
        assert_eq!(s.body.max, screen.max);
        assert_eq!(s.sidebar.height(), screen.height());
        assert_eq!(s.radius, RADIUS);
    }
}
