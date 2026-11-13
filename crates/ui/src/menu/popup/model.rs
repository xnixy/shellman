//! What a popup menu shows, as the caller builds it ([`Entry`]), and the same without the
//! caller's ids ([`Row`]), which is what the one window per process keeps.

use egui::{TextureId, Vec2};

/// How many levels a menu has at most: the menu and three levels of submenus. A submenu deeper
/// than that shows as a disabled row.
pub const MAX_LEVELS: usize = 4;

/// One line of a popup menu, carrying `T`, the value handed back when it is picked.
///
/// A label may mark its keyboard letter with `&` ("&Paste"), as Windows menus do; without one,
/// typing a row's first letter moves to it (and picks it when it is the only such row).
#[derive(Clone, Debug)]
pub enum Entry<T> {
    Item(Item<T>),
    /// A row that opens a submenu. `on_open`, when set, is handed back by
    /// [`PopupMenu::take_opened`](super::PopupMenu::take_opened) when it opens: for entries
    /// filled on demand (the caller then [`update`](super::PopupMenu::update)s the menu).
    Submenu { label: String, icon: Option<char>, image: Option<TextureId>, enabled: bool, entries: Vec<Entry<T>>, on_open: Option<T> },
    Separator,
    /// A quiet line above a group, never picked.
    Heading(String),
    /// A quiet line in the label's place, never picked: "Loading…".
    Note(String),
    /// A row someone else drew (an owner-drawn shell entry), shown as the picture it is (`size`
    /// in points), picked as `id`.
    Picture { id: T, texture: TextureId, size: Vec2, enabled: bool },
}

/// A row that can be picked.
#[derive(Clone, Debug)]
pub struct Item<T> {
    pub id: T,
    pub label: String,
    /// A glyph of the icon font (`theme::glyph`).
    pub icon: Option<char>,
    /// A picture in the icon column (16 points square), in place of a glyph: a shell verb's or an
    /// app's icon.
    pub image: Option<TextureId>,
    /// A quiet right-aligned hint, such as a shortcut. Only for shortcuts that really exist.
    pub hint: String,
    pub enabled: bool,
    pub checked: bool,
    /// Destructive: drawn in the danger colour.
    pub danger: bool,
}

impl<T> Entry<T> {
    pub fn item(id: T, label: impl Into<String>) -> Self {
        Entry::Item(Item {
            id,
            label: label.into(),
            icon: None,
            image: None,
            hint: String::new(),
            enabled: true,
            checked: false,
            danger: false,
        })
    }

    pub fn submenu(label: impl Into<String>, entries: Vec<Entry<T>>) -> Self {
        Entry::Submenu { label: label.into(), icon: None, image: None, enabled: true, entries, on_open: None }
    }

    pub fn separator() -> Self {
        Entry::Separator
    }

    pub fn heading(text: impl Into<String>) -> Self {
        Entry::Heading(text.into())
    }

    pub fn note(text: impl Into<String>) -> Self {
        Entry::Note(text.into())
    }

    pub fn picture(id: T, texture: TextureId, size: Vec2) -> Self {
        Entry::Picture { id, texture, size, enabled: true }
    }

    /// With this glyph in the icon column.
    pub fn icon(mut self, glyph: char) -> Self {
        match &mut self {
            Entry::Item(i) => i.icon = Some(glyph),
            Entry::Submenu { icon, .. } => *icon = Some(glyph),
            _ => {}
        }
        self
    }

    /// With this picture in the icon column (it wins over a glyph).
    pub fn image(mut self, texture: TextureId) -> Self {
        match &mut self {
            Entry::Item(i) => i.image = Some(texture),
            Entry::Submenu { image, .. } => *image = Some(texture),
            _ => {}
        }
        self
    }

    /// With a quiet hint on the right (a shortcut that really exists).
    pub fn hint(mut self, text: impl Into<String>) -> Self {
        if let Entry::Item(i) = &mut self {
            i.hint = text.into();
        }
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        match &mut self {
            Entry::Item(i) => i.enabled = on,
            Entry::Submenu { enabled, .. } | Entry::Picture { enabled, .. } => *enabled = on,
            _ => {}
        }
        self
    }

