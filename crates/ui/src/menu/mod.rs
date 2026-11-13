//! Shellman's menus. [`popup`] is the one menu every app shows: a window of its own, on glass,
//! the desktop's right-click menu. What is left here are flat menu rows ([`flat_item`]) for the
//! few lists that are not menus but panels inside a window (Files' tag editor, the terminal's
//! profile list until it moves to the popup).

pub mod dropdown;
mod keys;
pub mod popup;
mod row;

use egui::{Sense, Vec2};

pub use row::{Icon, ROW_H};

/// One line of a menu.
#[derive(Clone)]
pub struct Row<T> {
    pub icon: Icon,
    pub label: String,
    pub shortcut: String,
    pub enabled: bool,
    pub checked: bool,
    /// The letter that picks it from the keyboard.
    pub mnemonic: Option<char>,
    pub kind: Kind<T>,
}

#[derive(Clone)]
pub enum Kind<T> {
    Action(T),
    Submenu {
        rows: Vec<Row<T>>,
        /// Sent back as `Event::Opened` when the submenu opens: for entries filled on demand.
        on_open: Option<T>,
    },
    Separator,
    /// A quiet line that cannot be picked: "Loading", "2 items took too long".
    Note,
    /// A row someone else drew (an owner-drawn shell entry), shown as the picture it is, picked
    /// as `T`.
    Picture(egui::TextureId, Vec2, T),
}

impl<T> Row<T> {
    pub fn action(icon: Icon, label: impl Into<String>, shortcut: impl Into<String>, value: T) -> Self {
        Self {
            icon,
            label: label.into(),
            shortcut: shortcut.into(),
            enabled: true,
            checked: false,
            mnemonic: None,
            kind: Kind::Action(value),
        }
    }

    pub fn separator() -> Self {
        Self::plain(Kind::Separator, "")
    }

    pub fn note(text: impl Into<String>) -> Self {
        Self::plain(Kind::Note, text)
    }

    fn plain(kind: Kind<T>, label: impl Into<String>) -> Self {
        Self {
            icon: Icon::None,
            label: label.into(),
            shortcut: String::new(),
            enabled: false,
            checked: false,
            mnemonic: None,
            kind,
        }
    }
}

/// One entry of an ordinary egui popup or context menu, drawn as a row of shellman's own menu:
/// flat, lit only while hovered, with an icon column, the label, and a shortcut on the right; a
/// tick in the icon column when `checked` and there is no icon. Use it for every entry of a
/// popup so the popup reads like the right-click menu. True when clicked.
///
/// ```ignore
/// resp.context_menu(|ui| {
///     if menu::flat_item(ui, Icon::Glyph(glyph::ADD), "Open in new tab", "", false, true) {
///         ui.close_menu();
///     }
/// });
/// ```
pub fn flat_item(ui: &mut egui::Ui, icon: Icon, label: &str, shortcut: &str, checked: bool, enabled: bool) -> bool {
    let row = Row::<()> {
        icon,
        label: label.to_owned(),
        shortcut: shortcut.to_owned(),
        enabled,
        checked,
        mnemonic: None,
        kind: Kind::Action(()),
    };
    let width = ui.available_width().max(row::menu_width(ui.ctx(), std::slice::from_ref(&row)) - 2.0 * row::PAD);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, ROW_H), if enabled { Sense::click() } else { Sense::hover() });
    row::paint(ui, rect, &row, resp.hovered(), false);
    enabled && resp.clicked()
}

/// A thin line between groups of [`flat_item`]s.
pub fn flat_separator(ui: &mut egui::Ui) {
    let row = Row::<()>::separator();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row::height(&row)), Sense::hover());
    row::paint(ui, rect, &row, false, false);
}

/// Make an egui popup's own spacing match the flat rows: no gaps between them. Call first in
/// the popup.
pub fn flat_popup(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing = Vec2::ZERO;
}
