//! Which cells are colour emoji, and the sequences drawn as one glyph.
//!
//! The emulator gives each code point its `wcwidth` (an emoji two cells; a joiner, a variation
//! selector or a keycap mark nothing, kept on the cell before), and ConPTY is made to measure the
//! same way (`conpty::PSEUDOCONSOLE_GLYPH_WIDTH_WCSWIDTH`), so the grid and the program agree where
//! the cursor is. Drawing then joins what Unicode joins: a ZWJ sequence (👨‍👩‍👧, three wide cells
//! with joiners between) or an emoji and its skin tone (👍🏽, two wide cells) becomes one colour
//! glyph in the first two cells, the cells it took after them left blank. Flags stay text:
//! Windows' emoji font has none. Nothing here costs anything for text below U+2300 without marks.

use alacritty_terminal::index::Column;
use alacritty_terminal::term::cell::{Cell, Flags};

const ZWJ: char = '\u{200D}';
const VS16: char = '\u{FE0F}';
const VS15: char = '\u{FE0E}';
const KEYCAP: char = '\u{20E3}';

/// An emoji modifier (skin tone).
fn is_skin_tone(c: char) -> bool {
    ('\u{1F3FB}'..='\u{1F3FF}').contains(&c)
}

fn is_regional(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

/// A character that is an emoji when it takes two cells (Emoji_Presentation, which `wcwidth`
/// makes wide); CJK and other wide text is outside these blocks.
fn emoji_block(c: char) -> bool {
    let u = c as u32;
    u >= 0x1F000 && u <= 0x1FAFF || (0x2300..=0x2BFF).contains(&u) || matches!(u, 0x3030 | 0x303D | 0x3297 | 0x3299)
}

/// Whether a cell may start an emoji: worth a closer look.
pub fn candidate(cell: &Cell) -> bool {
    cell.c as u32 >= 0x2300 || cell.zerowidth().is_some()
}

/// The emoji starting at `col` of `row` (`cols` wide), as the text to draw, with the columns after
/// it that it takes over (to draw blank); None when the cell is not a colour emoji.
pub fn cluster<'a>(row: impl Fn(usize) -> &'a Cell, col: usize, cols: usize) -> Option<(String, usize)> {
    let first = row(col);
    let marks = first.zerowidth().unwrap_or(&[]);
    let wide = first.flags.contains(Flags::WIDE_CHAR);
    if marks.contains(&VS15) {
        return None;
    }
    let colour = (wide && emoji_block(first.c)) || (marks.contains(&VS16) && (first.c as u32 >= 0x2000 || marks.contains(&KEYCAP))) || marks.contains(&KEYCAP);
    // Windows' emoji font has no flags (it shows a flag's two letters), so regional indicators
    // stay text.
    if !colour || is_regional(first.c) {
        return None;
    }
    let mut text = String::new();
    text.push(first.c);
    text.extend(marks.iter());
    let span = |cell: &Cell| if cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
    let mut next = col + span(first);
    let mut last_marks: Vec<char> = marks.to_vec();
    let mut taken = 0;
    loop {
        if next >= cols {
            break;
        }
        let cell = row(next);
        let joined = last_marks.last() == Some(&ZWJ);
        if !(joined || is_skin_tone(cell.c)) || cell.c == ' ' || cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            break;
        }
        text.push(cell.c);
        let m = cell.zerowidth().unwrap_or(&[]);
        text.extend(m.iter());
        last_marks = m.to_vec();
        taken += span(cell);
        next += span(cell);
    }
    Some((text, taken))
}

/// [`cluster`] over a grid row.
pub fn in_row(row: &alacritty_terminal::grid::Row<Cell>, col: usize, cols: usize) -> Option<(String, usize)> {
    cluster(|c| &row[Column(c)], col, cols)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::index::Line;
    use alacritty_terminal::term::{test::TermSize, Config, Term};
    use alacritty_terminal::vte::ansi::Processor;

    fn row(text: &str) -> Vec<(usize, String, usize)> {
        let mut t = Term::new(Config::default(), &TermSize::new(40, 3), VoidListener);
        let mut p: Processor = Processor::new();
        p.advance(&mut t, text.as_bytes());
        let cols = t.columns();
        let r = &t.grid()[Line(0)];
        let mut out = Vec::new();
        let mut col = 0;
        while col < cols {
            if let Some((s, taken)) = in_row(r, col, cols) {
                out.push((col, s, taken));
                col += 2 + taken;
            } else {
                col += 1;
            }
        }
        out
    }

    #[test]
    fn plain_text_and_cjk_are_not_emoji() {
        assert!(row("hello wörld").is_empty());
        assert!(row("漢字 ☺ x").is_empty());
    }

    #[test]
    fn a_wide_emoji_is_one() {
        assert_eq!(row("a😀b"), [(1, "😀".to_string(), 0)]);
    }

    #[test]
    fn zwj_sequences_and_skin_tones_join() {
        // Man, woman, girl: three wide cells, drawn as one in the first two; four cells taken.
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(row(family), [(0, family.to_string(), 4)]);
        let thumbs = "\u{1F44D}\u{1F3FD}";
        assert_eq!(row(&format!("x{thumbs}y")), [(1, thumbs.to_string(), 2)]);
        // Two emoji side by side stay two.
        assert_eq!(row("😀😀").len(), 2);
    }

    #[test]
    fn variation_selectors_and_keycaps() {
        // A heart with VS16 is an emoji even in one cell; with VS15 it is text.
        assert_eq!(row("\u{2764}\u{FE0F}")[0].1, "\u{2764}\u{FE0F}");
        assert!(row("\u{2764}\u{FE0E}").is_empty());
        assert_eq!(row("1\u{FE0F}\u{20E3}")[0].1, "1\u{FE0F}\u{20E3}");
    }

    #[test]
    fn flags_stay_text() {
        assert!(row("\u{1F1E9}\u{1F1EA}").is_empty());
    }
}