    pub fn checked(mut self, on: bool) -> Self {
        if let Entry::Item(i) = &mut self {
            i.checked = on;
        }
        self
    }

    /// Destructive (Empty Recycle Bin, Delete): drawn in the danger colour.
    pub fn danger(mut self) -> Self {
        if let Entry::Item(i) = &mut self {
            i.danger = true;
        }
        self
    }

    /// A submenu's value handed back when it opens (see [`Entry::Submenu`]).
    pub fn on_open(mut self, value: T) -> Self {
        if let Entry::Submenu { on_open, .. } = &mut self {
            *on_open = Some(value);
        }
        self
    }
}

/// What kind of line a [`Row`] is.
#[derive(Clone, Debug, PartialEq)]
pub enum RowKind {
    /// Picks the caller's value number `usize`.
    Item(usize),
    Submenu(Vec<Row>),
    Separator,
    Heading,
    Note,
    /// Picks the caller's value number `usize`; drawn as the picture, `size` points.
    Picture(usize, TextureId, Vec2),
}

/// A line as the menu window keeps it: the caller's value replaced by its index.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub kind: RowKind,
    /// Without the `&`.
    pub label: String,
    /// The letter that picks it: from `&`, else none (first letters then only move).
    pub mnemonic: Option<char>,
    pub icon: Option<char>,
    pub image: Option<TextureId>,
    pub hint: String,
    pub enabled: bool,
    pub checked: bool,
    pub danger: bool,
    /// A submenu's value number handed back when it opens.
    pub opens: Option<usize>,
}

impl Row {
    /// Whether the keyboard and pointer can land on it.
    pub fn selectable(&self) -> bool {
        matches!(self.kind, RowKind::Item(_) | RowKind::Submenu(_) | RowKind::Picture(..))
    }

    pub fn submenu(&self) -> Option<&[Row]> {
        match &self.kind {
            RowKind::Submenu(rows) => Some(rows),
            _ => None,
        }
    }

    /// The letter typing moves to: the marked one, else the label's first.
    pub fn letter(&self) -> Option<char> {
        self.mnemonic.or_else(|| self.label.chars().next().map(|c| c.to_ascii_lowercase()))
    }

    /// Whether it shows something in the icon column.
    pub fn has_icon(&self) -> bool {
        self.icon.is_some() || self.image.is_some() || self.checked
    }
}

/// "&Paste" → ("Paste", Some('p')); "&&" is a literal "&".
pub fn parse_label(label: &str) -> (String, Option<char>) {
    let mut out = String::with_capacity(label.len());
    let mut mnemonic = None;
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            match chars.next() {
                Some('&') => out.push('&'),
                Some(n) => {
                    if mnemonic.is_none() {
                        mnemonic = Some(n.to_ascii_lowercase());
                    }
                    out.push(n);
                }
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    (out, mnemonic)
}

/// Turn the caller's entries into rows, moving each value into `ids`. `depth` 0 is the menu
/// itself; submenus deeper than [`MAX_LEVELS`] allows become disabled rows.
pub fn flatten<T>(entries: Vec<Entry<T>>, ids: &mut Vec<T>, depth: usize) -> Vec<Row> {
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        out.push(match e {
            Entry::Item(i) => {
                let (label, mnemonic) = parse_label(&i.label);
                Row {
                    kind: RowKind::Item(push(ids, i.id)),
                    label,
                    mnemonic,
                    icon: i.icon,
                    image: i.image,
                    hint: i.hint,
                    enabled: i.enabled,
                    checked: i.checked,
                    danger: i.danger,
                    opens: None,
                }
            }
            Entry::Submenu { label, icon, image, enabled, entries, on_open } => {
                let (label, mnemonic) = parse_label(&label);
                let deep = depth + 1 >= MAX_LEVELS;
                let opens = if deep { None } else { on_open.map(|v| push(ids, v)) };
                let rows = if deep { Vec::new() } else { flatten(entries, ids, depth + 1) };
                Row {
                    kind: RowKind::Submenu(rows),
                    label,
                    mnemonic,
                    icon,
                    image,
                    hint: String::new(),
                    enabled: enabled && !deep,
                    checked: false,
                    danger: false,
                    opens,
                }
            }
            Entry::Separator => plain(RowKind::Separator, String::new()),
            Entry::Heading(text) => plain(RowKind::Heading, text),
            Entry::Note(text) => plain(RowKind::Note, text),
            Entry::Picture { id, texture, size, enabled } => {
                let mut r = plain(RowKind::Picture(push(ids, id), texture, size), String::new());
                r.enabled = enabled;
                r
            }
        });
    }
    out
}

