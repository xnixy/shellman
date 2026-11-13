//! Selecting text with the mouse and what is copied, on `alacritty_terminal`'s selection model
//! (which keeps a selection on its text as new output scrolls it, and joins soft-wrapped lines),
//! plus finding the link under the pointer: an OSC 8 hyperlink, or a plain URL in the text.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::Term;

/// What ends a word for a double-click. Not `:`, `/`, `\`, `.`, `-` or `~`, so a whole path
/// (`C:\Users\me\file.txt`, `src/main.rs:12:5`) or URL is one word.
pub const WORD_STOPS: &str = " \t\"'`|│()[]{}<>,;";

/// How a press starts a selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Cell by cell (a drag).
    Cells,
    /// Whole words (a double-click).
    Words,
    /// Whole lines (a triple-click).
    Lines,
    /// A rectangle (Alt and a drag).
    Block,
}

/// Start a selection at `point`.
pub fn start<T>(term: &mut Term<T>, kind: Kind, point: Point, side: Side) {
    let ty = match kind {
        Kind::Cells => SelectionType::Simple,
        Kind::Words => SelectionType::Semantic,
        Kind::Lines => SelectionType::Lines,
        Kind::Block => SelectionType::Block,
    };
    term.selection = Some(Selection::new(ty, point, side));
}

/// Move the selection's free end to `point` (a drag, or Shift and a click).
pub fn extend<T>(term: &mut Term<T>, point: Point, side: Side) {
    if let Some(sel) = term.selection.as_mut() {
        sel.update(point, side);
    }
}

/// Whether there is a selection with something in it.
pub fn has<T>(term: &Term<T>) -> bool {
    term.selection.as_ref().is_some_and(|s| !s.is_empty())
}

/// The selected text as it goes on the clipboard: soft-wrapped lines joined (the emulator does
/// that), blanks at the end of each line left off, Windows line ends.
pub fn text<T>(term: &Term<T>) -> Option<String> {
    if !has(term) {
        return None;
    }
    let raw = term.selection_to_string()?;
    let mut text = clipboard_text(&raw);
    // Whole lines end with a line break; pasted back, it would run the line at once.
    if term.selection.as_ref().is_some_and(|s| s.ty == SelectionType::Lines) {
        text.truncate(text.trim_end_matches(['\r', '\n']).len());
    }
    Some(text)
}

/// Trim each line's trailing blanks and use CR LF between lines.
pub fn clipboard_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 16);
    let mut lines = raw.split('\n').peekable();
    while let Some(line) = lines.next() {
        out.push_str(line.trim_end_matches([' ', '\t', '\r']));
        if lines.peek().is_some() {
            out.push_str("\r\n");
        }
    }
    out
}

/// The grid point at a viewport cell (row from the screen's top).
pub fn point_at<T>(term: &Term<T>, col: usize, row: usize) -> Point {
    let offset = term.grid().display_offset() as i32;
    Point::new(Line(row as i32 - offset), Column(col.min(term.columns().saturating_sub(1))))
}

/// The OSC 8 link at `point`: its id and URI.
pub fn hyperlink_at<T>(term: &Term<T>, point: Point) -> Option<(String, String)> {
    let cell = &term.grid()[point];
    let link = cell.hyperlink()?;
    Some((link.id().to_owned(), link.uri().to_owned()))
}

/// A URL written in the text at `point`, across soft-wrapped lines: its first and last cells and
/// the URL.
pub fn url_at<T>(term: &Term<T>, point: Point) -> Option<(Point, Point, String)> {
    let grid = term.grid();
    let cols = term.columns();
    let top = Line(-(grid.history_size() as i32));
    let bottom = Line(term.screen_lines() as i32 - 1);
    let wraps = |line: Line| grid[line][Column(cols - 1)].flags.contains(Flags::WRAPLINE);
    // The logical line: up while the line above wraps into this one, down while this one wraps.
    let mut first = point.line;
    while first > top && wraps(first - 1) && point.line.0 - first.0 < 32 {
        first -= 1;
    }
    let mut last = point.line;
    while last < bottom && wraps(last) && last.0 - point.line.0 < 32 {
        last += 1;
    }
    let mut chars = Vec::new();
    let mut points = Vec::new();
    let mut at = None;
    let mut line = first;
    loop {
        for c in 0..cols {
            let cell = &grid[line][Column(c)];
            if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            let p = Point::new(line, Column(c));
            if p == point || (at.is_none() && line == point.line && c > point.column.0) {
                at.get_or_insert(chars.len());
            }
            chars.push(cell.c);
            points.push(p);
        }
        if line == last {
            break;
        }
        line += 1;
    }
    let (s, e) = url_span(&chars, at?)?;
    Some((points[s], points[e - 1], chars[s..e].iter().collect()))
}

