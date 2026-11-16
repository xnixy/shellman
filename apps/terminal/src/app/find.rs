//! Find in the scrollback (Ctrl+Shift+F): a glass pill in the top row, between the tabs and the
//! window buttons, as Files' search pill sits in its bar. Typing searches as it goes; Enter goes
//! to the next match up (older), Shift+Enter down; "Aa" makes it case-sensitive; Esc closes.
//!
//! The text is found with `alacritty_terminal`'s own search (its regex DFAs over the grid; the
//! query is taken literally). Every match in the history is counted for "3 of 120" a slice at a
//! time, a few milliseconds a frame from the bottom up, so 100 000 lines never stall the window;
//! the nearest matches are found first. What shows on screen is searched again for each
//! snapshot (a screenful is quick), so highlights are right even while output goes on; the count
//! starts over when output came, at most a few times a second.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Direction, Line, Point};
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};
use alacritty_terminal::term::Term;
use crate::snapshot::Highlight;
use egui::{Align2, Id, Rect, Stroke};
use shellman_ui::chips::round_icon;
use shellman_ui::theme::{self, colors, glyph, icon_font};
use std::time::{Duration, Instant};

/// The pill's width (narrower when the window is).
pub const WIDTH: f32 = 440.0;
/// The room the tabs keep beside the pill: one tab and the + and profile buttons.
const TABS_MIN: f32 = 170.0;
/// Narrower than this, the pill takes the whole row (the tabs give way while it is open).
const PILL_MIN: f32 = 300.0;
/// The pill's parts, in points: the search glyph, the field at its smallest, the count, the
/// case toggle, and the up, down and close buttons.
const GLYPH_W: f32 = 36.0;
const FIELD_MIN: f32 = 80.0;
const COUNT_W: f32 = 86.0;
const CASE_W: f32 = 36.0;
const BUTTONS_W: f32 = 3.0 * 32.0 + 6.0;

/// The pill's width in a row `row` wide (beside the tabs, `gap` from them), and whether it
/// takes the whole row.
pub fn pill_width(row: f32, gap: f32) -> (f32, bool) {
    let beside = (row - gap - TABS_MIN).min(WIDTH);
    if beside >= PILL_MIN { (beside, false) } else { (row.max(0.0), true) }
}

/// Which of the optional parts fit a pill `w` wide: the count goes first, then the case
/// toggle; the field always keeps its smallest width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Parts {
    pub count: bool,
    pub case: bool,
}

pub fn parts(w: f32) -> Parts {
    let base = GLYPH_W + FIELD_MIN + BUTTONS_W;
    Parts { count: w >= base + CASE_W + COUNT_W, case: w >= base + CASE_W }
}
/// Lines searched per slice of the count.
const CHUNK: i32 = 2000;
/// Time given to counting per frame.
const BUDGET: Duration = Duration::from_millis(4);
/// The count starts over at most this often while output comes.
const RECOUNT: Duration = Duration::from_millis(400);
const EDIT: &str = "term-find";

/// Regex metacharacters, escaped so the query is found as typed.
pub fn literal(query: &str, case: bool) -> String {
    let mut s = String::from(if case { "(?-i)" } else { "(?i)" });
    for ch in query.chars() {
        if "\\.+*?()|[]{}^$".contains(ch) {
            s.push('\\');
        }
        s.push(ch);
    }
    s
}

#[derive(Default)]
pub struct Find {
    pub open: bool,
    pub query: String,
    pub case: bool,
    focus: bool,
    regex: Option<RegexSearch>,
    /// Every match counted so far, nearest the bottom first.
    matches: Vec<Match>,
    /// The bottom line of the next slice to count; counting is done once past the top.
    next_line: i32,
    done: bool,
    /// The match shown as current (an index into `matches`).
    current: Option<usize>,
    /// Steps up asked for before the count got that far.
    owed: usize,
    /// Show the first match found (a new query) rather than leave the view (a recount).
    jump: bool,
    /// The tab's output count when counting started, and when.
    counted_at: (u64, Option<Instant>),
    tab: Option<u64>,
    /// Bumped whenever what is highlighted changes, so the snapshot is taken again.
    pub generation: u64,
}


