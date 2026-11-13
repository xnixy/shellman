//! The emulator's visible grid turned into the renderer's cells: colours resolved through the
//! palette (and any a program set with OSC 4/10/11), inverse and faint applied, wide characters
//! and the cursor marked. Taken under the emulator's lock, which it holds only for the copy.

use crate::palette::{dim, Palette, Rgb};
use crate::render::{self, Cell, Cursor, CursorShape, Frame};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::{self, Color, NamedColor};

/// How the cursor looks: the theme's cursor colour, and whether the window has the focus.
pub struct Look {
    pub palette: Palette,
    pub cursor: Rgb,
    /// Text on the block cursor.
    pub on_cursor: Rgb,
    pub focused: bool,
    /// The selection's colour (the accent), laid over the cells it covers.
    pub selection: Rgb,
    /// The blinking cursor's phase: false while it is off.
    pub cursor_on: bool,
    /// The link under the pointer, underlined.
    pub hover: Hover,
    /// Find's matches on screen, and its current one.
    pub find: Option<Highlight>,
}

/// What find marks: the matches on screen, and the current one (drawn apart).
#[derive(Clone, Default, Debug)]
pub struct Highlight {
    pub matches: Vec<std::ops::RangeInclusive<Point>>,
    pub current: Option<std::ops::RangeInclusive<Point>>,
}

/// Find's match colours: a warm marker for every match, the accent for the current one.
const MATCH_BG: [u8; 4] = [244, 196, 64, 255];
const MATCH_FG: [u8; 4] = [24, 22, 16, 255];

/// A link the pointer is on.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum Hover {
    #[default]
    None,
    /// An OSC 8 hyperlink: every cell with this id.
    Link(String),
    /// A URL in the text, from its first cell to its last.
    Span(Point, Point),
}

/// How strongly the selection's colour shows over glass, and over a cell's own background.
const SELECTION_ALPHA: u8 = 100;
const SELECTION_MIX: u16 = 110;

/// `base` and the combining `marks` after it as one character, through Unicode's composition
/// (NFC, by Windows); `base` when they do not make one.
pub fn compose(base: char, marks: &[char]) -> char {
    use windows::Win32::Globalization::{NormalizeString, NormalizationC};
    let mut s: Vec<u16> = Vec::with_capacity(8);
    let mut buf = [0u16; 2];
    for c in std::iter::once(base).chain(marks.iter().copied()) {
        s.extend_from_slice(c.encode_utf16(&mut buf));
    }
    let mut out = [0u16; 16];
    let n = unsafe { NormalizeString(NormalizationC, &s, Some(&mut out)) };
    if n <= 0 {
        return base;
    }
    let mut chars = char::decode_utf16(out[..n as usize].iter().copied());
    match (chars.next(), chars.next()) {
        (Some(Ok(c)), None) => c,
        _ => base,
    }
}

