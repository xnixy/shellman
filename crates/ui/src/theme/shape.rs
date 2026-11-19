//! Glasshouse's shapes, in points: one place for the corner radii and row heights every app uses.
//!
//! Pills for buttons and chips; rounded panels with hairlines for groups; softer squares for
//! fields, tabs and the chosen row in a sidebar or menu.

/// An island of glass: a panel, a group of settings, a card. Glasshouse's boxed list (20 px).
pub const ISLAND: f32 = 20.0;
/// A menu, a list of choices, a flyout under the top bar.
pub const POPOVER: f32 = 18.0;
/// A dialog over a window.
pub const DIALOG: f32 = 24.0;
/// A row in a menu or popover.
pub const MENU_ROW: f32 = 10.0;
/// A row in a sidebar.
pub const NAV_ROW: f32 = 14.0;
/// How tall a sidebar row is.
pub const NAV_ROW_H: f32 = 36.0;
/// A text field, a drop-down's button, a search box.
pub const FIELD: f32 = 12.0;
/// A tab.
pub const TAB: f32 = 12.0;
/// A tile in a grid (an app, a file, a window preview).
pub const TILE: f32 = 14.0;
/// A check box.
pub const CHECK: f32 = 6.0;
/// A tooltip.
pub const TOOLTIP: f32 = 10.0;

/// `r` as egui's whole-point corner radius.
pub fn corner(r: f32) -> egui::CornerRadius {
    egui::CornerRadius::same(r.round().clamp(0.0, 255.0) as u8)
}

/// As round as `rect` allows: a pill.
pub fn pill(rect: egui::Rect) -> egui::CornerRadius {
    corner(rect.height().min(rect.width()) / 2.0)
}