fn push<T>(ids: &mut Vec<T>, id: T) -> usize {
    ids.push(id);
    ids.len() - 1
}

fn plain(kind: RowKind, label: String) -> Row {
    Row {
        kind,
        label,
        mnemonic: None,
        icon: None,
        image: None,
        hint: String::new(),
        enabled: false,
        checked: false,
        danger: false,
        opens: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ampersands_mark_the_letter() {
        assert_eq!(parse_label("&Paste"), ("Paste".into(), Some('p')));
        assert_eq!(parse_label("Displa&y settings"), ("Display settings".into(), Some('y')));
        assert_eq!(parse_label("Tom && Jerry"), ("Tom & Jerry".into(), None));
        assert_eq!(parse_label("Plain"), ("Plain".into(), None));
        assert_eq!(parse_label("Trailing&"), ("Trailing".into(), None));
    }

    #[test]
    fn values_become_indices_in_order() {
        let entries = vec![
            Entry::submenu("&New", vec![Entry::item('f', "Folder"), Entry::item('t', "Text")]),
            Entry::separator(),
            Entry::item('p', "&Paste").enabled(false),
            Entry::heading("More"),
            Entry::note("Loading…"),
            Entry::picture('d', TextureId::Managed(1), Vec2::new(100.0, 30.0)),
        ];
        let mut ids = Vec::new();
        let rows = flatten(entries, &mut ids, 0);
        assert_eq!(ids, ['f', 't', 'p', 'd']);
        assert_eq!(rows[0].submenu().unwrap()[1].kind, RowKind::Item(1));
        assert_eq!(rows[2].kind, RowKind::Item(2));
        assert!(!rows[2].enabled);
        assert!(!rows[1].selectable() && !rows[3].selectable() && !rows[4].selectable());
        assert!(rows[5].selectable() && rows[5].enabled);
        assert_eq!(rows[0].letter(), Some('n'));
    }

    #[test]
    fn a_submenu_filled_on_demand_carries_its_value() {
        let entries = vec![Entry::submenu("Send to", vec![Entry::note("Loading…")]).on_open('s'), Entry::item('x', "X")];
        let mut ids = Vec::new();
        let rows = flatten(entries, &mut ids, 0);
        assert_eq!(ids, ['s', 'x']);
        assert_eq!(rows[0].opens, Some(0));
        assert_eq!(rows[1].kind, RowKind::Item(1));
    }

    #[test]
    fn submenus_nest_to_the_limit_then_show_disabled() {
        fn nest(n: usize) -> Entry<usize> {
            if n == 0 {
                Entry::item(99, "leaf")
            } else {
                Entry::submenu(format!("level {n}"), vec![nest(n - 1)])
            }
        }
        let mut ids = Vec::new();
        let rows = flatten(vec![nest(MAX_LEVELS)], &mut ids, 0);
        let mut level: &[Row] = &rows;
        for _ in 0..MAX_LEVELS - 1 {
            assert!(level[0].enabled);
            level = level[0].submenu().unwrap();
        }
        // The last level's submenu row is there, but greyed and empty.
        assert!(!level[0].enabled);
        assert_eq!(level[0].submenu().map(<[Row]>::len), Some(0));
        assert!(ids.is_empty());
    }
}