pub fn take<T>(term: &Term<T>, look: &Look, frame: &mut Frame) {
    let cols = term.columns();
    let rows = term.screen_lines();
    let grid = term.grid();
    let offset = grid.display_offset() as i32;
    frame.cols = cols;
    frame.rows = rows;
    frame.cells.clear();
    frame.cells.reserve(cols * rows);
    let colors = term.colors();
    // Bold is drawn with the bold face, not brightened (as Windows Terminal does by default).
    // `ground`: the colour is a background, or text on one (the palette's Campbell set), not text
    // on the glass (the theme's set).
    let resolve = |c: Color, ground: bool| -> Rgb {
        let rgb = |v: ansi::Rgb| [v.r, v.g, v.b];
        match c {
            Color::Spec(v) => rgb(v),
            Color::Indexed(i) => colors[i as usize].map(rgb).unwrap_or_else(|| if ground { look.palette.indexed_ground(i) } else { look.palette.indexed(i) }),
            Color::Named(n) => {
                let set = colors[n].map(rgb);
                set.unwrap_or_else(|| match n {
                    NamedColor::Foreground | NamedColor::BrightForeground => look.palette.fg,
                    NamedColor::Background => look.palette.bg,
                    NamedColor::Cursor => look.cursor,
                    NamedColor::DimForeground => dim(look.palette.fg),
                    n if (n as usize) < 16 && ground => look.palette.ground[n as usize],
                    n if (n as usize) < 16 => look.palette.ansi[n as usize],
                    // The dim colours (259..=266) are the eight base ones, faint.
                    n => dim(look.palette.ansi[(n as usize - NamedColor::DimBlack as usize) & 7]),
                })
            }
        }
    };
    let selection = term.selection.as_ref().and_then(|s| s.to_range(term));
    let s = look.selection;
    for line in 0..rows {
        let grid_line = Line(line as i32 - offset);
        let row = &grid[grid_line];
        // Only rows the selection reaches look at it cell by cell.
        let sel = selection.filter(|r| r.start.line <= grid_line && grid_line <= r.end.line);
        // Find's matches on this row, as column spans (a match can wrap onto the next row).
        let span = |m: &std::ops::RangeInclusive<Point>| -> Option<(usize, usize)> {
            if m.start().line > grid_line || m.end().line < grid_line {
                return None;
            }
            let from = if m.start().line == grid_line { m.start().column.0 } else { 0 };
            let to = if m.end().line == grid_line { m.end().column.0 } else { cols.saturating_sub(1) };
            Some((from, to))
        };
        // Columns a joined emoji before them took over: drawn blank.
        let mut taken_until = 0usize;
        let (found, current): (Vec<(usize, usize)>, Option<(usize, usize)>) = match &look.find {
            Some(h) => (h.matches.iter().filter_map(span).collect(), h.current.as_ref().and_then(span)),
            None => (Vec::new(), None),
        };
        for col in 0..cols {
            let cell = &row[Column(col)];
            let f = cell.flags;
            let default_bg = cell.bg == Color::Named(NamedColor::Background) && colors[NamedColor::Background].is_none();
            // Text on a coloured background (or inverse, which makes its colour the background)
            // takes the ground set: white on red stays white in the light theme.
            let mut fg = resolve(cell.fg, !default_bg || f.contains(Flags::INVERSE));
            if f.contains(Flags::DIM) {
                fg = dim(fg);
            }
            let bg = resolve(cell.bg, true);
            let (mut fg, mut bg) = ([fg[0], fg[1], fg[2], 255], [bg[0], bg[1], bg[2], if default_bg { 0 } else { 255 }]);
            if f.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
                fg[3] = 255;
                bg[3] = 255;
            }
            if current.is_some_and(|(a, b)| a <= col && col <= b) {
                let a = look.selection;
                bg = [a[0], a[1], a[2], 255];
                fg = [look.on_cursor[0], look.on_cursor[1], look.on_cursor[2], 255];
            } else if found.iter().any(|&(a, b)| a <= col && col <= b) {
                bg = MATCH_BG;
                fg = MATCH_FG;
            }
            if let Some(r) = &sel {
                if r.contains(Point::new(grid_line, Column(col))) {
                    bg = if bg[3] == 0 {
                        [s[0], s[1], s[2], SELECTION_ALPHA]
                    } else {
                        let m = |a: u8, b: u8| ((a as u16 * (256 - SELECTION_MIX) + b as u16 * SELECTION_MIX) >> 8) as u8;
                        [m(bg[0], s[0]), m(bg[1], s[1]), m(bg[2], s[2]), 255]
                    };
                }
            }
            let mut flags = 0;
            for (from, to) in [
                (Flags::BOLD, render::BOLD),
                (Flags::ITALIC, render::ITALIC),
                (Flags::UNDERLINE | Flags::UNDERCURL | Flags::DOTTED_UNDERLINE | Flags::DASHED_UNDERLINE, render::UNDERLINE),
                (Flags::DOUBLE_UNDERLINE, render::DOUBLE_UNDERLINE),
                (Flags::STRIKEOUT, render::STRIKE),
                (Flags::WIDE_CHAR, render::WIDE),
            ] {
                if f.intersects(from) {
                    flags |= to;
                }
            }
            let hidden = f.intersects(Flags::HIDDEN | Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
            let mut ch = if hidden { ' ' } else { cell.c };
            if col < taken_until {
                ch = ' ';
            } else if !hidden && crate::emoji::candidate(cell) {
                // A colour emoji (with what joins it): one glyph from the colour page.
                if let Some((text, taken)) = crate::emoji::in_row(row, col, cols) {
                    if let Some(id) = frame.clusters.intern(text) {
                        ch = id;
                        flags |= render::COLOR;
                        taken_until = col + if f.contains(Flags::WIDE_CHAR) { 2 } else { 1 } + taken;
                    }
                }
            }
            // Combining marks after a character: drawn as the one precomposed character they
            // make, where there is one (e and U+0301 as é).
            if let Some(marks) = cell.zerowidth().filter(|_| !hidden && flags & render::COLOR == 0) {
                ch = compose(ch, marks);
            }
            // A wide character's spacer keeps its background run going but draws nothing.
            if f.contains(Flags::WIDE_CHAR_SPACER) {
                flags = 0;
            }
            let hovered = match &look.hover {
                Hover::None => false,
                Hover::Link(id) => cell.hyperlink().is_some_and(|h| h.id() == id),
                Hover::Span(a, b) => {
                    let p = Point::new(grid_line, Column(col));
                    *a <= p && p <= *b
                }
            };
            if hovered {
                flags |= render::UNDERLINE;
            }
            frame.cells.push(Cell { ch, fg, bg, flags });
        }
    }

    // The cursor, only where it is on screen (not scrolled away) and shown.
    frame.cursor = None;
    let point = grid.cursor.point;
    let style = term.cursor_style();
    let visible = term.mode().contains(TermMode::SHOW_CURSOR);
    // Blinking only matters while focused: unfocused, the cursor is a steady outline.
    frame.blinking = visible && style.blinking && look.focused;
    let visible = visible && (look.cursor_on || !frame.blinking);
    let line = point.line.0 + offset;
    if visible && line >= 0 && (line as usize) < rows {
        let (row, mut col) = (line as usize, point.column.0.min(cols.saturating_sub(1)));
        if col > 0 && grid[point.line][Column(col)].flags.contains(Flags::WIDE_CHAR_SPACER) {
            col -= 1;
        }
        let i = row * cols + col;
        let wide = frame.cells[i].flags & render::WIDE != 0;
        let shape = match style.shape {
            ansi::CursorShape::Hidden => return,
            _ if !look.focused => Some(CursorShape::Hollow),
            ansi::CursorShape::Beam => Some(CursorShape::Beam),
            ansi::CursorShape::Underline => Some(CursorShape::Underline),
            ansi::CursorShape::HollowBlock => Some(CursorShape::Hollow),
            ansi::CursorShape::Block => None,
        };
        let c = look.cursor;
        match shape {
            Some(shape) => frame.cursor = Some(Cursor { col, row, shape, color: [c[0], c[1], c[2], 255], wide }),
            None => {
                // A block cursor is its cell, drawn in the cursor's colours.
                let cells = if wide && col + 1 < cols { 2 } else { 1 };
                for k in 0..cells {
                    let cell = &mut frame.cells[i + k];
                    cell.bg = [c[0], c[1], c[2], 255];
                    cell.fg = [look.on_cursor[0], look.on_cursor[1], look.on_cursor[2], 255];
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::{test::TermSize, Config};

    fn snap(text: &str, light: bool) -> Frame {
        let mut t = Term::new(Config::default(), &TermSize::new(20, 2), VoidListener);
        let mut p: ansi::Processor = ansi::Processor::new();
        p.advance(&mut t, text.as_bytes());
        let palette = Palette::new(light, [20, 20, 20], [240, 240, 240]);
        let look = Look {
            palette,
            cursor: [0; 3],
            on_cursor: [0; 3],
            focused: false,
            selection: [0; 3],
            cursor_on: false,
            hover: Hover::None,
            find: None,
        };
        let mut frame = Frame { cols: 0, rows: 0, cells: Vec::new(), cursor: None, blinking: false, clusters: Default::default() };
        take(&t, &look, &mut frame);
        frame
    }

    #[test]
    fn white_on_a_colour_stays_white_in_the_light_theme() {
        // Bright white on red, white on blue: Campbell's white, not the light theme's grey.
        let f = snap("\x1b[97;41mA\x1b[37;44mB\x1b[0m\x1b[97mC", true);
        assert_eq!(&f.cells[0].fg[..3], &[242, 242, 242]);
        assert_eq!(&f.cells[0].bg[..3], &[197, 15, 31]);
        assert_eq!(&f.cells[1].fg[..3], &[204, 204, 204]);
        // On the glass, "white" is the light theme's dark grey, so it still shows.
        assert_eq!(&f.cells[2].fg[..3], &[60, 60, 60]);
        assert_eq!(f.cells[2].bg[3], 0);
    }

    #[test]
    fn dark_text_on_the_glass_is_lifted_but_backgrounds_are_campbell() {
        let f = snap("\x1b[34mA\x1b[44mB", false);
        assert_eq!(&f.cells[0].fg[..3], &[52, 100, 230]);
        assert_eq!(&f.cells[1].bg[..3], &[0, 55, 218]);
    }

    #[test]
    fn inverse_takes_the_ground_colours() {
        let f = snap("\x1b[7;31mA", true);
        assert_eq!(&f.cells[0].bg[..3], &[197, 15, 31]);
    }

    #[test]
    fn combining_marks_compose_where_they_can() {
        assert_eq!(compose('e', &['\u{301}']), '\u{e9}');
        assert_eq!(compose('a', &['\u{30A}']), '\u{e5}');
        // No precomposed form: the base is kept.
        assert_eq!(compose('x', &['\u{301}']), 'x');
    }
}