impl Find {
    /// Open (or, when open, put the keyboard back in) the pill.
    pub fn show(&mut self) {
        self.open = true;
        self.focus = true;
        self.generation += 1;
    }

    /// The query was set from outside the pill (`SHELLMAN_TERMINAL_SCRIPT`): count afresh.
    pub fn changed_by_script(&mut self) {
        self.tab = None;
    }

    pub fn close(&mut self) {
        *self = Find { generation: self.generation + 1, case: self.case, query: std::mem::take(&mut self.query), ..Find::default() };
    }

    pub fn has_focus(&self, ctx: &egui::Context) -> bool {
        self.open && ctx.memory(|m| m.has_focus(Id::new(EDIT)))
    }

    /// Start over: a new query, case, or tab.
    fn restart(&mut self, bottom: i32, jump: bool) {
        self.regex = if self.query.is_empty() { None } else { RegexSearch::new(&literal(&self.query, self.case)).ok() };
        self.matches.clear();
        self.next_line = bottom;
        self.done = self.regex.is_none();
        self.current = None;
        self.owed = 0;
        self.jump = jump;
        self.generation += 1;
    }

    /// When the count, behind output that came since it started (`seq` is the output count
    /// now), is due to start over: a frame must come then, or a window with nothing else to
    /// draw would keep showing the old count ("No results" beside a match on screen).
    pub fn recount_due(&self, seq: u64) -> Option<Duration> {
        let since = self.counted_at.1?;
        (self.open && self.regex.is_some() && seq != self.counted_at.0).then(|| RECOUNT.saturating_sub(since.elapsed()))
    }

    /// Count on for a few milliseconds; start over when the query, the tab or the output
    /// changed. `seq` is the tab's output count.
    pub fn step<T: alacritty_terminal::event::EventListener>(&mut self, term: &mut Term<T>, tab: u64, seq: u64, changed: bool) -> bool {
        let bottom = term.bottommost_line().0;
        if changed || self.tab != Some(tab) {
            self.tab = Some(tab);
            self.counted_at = (seq, Some(Instant::now()));
            self.restart(bottom, true);
        } else if seq != self.counted_at.0 && self.counted_at.1.is_none_or(|t| t.elapsed() >= RECOUNT) {
            self.counted_at = (seq, Some(Instant::now()));
            self.restart(bottom, false);
        }
        let Some(regex) = self.regex.as_mut() else { return false };
        let top = term.topmost_line().0;
        let last = term.last_column();
        let start = Instant::now();
        let before = self.matches.len();
        while !self.done && start.elapsed() < BUDGET {
            let hi = self.next_line;
            let lo = (hi - CHUNK + 1).max(top);
            let mut found: Vec<Match> = RegexIter::new(Point::new(Line(lo), Column(0)), Point::new(Line(hi), last), Direction::Right, term, regex).collect();
            found.reverse();
            self.matches.extend(found);
            if lo <= top {
                self.done = true;
            } else {
                self.next_line = lo - 1;
            }
        }
        if self.matches.len() != before {
            self.generation += 1;
            if self.current.is_none() {
                // A new query shows the nearest match; a recount keeps to the view.
                let view_bottom = term.screen_lines() as i32 - 1 - term.grid().display_offset() as i32;
                let pick = if self.jump { Some(0) } else { self.matches.iter().position(|m| m.start().line.0 <= view_bottom) };
                if let Some(i) = pick {
                    self.current = Some(i);
                    if self.jump {
                        term.scroll_to_point(*self.matches[i].start());
                    }
                }
            }
            while self.owed > 0 && self.current.is_some_and(|i| i + 1 < self.matches.len()) {
                self.owed -= 1;
                self.move_by(term, 1);
            }
        }
        !self.done
    }

    /// Go `dir` matches up (positive, older) or down, wrapping once everything was counted.
    pub fn go<T: alacritty_terminal::event::EventListener>(&mut self, term: &mut Term<T>, up: bool) {
        if self.matches.is_empty() {
            return;
        }
        if up && self.current.is_some_and(|i| i + 1 >= self.matches.len()) && !self.done {
            self.owed += 1;
            return;
        }
        self.move_by(term, if up { 1 } else { -1 });
    }

