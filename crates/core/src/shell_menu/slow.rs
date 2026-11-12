//! Remembering slow menu handlers, and which menus look alike.
//!
//! Each submenu a handler fills is timed. One that was slow is filled last next time; one that
//! took longer than the whole wait is not filled at all until the person opens it, so it cannot
//! hold up the rest again. A handler that turns fast again is forgotten.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

/// Slower than this, a submenu is filled after the others.
pub const SLOW: Duration = Duration::from_millis(250);
/// Slower than this, the menu stops waiting for it: it is filled on demand from then on.
pub const TOO_SLOW: Duration = Duration::from_millis(1500);

/// How a handler is treated on the next walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Treat {
    Normal,
    Last,
    Skip,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
struct Record {
    /// The longest it took, in milliseconds.
    worst_ms: u64,
}

/// Slow handlers, keyed by their entry's verb or label (`Item::slow_key`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SlowBook {
    handlers: BTreeMap<String, Record>,
}

/// The book as one walk reads it.
pub type Plan = SlowBook;

impl SlowBook {
    pub fn load(path: &Path) -> Self {
        crate::config::load_from(path)
    }

    pub fn save(&self, path: &Path) {
        crate::config::save_to(path, self)
    }

    /// Notes how long `key` took. A fast run clears what was known against it.
    /// True when the book changed.
    pub fn record(&mut self, key: &str, took: Duration) -> bool {
        if took < SLOW {
            return self.handlers.remove(key).is_some();
        }
        let ms = took.as_millis() as u64;
        let rec = self.handlers.entry(key.to_string()).or_default();
        if ms > rec.worst_ms {
            rec.worst_ms = ms;
            return true;
        }
        false
    }

    pub fn of(&self, key: &str) -> Treat {
        match self.handlers.get(key) {
            None => Treat::Normal,
            Some(r) if r.worst_ms >= TOO_SLOW.as_millis() as u64 => Treat::Skip,
            Some(_) => Treat::Last,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

/// What a menu's static entries depend on: the kind of thing it is for. Menus with the same key
/// look alike, so the last one can stand in while the real one loads. `kinds` are the items' file
/// extensions (lower case, "<folder>" for a folder); empty for a folder's background.
pub fn type_key(kinds: &[String], extended: bool) -> String {
    let what = match kinds {
        [] => "<background>".to_string(),
        [first, rest @ ..] if rest.iter().all(|k| k == first) => first.clone(),
        _ => "<mixed>".to_string(),
    };
    let many = if kinds.len() > 1 { "many" } else { "one" };
    let shift = if extended { "+shift" } else { "" };
    format!("{what}|{many}{shift}")
}

/// The kind of one item, for `type_key`.
pub fn kind_of(path: &Path, is_dir: bool) -> String {
    if is_dir {
        return "<folder>".into();
    }
    match path.extension() {
        Some(ext) => format!(".{}", ext.to_string_lossy().to_lowercase()),
        None => "<none>".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_handlers_go_last_then_get_skipped_then_forgiven() {
        let mut book = SlowBook::default();
        assert_eq!(book.of("verb:x"), Treat::Normal);
        assert!(!book.record("verb:x", Duration::from_millis(20)));
        assert_eq!(book.of("verb:x"), Treat::Normal);
        assert!(book.record("verb:x", Duration::from_millis(400)));
        assert_eq!(book.of("verb:x"), Treat::Last);
        // A faster slow run keeps the worst.
        assert!(!book.record("verb:x", Duration::from_millis(300)));
        assert!(book.record("verb:x", Duration::from_secs(3)));
        assert_eq!(book.of("verb:x"), Treat::Skip);
        assert!(book.record("verb:x", Duration::from_millis(10)));
        assert_eq!(book.of("verb:x"), Treat::Normal);
        assert!(book.is_empty());
    }

    #[test]
    fn the_book_survives_a_round_trip() {
        let dir = std::env::temp_dir().join(format!("shellman-slow-{}", std::process::id()));
        let file = dir.join("slow.json");
        let mut book = SlowBook::default();
        book.record("label:Send to", Duration::from_secs(2));
        book.save(&file);
        assert_eq!(SlowBook::load(&file), book);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn menus_for_the_same_kind_of_thing_share_a_key() {
        let k = |v: &[&str], x| type_key(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>(), x);
        assert_eq!(k(&[".txt"], false), ".txt|one");
        assert_eq!(k(&[".txt", ".txt"], false), ".txt|many");
        assert_eq!(k(&[".txt", ".png"], false), "<mixed>|many");
        assert_eq!(k(&[], false), "<background>|one");
        assert_eq!(k(&["<folder>"], true), "<folder>|one+shift");
        assert_eq!(kind_of(Path::new("A.TXT"), false), ".txt");
        assert_eq!(kind_of(Path::new("Makefile"), false), "<none>");
        assert_eq!(kind_of(Path::new("dir.d"), true), "<folder>");
    }
}
