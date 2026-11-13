//! The emulator's visible grid turned into the renderer's cells: colours resolved through the
//! palette (and any a program set with OSC 4/10/11), inverse and faint applied, wide characters
//! and the cursor marked. Taken under the emulator's lock, which it holds only for the copy.

use crate::palette::{dim, Palette, Rgb};
use crate::render::{self, Cell, Cursor, CursorShape, Frame};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
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
    let resolve = |c: Color| -> Rgb {
        let rgb = |v: ansi::Rgb| [v.r, v.g, v.b];
        match c {
            Color::Spec(v) => rgb(v),
            Color::Indexed(i) => colors[i as usize].map(rgb).unwrap_or_else(|| look.palette.indexed(i)),
            Color::Named(n) => {
                let set = colors[n].map(rgb);
                set.unwrap_or_else(|| match n {
                    NamedColor::Foreground | NamedColor::BrightForeground => look.palette.fg,
                    NamedColor::Background => look.palette.bg,
                    NamedColor::Cursor => look.cursor,
                    NamedColor::DimForeground => dim(look.palette.fg),
                    n if (n as usize) < 16 => look.palette.ansi[n as usize],
                    // The dim colours (259..=266) are the eight base ones, faint.
                    n => dim(look.palette.ansi[(n as usize - NamedColor::DimBlack as usize) & 7]),
                })
            }
        }
    };
    for line in 0..rows {
        let row = &grid[Line(line as i32 - offset)];
        for col in 0..cols {
            let cell = &row[Column(col)];
            let f = cell.flags;
            let default_bg = cell.bg == Color::Named(NamedColor::Background) && colors[NamedColor::Background].is_none();
            let mut fg = resolve(cell.fg);
            if f.contains(Flags::DIM) {
                fg = dim(fg);
            }
            let bg = resolve(cell.bg);
            let (mut fg, mut bg) = ([fg[0], fg[1], fg[2], 255], [bg[0], bg[1], bg[2], if default_bg { 0 } else { 255 }]);
            if f.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
                fg[3] = 255;
                bg[3] = 255;
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
            let ch = if hidden { ' ' } else { cell.c };
            // A wide character's spacer keeps its background run going but draws nothing.
            if f.contains(Flags::WIDE_CHAR_SPACER) {
                flags = 0;
            }
            frame.cells.push(Cell { ch, fg, bg, flags });
        }
    }

    // The cursor, only where it is on screen (not scrolled away) and shown.
    frame.cursor = None;
    let point = grid.cursor.point;
    let visible = term.mode().contains(TermMode::SHOW_CURSOR);
    let line = point.line.0 + offset;
    if visible && line >= 0 && (line as usize) < rows {
        let (row, mut col) = (line as usize, point.column.0.min(cols.saturating_sub(1)));
        if col > 0 && grid[point.line][Column(col)].flags.contains(Flags::WIDE_CHAR_SPACER) {
            col -= 1;
        }
        let i = row * cols + col;
        let wide = frame.cells[i].flags & render::WIDE != 0;
        let shape = match term.cursor_style().shape {
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