    fn move_by<T: alacritty_terminal::event::EventListener>(&mut self, term: &mut Term<T>, by: i32) {
        let n = self.matches.len() as i32;
        let i = match self.current {
            None => 0,
            Some(i) => {
                let j = i as i32 + by;
                if j < 0 {
                    if self.done { n - 1 } else { 0 }
                } else if j >= n {
                    if self.done { 0 } else { n - 1 }
                } else {
                    j
                }
            }
        };
        self.current = Some(i as usize);
        term.scroll_to_point(*self.matches[i as usize].start());
        self.generation += 1;
    }

    /// The matches on screen now, and the current one.
    pub fn highlight<T>(&mut self, term: &Term<T>) -> Option<Highlight> {
        if !self.open {
            return None;
        }
        let regex = self.regex.as_mut()?;
        let offset = term.grid().display_offset() as i32;
        let top = Line(-offset);
        let bottom = Line(term.screen_lines() as i32 - 1 - offset);
        let matches = RegexIter::new(Point::new(top, Column(0)), Point::new(bottom, term.last_column()), Direction::Right, term, regex).collect();
        Some(Highlight { matches, current: self.current.and_then(|i| self.matches.get(i).cloned()) })
    }

    /// "3 of 120", "3 of 120+" while counting, or "No results".
    pub fn count_label(&self) -> String {
        if self.query.is_empty() {
            return String::new();
        }
        let more = if self.done { "" } else { "+" };
        match (self.current, self.matches.len()) {
            (_, 0) if self.done => "No results".into(),
            (_, 0) => "…".into(),
            (Some(i), n) => format!("{} of {n}{more}", i + 1),
            (None, n) => format!("{n}{more}"),
        }
    }
}

/// What the pill asks of the app this frame.
#[derive(Default)]
pub struct PillOut {
    /// The query or the case changed.
    pub changed: bool,
    /// Up (older) or down.
    pub go: Option<bool>,
    pub close: bool,
}

