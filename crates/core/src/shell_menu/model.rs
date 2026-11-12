//! The shell menu as plain data: what a walk of the Windows menu turns into, and what Files draws.
//! Nothing here touches COM, so it can be built and checked in tests from a described menu.

use std::sync::Arc;

/// A small picture: a menu icon, or a whole owner-drawn row. RGBA, premultiplied, rows top down.
#[derive(Clone, PartialEq, Eq)]
pub struct Icon {
    pub size: [usize; 2],
    pub rgba: Arc<Vec<u8>>,
    /// A hash of the pixels, so the same picture becomes one texture however often it comes back.
    pub hash: u64,
}

impl Icon {
    pub fn new(size: [usize; 2], rgba: Vec<u8>) -> Self {
        let hash = content_hash(&rgba, size);
        Self { size, rgba: Arc::new(rgba), hash }
    }
}

impl std::fmt::Debug for Icon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Icon({}x{} #{:x})", self.size[0], self.size[1], self.hash)
    }
}

/// FNV-1a over the size and the pixels: cheap, and good enough to tell icons apart.
pub fn content_hash(bytes: &[u8], size: [usize; 2]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: u8| {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    };
    for b in (size[0] as u32).to_le_bytes().into_iter().chain((size[1] as u32).to_le_bytes()) {
        eat(b);
    }
    for &b in bytes {
        eat(b);
    }
    h
}

/// Where a submenu stands. Extensions fill some submenus only when they are about to open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Cascade {
    /// Not a submenu.
    #[default]
    None,
    /// A submenu whose entries are all here.
    Filled,
    /// A submenu still being asked for its entries.
    Pending,
    /// A submenu whose handler was too slow before; it is filled when the person opens it.
    Deferred,
}

/// One entry of the shell's menu.
#[derive(Clone, Debug, Default)]
pub struct Item {
    /// The command's offset from the first id handed to the menu: what `InvokeCommand` takes.
    pub id: u32,
    /// The text, with the `&` mnemonic marks taken out.
    pub label: String,
    /// The letter the `&` marked, if any.
    pub mnemonic: Option<char>,
    /// A shortcut the label carried after a tab, like "Ctrl+X".
    pub shortcut: String,
    /// The language-independent name of the command, lower case, when the handler gives one.
    pub verb: Option<String>,
    /// The handler's help text, used to name owner-drawn entries that have no text.
    pub help: Option<String>,
    pub icon: Option<Icon>,
    /// An owner-drawn entry rendered off screen: the whole row as the extension draws it.
    pub drawn: Option<Icon>,
    /// An owner-drawn entry, whether or not it could be rendered.
    pub owner_drawn: bool,
    pub enabled: bool,
    pub checked: bool,
    pub separator: bool,
    /// The bold entry Windows would run on a double click.
    pub default: bool,
    pub cascade: Cascade,
    pub submenu: Vec<Item>,
    /// Where it sits in the shell's own menu (indices from the top), which stays put when Files
    /// leaves entries out; a deferred submenu is asked for by it.
    pub path: Vec<usize>,
}

impl Item {
    pub fn separator() -> Self {
        Self { separator: true, ..Default::default() }
    }

    /// The text to show: the label, else for an owner-drawn entry its help text or verb.
    pub fn title(&self) -> String {
        if !self.label.is_empty() {
            return self.label.clone();
        }
        self.help
            .clone()
            .filter(|h| !h.is_empty())
            .or_else(|| self.verb.clone())
            .unwrap_or_default()
    }

    /// Whether this is a submenu, filled or not.
    pub fn is_submenu(&self) -> bool {
        self.cascade != Cascade::None
    }

    /// The slow-handler key for this entry: its verb when it has one (the same in every
    /// language), else its label.
    pub fn slow_key(&self) -> String {
        match &self.verb {
            Some(v) if !v.is_empty() => format!("verb:{v}"),
            _ => format!("label:{}", self.title()),
        }
    }
}

/// A Windows menu label taken apart: "Open &with...\tCtrl+O" is "Open with...", the mnemonic
/// 'w', and the shortcut "Ctrl+O". "&&" stands for a literal "&".
pub fn parse_label(raw: &str) -> (String, Option<char>, String) {
    let (text, shortcut) = match raw.split_once('\t') {
        Some((t, s)) => (t, s.trim().to_string()),
        None => (raw, String::new()),
    };
    let mut out = String::with_capacity(text.len());
    let mut mnemonic = None;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '&' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('&') => out.push('&'),
            Some(next) => {
                mnemonic.get_or_insert(next.to_ascii_lowercase());
                out.push(next);
            }
            None => {}
        }
    }
    // East Asian labels put the mnemonic in brackets after the text, "Open(&O)": drop the
    // leftover "(O)" too, since the letter is not part of the words.
    if let Some(m) = mnemonic {
        let tail = format!("({})", m.to_ascii_uppercase());
        if out.ends_with(&tail) && out.len() > tail.len() {
            out.truncate(out.len() - tail.len());
        }
    }
    (out.trim().to_string(), mnemonic, shortcut)
}

