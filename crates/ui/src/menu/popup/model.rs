//! What a popup menu shows, as the caller builds it ([`Entry`]), and the same without the
//! caller's ids ([`Row`]), which is what the one window per process keeps.

/// One line of a popup menu, carrying `T`, the value handed back when it is picked.
///
/// A label may mark its keyboard letter with `&` ("&Paste"), as Windows menus do; without one,
/// typing a row's first letter moves to it (and picks it when it is the only such row).
#[derive(Clone, Debug)]
pub enum Entry<T> {
    Item(Item<T>),
    /// A row that opens a submenu. One level: a submenu's own submenus show as disabled rows.
    Submenu { label: String, icon: Option<char>, enabled: bool, entries: Vec<Entry<T>> },
    Separator,
    /// A quiet line above a group, never picked.
    Heading(String),
}

/// A row that can be picked.
#[derive(Clone, Debug)]
pub struct Item<T> {
    pub id: T,
    pub label: String,
    /// A glyph of the icon font (`theme::glyph`).
    pub icon: Option<char>,
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
            hint: String::new(),
            enabled: true,
            checked: false,
            danger: false,
        })
    }

    pub fn submenu(label: impl Into<String>, entries: Vec<Entry<T>>) -> Self {
        Entry::Submenu { label: label.into(), icon: None, enabled: true, entries }
    }

    pub fn separator() -> Self {
        Entry::Separator
    }

    pub fn heading(text: impl Into<String>) -> Self {
        Entry::Heading(text.into())
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
            Entry::Submenu { enabled, .. } => *enabled = on,
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
}

/// What kind of line a [`Row`] is.
#[derive(Clone, Debug, PartialEq)]
pub enum RowKind {
    /// Picks the caller's value number `usize`.
    Item(usize),
    Submenu(Vec<Row>),
    Separator,
    Heading,
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
    pub hint: String,
    pub enabled: bool,
    pub checked: bool,
    pub danger: bool,
}

impl Row {
    /// Whether the keyboard and pointer can land on it.
    pub fn selectable(&self) -> bool {
        matches!(self.kind, RowKind::Item(_) | RowKind::Submenu(_))
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
/// itself; deeper submenus than one level become disabled rows.
pub fn flatten<T>(entries: Vec<Entry<T>>, ids: &mut Vec<T>, depth: usize) -> Vec<Row> {
    entries
        .into_iter()
        .map(|e| match e {
            Entry::Item(i) => {
                let (label, mnemonic) = parse_label(&i.label);
                ids.push(i.id);
                Row {
                    kind: RowKind::Item(ids.len() - 1),
                    label,
                    mnemonic,
                    icon: i.icon,
                    hint: i.hint,
                    enabled: i.enabled,
                    checked: i.checked,
                    danger: i.danger,
                }
            }
            Entry::Submenu { label, icon, enabled, entries } => {
                let (label, mnemonic) = parse_label(&label);
                let deep = depth >= 1;
                let rows = if deep { Vec::new() } else { flatten(entries, ids, depth + 1) };
                Row {
                    kind: RowKind::Submenu(rows),
                    label,
                    mnemonic,
                    icon,
                    hint: String::new(),
                    enabled: enabled && !deep,
                    checked: false,
                    danger: false,
                }
            }
            Entry::Separator => plain(RowKind::Separator, String::new()),
            Entry::Heading(text) => plain(RowKind::Heading, text),
        })
        .collect()
}

fn plain(kind: RowKind, label: String) -> Row {
    Row { kind, label, mnemonic: None, icon: None, hint: String::new(), enabled: false, checked: false, danger: false }
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
        ];
        let mut ids = Vec::new();
        let rows = flatten(entries, &mut ids, 0);
        assert_eq!(ids, ['f', 't', 'p']);
        assert_eq!(rows[0].submenu().unwrap()[1].kind, RowKind::Item(1));
        assert_eq!(rows[2].kind, RowKind::Item(2));
        assert!(!rows[2].enabled);
        assert!(!rows[1].selectable() && !rows[3].selectable());
        assert_eq!(rows[0].letter(), Some('n'));
    }

    #[test]
    fn a_second_level_of_submenus_is_a_disabled_row() {
        let inner = Entry::submenu("Deep", vec![Entry::item(1, "x")]);
        let mut ids = Vec::new();
        let rows = flatten(vec![Entry::submenu("Top", vec![inner])], &mut ids, 0);
        let deep = &rows[0].submenu().unwrap()[0];
        assert!(!deep.enabled);
        assert_eq!(deep.submenu().map(<[Row]>::len), Some(0));
        assert!(ids.is_empty());
    }
}