/// Draw the pill's contents in `rect` (its island is added by the caller).
pub fn pill(ui: &mut egui::Ui, rect: Rect, find: &mut Find) -> PillOut {
    let c = colors();
    let mut out = PillOut::default();
    let id = Id::new(EDIT);
    let focused = ui.memory(|m| m.has_focus(id));
    if focused {
        ui.painter().rect_stroke(rect.shrink(1.0), egui::CornerRadius::same((rect.height() / 2.0) as u8), Stroke::new(1.0_f32, theme::with_alpha(c.accent_fill, 190)), egui::StrokeKind::Inside);
    }
    ui.painter().text(egui::pos2(rect.left() + 18.0, rect.center().y), Align2::LEFT_CENTER, glyph::SEARCH, icon_font(12.0), c.muted);
    let show = parts(rect.width());
    let label = find.count_label();
    // From the right: close, down, up, case, the count.
    let b = 30.0;
    let mut x = rect.right() - 6.0;
    let mut button = |ui: &mut egui::Ui, g: char, name: &str, tip: &str, on: bool| {
        let r = Rect::from_center_size(egui::pos2(x - b / 2.0, rect.center().y), egui::vec2(b, b));
        x -= b + 2.0;
        let resp = round_icon(ui, r, Id::new(("find", name)), g, true, on, tip);
        shellman_ui::hover::label(ui, &resp, tip);
        resp.clicked()
    };
    if button(ui, glyph::CLOSE, "close", "Close (Esc)", false) {
        out.close = true;
    }
    if button(ui, glyph::SORT_DOWN, "down", "Next down (Shift+Enter)", false) {
        out.go = Some(false);
    }
    if button(ui, glyph::SORT_UP, "up", "Next up (Enter)", false) {
        out.go = Some(true);
    }
    // The case toggle: "Aa", lit with the accent when on (in high contrast, outlined).
    if show.case {
        let r = Rect::from_center_size(egui::pos2(x - b / 2.0, rect.center().y), egui::vec2(b, b));
        x -= b + 6.0;
        let resp = ui.interact(r, Id::new(("find", "case")), egui::Sense::click());
        if find.case {
            ui.painter().circle_filled(r.center(), b / 2.0, c.accent_fill);
        } else if resp.hovered() && !c.high_contrast {
            ui.painter().circle_filled(r.center(), b / 2.0, shellman_ui::chips::soft_colors(c.light).1);
        }
        if c.high_contrast {
            ui.painter().circle_stroke(r.center(), b / 2.0 - 0.5, Stroke::new(1.0_f32, c.text));
        }
        let fg = if find.case { c.on_accent } else { c.muted };
        ui.painter().text(r.center(), Align2::CENTER_CENTER, "Aa", egui::FontId::proportional(12.5), fg);
        shellman_ui::hover::label(ui, &resp, if find.case { "Match case: on" } else { "Match case: off" });
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, find.case, "Match case"));
        theme::focus_ring(ui, &resp, r, b / 2.0);
        if resp.clicked() {
            find.case = !find.case;
            out.changed = true;
        }
    }
    if show.count {
        ui.painter().text(egui::pos2(x - 4.0, rect.center().y), Align2::RIGHT_CENTER, &label, egui::FontId::proportional(12.5), c.muted);
        x -= COUNT_W;
    }
    // The field's name for a screen reader, with the count (which may not show).
    let name = if label.is_empty() { "Find in the terminal".to_owned() } else { format!("Find in the terminal, {label}") };
    let glyph_r = Rect::from_min_size(rect.min, egui::vec2(GLYPH_W, rect.height()));
    let name_resp = ui.interact(glyph_r, Id::new(("find", "name")), egui::Sense::hover());
    name_resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name));
    let inner = Rect::from_min_max(egui::pos2(rect.left() + GLYPH_W, rect.top()), egui::pos2(x.max(rect.left() + GLYPH_W), rect.bottom()));
    let edit = ui.put(
        inner,
        egui::TextEdit::singleline(&mut find.query)
            .id(id)
            .frame(false)
            .hint_text(egui::RichText::new("Find").color(c.muted))
            .vertical_align(egui::Align::Center)
            .margin(egui::vec2(2.0, 0.0))
            .desired_width(inner.width()),
    )
    .labelled_by(name_resp.id);
    if std::mem::take(&mut find.focus) {
        edit.request_focus();
    }
    if edit.changed() {
        out.changed = true;
    }
    // Enter would leave the field: it steps instead, and the field keeps the keyboard.
    let (enter, shift, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.modifiers.shift, i.key_pressed(egui::Key::Escape)));
    if edit.has_focus() || edit.lost_focus() {
        if enter {
            out.go = Some(!shift);
            edit.request_focus();
        }
        if esc {
            out.close = true;
        }
    }
    out
}