/// Counts every entry in a tree, submenus included, separators not.
pub fn count(items: &[Item]) -> usize {
    items
        .iter()
        .filter(|i| !i.separator)
        .map(|i| 1 + count(&i.submenu))
        .sum()
}

/// The entry at `path` (indices from the top level down).
pub fn at<'a>(items: &'a [Item], path: &[usize]) -> Option<&'a Item> {
    let (&first, rest) = path.split_first()?;
    let item = items.get(first)?;
    if rest.is_empty() {
        Some(item)
    } else {
        at(&item.submenu, rest)
    }
}

/// Finds the entry in `items` that stands for `want`, an entry of an older walk of a menu like
/// it: same verb, else same label, at the same depth under matching parents. Ids differ from one
/// walk to the next, so a pick made on a cached menu is matched to the fresh one this way.
pub fn find_like(items: &[Item], want: &[&Item]) -> Option<Vec<usize>> {
    let (first, rest) = want.split_first()?;
    let pos = items.iter().position(|i| {
        !i.separator
            && match (&i.verb, &first.verb) {
                (Some(a), Some(b)) => a == b,
                _ => i.title() == first.title(),
            }
    })?;
    let mut path = vec![pos];
    if !rest.is_empty() {
        path.extend(find_like(&items[pos].submenu, rest)?);
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_lose_their_marks() {
        assert_eq!(parse_label("Open &with"), ("Open with".into(), Some('w'), String::new()));
        assert_eq!(parse_label("Tom && Jerry"), ("Tom & Jerry".into(), None, String::new()));
        assert_eq!(
            parse_label("Cu&t\tCtrl+X"),
            ("Cut".into(), Some('t'), "Ctrl+X".into())
        );
        assert_eq!(parse_label("開く(&O)"), ("開く".into(), Some('o'), String::new()));
        assert_eq!(parse_label("Trailing&"), ("Trailing".into(), None, String::new()));
        assert_eq!(parse_label("&A && &B"), ("A & B".into(), Some('a'), String::new()));
    }

    #[test]
    fn owner_drawn_entries_are_named_by_help_then_verb() {
        let mut i = Item { help: Some("Scan with Defender".into()), verb: Some("scan".into()), ..Default::default() };
        assert_eq!(i.title(), "Scan with Defender");
        i.help = None;
        assert_eq!(i.title(), "scan");
        assert_eq!(i.slow_key(), "verb:scan");
        i.verb = None;
        i.label = "Send to".into();
        assert_eq!(i.slow_key(), "label:Send to");
    }

    #[test]
    fn same_pixels_same_hash() {
        let a = Icon::new([1, 1], vec![1, 2, 3, 4]);
        let b = Icon::new([1, 1], vec![1, 2, 3, 4]);
        let c = Icon::new([1, 1], vec![1, 2, 3, 5]);
        assert_eq!(a.hash, b.hash);
        assert_ne!(a.hash, c.hash);
        // The size is part of it: the same bytes in another shape are another picture.
        assert_ne!(Icon::new([2, 1], vec![0; 8]).hash, Icon::new([1, 2], vec![0; 8]).hash);
    }

    #[test]
    fn a_pick_on_an_old_menu_finds_its_entry_in_a_new_one() {
        let leaf = |label: &str, verb: Option<&str>, id| Item {
            label: label.into(),
            verb: verb.map(Into::into),
            id,
            ..Default::default()
        };
        let mut send = leaf("Send to", None, 0);
        send.cascade = Cascade::Filled;
        send.submenu = vec![leaf("Desktop", None, 7), leaf("Mail", None, 8)];
        let fresh = vec![leaf("7-Zip", Some("7zip"), 3), Item::separator(), send.clone()];
        let old_send = Item { id: 99, ..send.clone() };
        let old_mail = leaf("Mail", None, 42);
        assert_eq!(find_like(&fresh, &[&old_send, &old_mail]), Some(vec![2, 1]));
        assert_eq!(at(&fresh, &[2, 1]).map(|i| i.id), Some(8));
        assert_eq!(find_like(&fresh, &[&leaf("x", Some("7zip"), 0)]), Some(vec![0]));
        assert_eq!(find_like(&fresh, &[&leaf("Gone", None, 0)]), None);
        assert_eq!(count(&fresh), 4);
    }
}