const SCHEMES: [&str; 7] = ["https://", "http://", "file://", "ftp://", "ssh://", "git://", "mailto:"];

/// The URL in `line` covering index `at`, as `[start, end)`.
pub fn url_span(line: &[char], at: usize) -> Option<(usize, usize)> {
    let allowed = |c: char| !c.is_whitespace() && !c.is_control() && !matches!(c, '<' | '>' | '"' | '`' | '{' | '}' | '|' | '^' | '\\' | '\'');
    // The run of URL characters around `at`.
    let mut s = at;
    while s > 0 && allowed(line[s - 1]) {
        s -= 1;
    }
    let mut e = at;
    while e < line.len() && allowed(line[e]) {
        e += 1;
    }
    if at >= e {
        return None;
    }
    // A scheme somewhere in the run before `at`: the URL starts there.
    let run: String = line[s..e].iter().collect();
    let byte_at = line[s..at].iter().map(|c| c.len_utf8()).sum::<usize>();
    let start_byte = SCHEMES
        .iter()
        .flat_map(|scheme| run.match_indices(scheme).map(|(k, _)| k))
        .filter(|&k| k <= byte_at)
        .max()?;
    let start = s + run[..start_byte].chars().count();
    let mut end = e;
    // Punctuation that ends a sentence, and closing brackets with no opening one in the URL.
    loop {
        let Some(&c) = line[start..end].last() else { return None };
        let unbalanced = |open: char, close: char| {
            c == close && line[start..end].iter().filter(|&&x| x == open).count() < line[start..end].iter().filter(|&&x| x == close).count()
        };
        if matches!(c, '.' | ',' | ';' | ':' | '!' | '?') || unbalanced('(', ')') || unbalanced('[', ']') {
            end -= 1;
        } else {
            break;
        }
    }
    let url: String = line[start..end].iter().collect();
    let scheme_len = SCHEMES.iter().find(|s| url.starts_with(**s))?.len();
    (url.len() > scheme_len && at < end).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;

    struct Size(usize, usize);
    impl Dimensions for Size {
        fn total_lines(&self) -> usize {
            self.1
        }
        fn screen_lines(&self) -> usize {
            self.1
        }
        fn columns(&self) -> usize {
            self.0
        }
    }

    fn term(cols: usize, rows: usize, text: &str) -> Term<VoidListener> {
        let config = Config { semantic_escape_chars: WORD_STOPS.into(), ..Config::default() };
        let mut t = Term::new(config, &Size(cols, rows), VoidListener);
        let mut p: Processor = Processor::new();
        p.advance(&mut t, text.replace('\n', "\r\n").as_bytes());
        t
    }

    fn at(line: i32, col: usize) -> Point {
        Point::new(Line(line), Column(col))
    }

    fn select(t: &mut Term<VoidListener>, kind: Kind, from: Point, to: Point) -> String {
        start(t, kind, from, Side::Left);
        extend(t, to, Side::Right);
        text(t).unwrap_or_default()
    }

    #[test]
    fn cells_across_lines_trim_trailing_blanks_and_use_crlf() {
        let mut t = term(20, 4, "hello   \nworld");
        assert_eq!(select(&mut t, Kind::Cells, at(0, 0), at(1, 19)), "hello\r\nworld");
        assert_eq!(select(&mut t, Kind::Cells, at(0, 1), at(0, 3)), "ell");
    }

    #[test]
    fn soft_wrapped_lines_join() {
        // 25 characters in 10 columns: one logical line over three rows.
        let mut t = term(10, 4, "abcdefghijklmnopqrstuvwxy\nnext");
        assert_eq!(select(&mut t, Kind::Cells, at(0, 0), at(3, 9)), "abcdefghijklmnopqrstuvwxy\r\nnext");
        // A whole line by triple-click: the logical one.
        assert_eq!(select(&mut t, Kind::Lines, at(1, 3), at(1, 3)), "abcdefghijklmnopqrstuvwxy");
        assert_eq!(select(&mut t, Kind::Lines, at(1, 3), at(3, 0)), "abcdefghijklmnopqrstuvwxy\r\nnext");
    }

    #[test]
    fn words_take_paths_and_urls_whole() {
        let mut t = term(60, 2, r#"open C:\Users\me\file.txt, "https://x.org/a?b=1" src/a.rs:12"#);
        assert_eq!(select(&mut t, Kind::Words, at(0, 8), at(0, 8)), r"C:\Users\me\file.txt");
        assert_eq!(select(&mut t, Kind::Words, at(0, 30), at(0, 30)), "https://x.org/a?b=1");
        assert_eq!(select(&mut t, Kind::Words, at(0, 52), at(0, 52)), "src/a.rs:12");
        assert_eq!(select(&mut t, Kind::Words, at(0, 1), at(0, 1)), "open");
    }

    #[test]
    fn block_selection_is_a_rectangle() {
        let mut t = term(10, 3, "abcdef\nghijkl\nmnopqr");
        assert_eq!(select(&mut t, Kind::Block, at(0, 1), at(2, 2)), "bc\r\nhi\r\nno");
    }

    #[test]
    fn wide_characters_copy_once() {
        let mut t = term(10, 2, "a漢字b");
        assert_eq!(select(&mut t, Kind::Cells, at(0, 0), at(0, 9)), "a漢字b");
    }

    #[test]
    fn a_selection_follows_its_text_as_output_scrolls() {
        let mut t = term(10, 3, "one\ntwo\nthree");
        start(&mut t, Kind::Words, at(0, 0), Side::Left);
        let mut p: Processor = Processor::new();
        p.advance(&mut t, b"\r\nfour\r\nfive");
        assert_eq!(text(&t).as_deref(), Some("one"));
    }

    #[test]
    fn no_selection_no_text() {
        let mut t = term(10, 2, "abc");
        assert_eq!(text(&t), None);
        start(&mut t, Kind::Cells, at(0, 1), Side::Left);
        assert_eq!(text(&t), None, "a press without a drag selects nothing");
    }

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn url(s: &str, at: usize) -> Option<String> {
        let c = chars(s);
        url_span(&c, at).map(|(a, b)| c[a..b].iter().collect())
    }

    #[test]
    fn urls_in_text() {
        let s = "see https://example.org/a_(b)?q=1. Then (http://x.io/y) end";
        assert_eq!(url(s, 10).as_deref(), Some("https://example.org/a_(b)?q=1"));
        assert_eq!(url(s, 4).as_deref(), Some("https://example.org/a_(b)?q=1"), "on the scheme");
        assert_eq!(url(s, 45).as_deref(), Some("http://x.io/y"));
        assert_eq!(url(s, 1), None);
        assert_eq!(url("https:// nothing", 3), None);
        assert_eq!(url("mailto:me@x.org", 9).as_deref(), Some("mailto:me@x.org"));
        assert_eq!(url("<https://a.b/c>", 5).as_deref(), Some("https://a.b/c"));
    }

    #[test]
    fn a_url_across_wrapped_rows() {
        let t = term(12, 4, "go https://example.org/abc ok");
        let (s, e, u) = url_at(&t, at(1, 3)).unwrap();
        assert_eq!(u, "https://example.org/abc");
        assert_eq!((s, e), (at(0, 3), at(2, 1)));
        assert!(url_at(&t, at(2, 4)).is_none());
    }
}