#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::{test::TermSize, Config};
    use alacritty_terminal::vte::ansi::Processor;

    #[test]
    fn the_pill_never_overflows_its_row() {
        // Wide: the full pill beside the tabs.
        assert_eq!(pill_width(1200.0, 12.0), (WIDTH, false));
        // Narrower: it gives way, keeping the tabs their room, down to its smallest.
        let (w, alone) = pill_width(560.0, 12.0);
        assert!(!alone && w + 12.0 + TABS_MIN <= 560.0 + 0.01 && w >= PILL_MIN);
        // The narrowest window (420 wide leaves a 274 row): the whole row.
        assert_eq!(pill_width(274.0, 12.0), (274.0, true));
        // Parts drop out in order, the field always keeps its smallest width.
        assert_eq!(parts(WIDTH), Parts { count: true, case: true });
        assert_eq!(parts(274.0), Parts { count: false, case: true });
        assert_eq!(parts(200.0), Parts { count: false, case: false });
        for w in [200.0, 254.0, 274.0, 300.0, 340.0, WIDTH] {
            let p = parts(w);
            let used = GLYPH_W + FIELD_MIN + BUTTONS_W + if p.case { CASE_W } else { 0.0 } + if p.count { COUNT_W } else { 0.0 };
            assert!(used <= w.max(GLYPH_W + FIELD_MIN + BUTTONS_W), "{w}: {used}");
        }
    }

    #[test]
    fn queries_are_taken_literally() {
        assert_eq!(literal("a.b*(c)", false), r"(?i)a\.b\*\(c\)");
        assert_eq!(literal("Path\\x", true), r"(?-i)Path\\x");
        assert!(RegexSearch::new(&literal("[ok] {1} $HOME ^x | y?", false)).is_ok());
    }

    fn term(lines: usize, text: &str) -> Term<VoidListener> {
        let config = Config { scrolling_history: lines, ..Config::default() };
        let mut t = Term::new(config, &TermSize::new(80, 24), VoidListener);
        let mut p: Processor = Processor::new();
        p.advance(&mut t, text.as_bytes());
        t
    }

    fn count(t: &mut Term<VoidListener>, f: &mut Find) {
        let mut first = true;
        while f.step(t, 0, 0, std::mem::take(&mut first)) {}
    }

    /// 100 000 lines: counted in slices of a few milliseconds each, every match found, the
    /// nearest first, and stepping up goes to older lines.
    #[test]
    fn the_whole_history_is_counted_in_slices() {
        let mut text = String::new();
        for i in 0..100_000 {
            if i % 1000 == 0 {
                text.push_str(&format!("line {i} NEEDLE here\r\n"));
            } else {
                text.push_str(&format!("line {i} hay\r\n"));
            }
        }
        let mut t = term(100_000, &text);
        let mut f = Find { query: "needle".into(), open: true, ..Find::default() };
        let mut slices = 0;
        let mut first = true;
        let mut longest = Duration::ZERO;
        loop {
            let started = Instant::now();
            let more = f.step(&mut t, 0, 0, std::mem::take(&mut first));
            longest = longest.max(started.elapsed());
            slices += 1;
            if !more {
                break;
            }
        }
        assert!(slices > 1, "counted in one go");
        // Some 100k lines minus what scrolled off: every 1000th line.
        assert!(f.matches.len() >= 99, "found {}", f.matches.len());
        assert_eq!(f.current, Some(0));
        assert!(f.count_label().starts_with("1 of "));
        let newest = f.matches[0].start().line;
        f.go(&mut t, true);
        assert!(f.matches[f.current.unwrap()].start().line < newest);
        // A frame's slice stays short (debug build: generous).
        assert!(longest < Duration::from_millis(250), "a slice took {longest:?}");
    }

    /// Output that came just after a count started is counted once the wait is over, even when
    /// nothing else asks for a frame.
    #[test]
    fn a_count_behind_new_output_asks_for_a_frame() {
        let mut t = term(1000, "nothing here

");
        let mut f = Find { query: "here".into(), open: true, ..Find::default() };
        while f.step(&mut t, 0, 1, false) {}
        assert_eq!(f.recount_due(1), None, "up to date");
        let due = f.recount_due(2).expect("a frame is asked for");
        assert!(due <= RECOUNT);
        f.close();
        assert_eq!(f.recount_due(3), None, "closed");
    }

    #[test]
    fn case_and_wrapping() {
        let mut t = term(1000, "Alpha\r\nalpha\r\nALPHA\r\n");
        let mut f = Find { query: "alpha".into(), open: true, ..Find::default() };
        count(&mut t, &mut f);
        assert_eq!(f.matches.len(), 3);
        f.case = true;
        count(&mut t, &mut f);
        assert_eq!(f.matches.len(), 1);
        // Down from the newest wraps to the oldest once all are counted.
        f.case = false;
        count(&mut t, &mut f);
        assert_eq!(f.current, Some(0));
        f.go(&mut t, false);
        assert_eq!(f.current, Some(2));
        f.go(&mut t, true);
        assert_eq!(f.current, Some(0));
        let mut none = Find { query: "zzz".into(), open: true, ..Find::default() };
        count(&mut t, &mut none);
        assert_eq!(none.count_label(), "No results");
        let h = f.highlight(&t).unwrap();
        assert_eq!(h.matches.len(), 3);
    }
}
