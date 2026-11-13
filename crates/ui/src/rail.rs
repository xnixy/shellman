//! The rail: the labelled list of pages (or places) at a window's left, as one island of glass.
//! Every shellman app that has one draws it with this, so they all look and behave alike: an icon
//! and a name per row, the current row filled with the accent, optional quiet group headings, and
//! a button at the foot that folds it to icons only (the name then shows as a hover pill).
//!
//! The caller lays the window out around it: ask [`width`] how wide the rail is, then call
//! [`show`]. Whether it is folded is the app's own choice, else the default every app shares
//! ([`crate::appearance::Appearance::rail_folded`]); a narrow window folds it regardless
//! ([`mode`]), without touching that choice.
//!
//! Rows are plain data ([`Row`]); what a row does beyond a click (a drop target, a middle click,
//! a context menu) the caller attaches to the row's response in [`Out::rows`]. The rail's own
//! empty space moves the window; [`Out::background`] is that response, for a menu.

use crate::chips;
use crate::glass::islands::{self, Island};
use crate::glass::Style;
use crate::theme::{colors, icon_font, semibold_font, with_alpha};
use egui::emath::GuiRounding as _;
use egui::text::{LayoutJob, TextWrapping};
use egui::{pos2, vec2, Align2, Color32, FontId, Id, Rect, Response, Sense, TextureId, Ui, UiBuilder};
use std::borrow::Cow;
use std::time::Duration;

/// Below this window width the rail shows icons only, whatever the user chose.
pub const NARROW_BELOW: f32 = 800.0;
/// Space between the window's edge and the rail island.
const MARGIN: f32 = 12.0;
/// The folded rail's width.
pub const FOLDED_W: f32 = 56.0;
const ICON_CELL: f32 = 34.0;
const ROW_H: f32 = 36.0;
const GAP: f32 = 2.0;
/// Space inside a wide row before its icon and after its name.
const ROW_PAD: f32 = 14.0;
/// The icon's column in a wide row, including the gap before the name.
const ICON_COL: f32 = 30.0;
/// Space inside the island above, below and beside its rows.
const INSET: f32 = 10.0;
const HEADING_H: f32 = 28.0;
const DIVIDER_H: f32 = 12.0;
/// The longest name a wide rail makes room for; longer ones end in an ellipsis.
const LABEL_MAX: f32 = 180.0;
/// A bitmap icon's size.
const IMAGE: f32 = 16.0;
const TIP_DELAY: f64 = 0.35;
const TIP_H: f32 = 32.0;
const TIP_PAD: f32 = 16.0;
const TIP_PAD_Y: f32 = 8.0;
const TIP_OFFSET: f32 = 8.0;
/// Segoe Fluent Icons: ClosePane and OpenPane.
const CLOSE_PANE: char = '\u{E89F}';
const OPEN_PANE: char = '\u{E8A0}';

/// What a row shows for its icon, drawn in the row's ink unless it has colours of its own.
#[derive(Clone, Debug, PartialEq)]
pub enum Icon {
    /// A Segoe Fluent Icons glyph.
    Glyph(char),
    /// A letter or two, in the text font (a pinned folder's initial).
    Initial(String),
    /// A coloured dot (a tag).
    Dot(Color32),
    /// A white mask texture, tinted with the row's ink.
    Image(TextureId),
    /// Nothing yet (an icon still loading).
    Blank,
}

/// One row: a place to go.
#[derive(Clone, Debug)]
pub struct Row<'a> {
    /// Stable and unique: it names the row for the selection and for egui.
    pub key: Cow<'a, str>,
    pub icon: Icon,
    pub label: Cow<'a, str>,
    /// More than the name, shown beside the row when the pointer rests on it (a folder's path).
    /// Without it, only a folded rail names its rows that way.
    pub tip: Option<String>,
    /// Faded: there but not usable now (a pinned folder that is gone).
    pub dim: bool,
}

impl<'a> Row<'a> {
    pub fn new(key: impl Into<Cow<'a, str>>, icon: Icon, label: impl Into<Cow<'a, str>>) -> Self {
        Self { key: key.into(), icon, label: label.into(), tip: None, dim: false }
    }

    pub fn tip(mut self, tip: impl Into<String>) -> Self {
        self.tip = Some(tip.into());
        self
    }

    pub fn dim(mut self, dim: bool) -> Self {
        self.dim = dim;
        self
    }
}

/// One line of the rail.
#[derive(Clone, Debug)]
pub enum Entry<'a> {
    /// A small quiet heading over the rows after it (a thin divider when the rail is folded).
    Heading(Cow<'a, str>),
    Row(Row<'a>),
}

impl<'a> Entry<'a> {
    /// A row with a glyph, the common case.
    pub fn item(key: &'a str, glyph: char, label: &'a str) -> Self {
        Entry::Row(Row::new(key, Icon::Glyph(glyph), label))
    }

    pub fn heading(text: impl Into<Cow<'a, str>>) -> Self {
        Entry::Heading(text.into())
    }

    fn label(&self) -> Option<&str> {
        match self {
            Entry::Row(r) => Some(&r.label),
            Entry::Heading(_) => None,
        }
    }
}

impl<'a> From<Row<'a>> for Entry<'a> {
    fn from(row: Row<'a>) -> Self {
        Entry::Row(row)
    }
}

/// How the rail is drawn at this moment.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Icon and name per row.
    Wide,
    /// Icons only, by the user's choice; the foot button opens it again.
    Folded,
    /// Icons only because the window is narrow; there is no button, as it could not open.
    Narrow,
}

impl Mode {
    pub fn icons_only(self) -> bool {
        self != Mode::Wide
    }
}

/// The mode for a window `window_width` wide when the app's sidebar is `folded` (see
/// [`crate::appearance::Appearance::rail_folded`]).
pub fn mode(window_width: f32, folded: bool) -> Mode {
    if window_width < NARROW_BELOW {
        Mode::Narrow
    } else if folded {
        Mode::Folded
    } else {
        Mode::Wide
    }
}

/// What the user did to the rail this frame.
pub struct Out {
    /// Index into the entries of the row that was clicked.
    pub clicked: Option<usize>,
    /// The foot button was pressed: flip this app's fold
    /// ([`crate::appearance::Appearance::toggle_rail_saved`]).
    pub toggled: bool,
    /// Each entry's response, by index (`None` for headings). Its `interact_rect` is clipped to
    /// what shows of the rail, so a drop target made from it never reaches past the island.
    pub rows: Vec<Option<Response>>,
    /// The rail's empty space (it already moves the window), for a menu of the caller's.
    pub background: Response,
    /// The island.
    pub rect: Rect,
}

fn row_h(mode: Mode) -> f32 {
    if mode.icons_only() { ICON_CELL } else { ROW_H }
}

/// How tall the entry at `i` is. Folded, a heading is a divider, and none above the first row.
fn entry_h(entries: &[Entry], i: usize, mode: Mode) -> f32 {
    match entries[i] {
        Entry::Heading(_) if mode.icons_only() && i == 0 => 0.0,
        Entry::Heading(_) if mode.icons_only() => DIVIDER_H,
        Entry::Heading(_) => HEADING_H,
        Entry::Row(_) => row_h(mode),
    }
}

fn list_h(entries: &[Entry], mode: Mode) -> f32 {
    let shown: Vec<f32> = (0..entries.len()).map(|i| entry_h(entries, i, mode)).filter(|h| *h > 0.0).collect();
    shown.iter().sum::<f32>() + GAP * shown.len().saturating_sub(1) as f32
}

fn has_foot(mode: Mode) -> bool {
    mode != Mode::Narrow
}

/// The island's height for these entries, before the window limits it.
pub fn wanted_height(entries: &[Entry], mode: Mode) -> f32 {
    let foot = if has_foot(mode) { GAP + row_h(mode) } else { 0.0 };
    2.0 * INSET + list_h(entries, mode) + foot
}

/// The wide rail's width for the longest name `longest` points wide.
fn wide_width(longest: f32) -> f32 {
    longest.min(LABEL_MAX).ceil() + ROW_PAD * 2.0 + ICON_COL + 2.0 * INSET
}

/// The rail's width: for the longest name when wide (up to a limit), the same for every app
/// when folded.
pub fn width(ui: &Ui, entries: &[Entry], mode: Mode) -> f32 {
    if mode.icons_only() {
        return FOLDED_W;
    }
    let longest = ui.fonts(|f| {
        entries
            .iter()
            .filter_map(Entry::label)
            .chain(std::iter::once("Collapse"))
            .map(|l| f.layout_no_wrap(l.to_owned(), semibold_font(14.0), Color32::WHITE).size().x)
            .fold(0.0, f32::max)
    });
    wide_width(longest)
}

/// Draw the rail at the window's left, `top` down, as an island pushed onto `list` (so the
/// window makes it glass). `solid` is the style to paint with when the window has no composed
/// glass. `selected` is the key of the current row.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    list: &mut Vec<Island>,
    screen: Rect,
    top: f32,
    entries: &[Entry],
    selected: Option<&str>,
    mode: Mode,
    solid: Option<&Style>,
    radius: f32,
) -> Out {
    let c = colors();
    let w = width(ui, entries, mode);
    let wide = mode == Mode::Wide;
    let foot = if has_foot(mode) { GAP + row_h(mode) } else { 0.0 };
    let height = wanted_height(entries, mode).min(screen.bottom() - MARGIN - top);
    let rail = Rect::from_min_size(pos2(screen.left() + MARGIN, top), vec2(w, height));
    list.push(if wide { Island::new(rail, radius) } else { Island::pill(rail) });

    // Its empty space moves the window, like the gaps around it. Made first, so rows win.
    let background = ui.interact(rail, Id::new("rail-bg"), Sense::click_and_drag());
    crate::widgets::window_drag(ui, &background);

    let mut out = Out { clicked: None, toggled: false, rows: vec![None; entries.len()], background, rect: rail };
    let row_w = if wide { w - 2.0 * INSET } else { ICON_CELL };
    let h = row_h(mode);
    let (_, hover, _) = chips::soft_colors(c.light);
    let mut named: Option<(Rect, String)> = None;

    // One row: fill, icon, name. Returns its response and whether its pill is due.
    let draw = |ui: &mut Ui, slot: Rect, row: &Row, current: bool| {
        let cell = Rect::from_center_size(slot.center(), vec2(row_w, h));
        let resp = ui.interact(cell, Id::new(("rail", row.key.as_ref())), Sense::click());
        let (fill, fg) = if current {
            (c.accent_fill, c.on_accent)
        } else if resp.hovered() {
            (hover, c.text)
        } else {
            (Color32::TRANSPARENT, c.muted)
        };
        let fade = |col: Color32| if row.dim { col.gamma_multiply(0.4) } else { col };
        ui.painter().rect_filled(cell, 12.0, fill);
        let icon_at = if wide { pos2(cell.left() + ROW_PAD + ICON_COL / 2.0 - 4.0, cell.center().y) } else { cell.center() };
        paint_icon(ui, &row.icon, icon_at, fade(fg));
        if wide {
            let font = if current { semibold_font(14.0) } else { FontId::proportional(14.0) };
            let ink = if current || resp.hovered() { fg } else { c.text.gamma_multiply(0.8) };
            let left = cell.left() + ROW_PAD + ICON_COL;
            let galley = fit(ui, &row.label, font, fade(ink), cell.right() - ROW_PAD / 2.0 - left);
            let at = pos2(left, cell.center().y - galley.size().y / 2.0);
            ui.painter().galley(at, galley, fade(ink));
        }
        // A rested pointer asks for the row's pill: its name when folded, its tip when it has one.
        let wants = !wide || row.tip.is_some();
        let tip = wants && ready(ui.ctx(), Id::new(("rail-tip", row.key.as_ref())), resp.hovered());
        (resp, tip)
    };

    let inner = Rect::from_min_max(pos2(rail.left(), rail.top() + INSET), pos2(rail.right(), rail.bottom() - INSET - foot));
    let scrolled = ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
        ui.spacing_mut().scroll = egui::style::ScrollStyle::floating();
        egui::ScrollArea::vertical().id_salt("rail-scroll").auto_shrink([false, false]).drag_to_scroll(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = GAP;
            for (i, entry) in entries.iter().enumerate() {
                let eh = entry_h(entries, i, mode);
                if eh <= 0.0 {
                    continue;
                }
                let (slot, _) = ui.allocate_exact_size(vec2(inner.width(), eh), Sense::hover());
                match entry {
                    Entry::Heading(text) if wide => {
                        let galley = fit(ui, &text.to_uppercase(), semibold_font(10.5), c.muted.gamma_multiply(0.8), slot.width() - 2.0 * (INSET + ROW_PAD));
                        let at = pos2(slot.left() + INSET + ROW_PAD, slot.bottom() - 6.0 - galley.size().y);
                        ui.painter().galley(at, galley, c.muted);
                    }
                    Entry::Heading(_) => {
                        let line = Rect::from_center_size(slot.center(), vec2(20.0, 1.0));
                        ui.painter().rect_filled(line, 0, with_alpha(c.text, 30));
                    }
                    Entry::Row(row) => {
                        let (resp, tip) = draw(ui, slot, row, selected == Some(row.key.as_ref()));
                        if tip {
                            let text = if wide { row.tip.clone() } else { Some(row.tip.clone().unwrap_or_else(|| row.label.to_string())) };
                            named = text.map(|t| (resp.rect, t));
                        }
                        if resp.clicked() {
                            out.clicked = Some(i);
                        }
                        out.rows[i] = Some(resp);
                    }
                }
            }
        })
    });
    // More rows than fit (wide; folded, the cut-off icon says it): a hairline over the foot, so the cut-off row reads as scrolled under.
    let more = scrolled.inner;
    if wide && more.content_size.y > more.inner_rect.height() + 0.5 {
        let y = rail.bottom() - INSET - h - GAP / 2.0 - 1.0;
        let line = Rect::from_center_size(pos2(rail.center().x, y), vec2(row_w - 2.0 * ROW_PAD, 1.0));
        ui.painter().rect_filled(line, 0, with_alpha(c.text, 30));
    }

    // The foot: the fold button, level with the rows above it.
    if has_foot(mode) {
        let slot = Rect::from_min_size(pos2(rail.left() + (w - row_w) / 2.0, rail.bottom() - INSET - h), vec2(row_w, h));
        let (glyph, label) = if wide { (CLOSE_PANE, "Collapse") } else { (OPEN_PANE, "Expand") };
        let fold = Row::new("rail-fold", Icon::Glyph(glyph), label);
        let (resp, tip) = draw(ui, slot, &fold, false);
        if tip {
            named = Some((resp.rect, label.to_owned()));
        }
        if resp.clicked() {
            out.toggled = true;
        }
    }

    if let Some((cell, text)) = named {
        let beside = Rect::from_min_max(pos2(rail.right() - 1.0, cell.top()), pos2(rail.right(), cell.bottom()));
        pill(ui, beside, &text, screen, list, solid);
    }
    out
}

fn paint_icon(ui: &Ui, icon: &Icon, at: egui::Pos2, fg: Color32) {
    let p = ui.painter();
    match icon {
        Icon::Glyph(g) => {
            p.text(at, Align2::CENTER_CENTER, g, icon_font(16.0), fg);
        }
        Icon::Initial(s) => {
            p.text(at, Align2::CENTER_CENTER, s, semibold_font(15.0), fg);
        }
        Icon::Dot(col) => {
            p.circle_filled(at, 6.0, *col);
        }
        Icon::Image(id) => {
            let ppp = ui.ctx().pixels_per_point();
            let rect = Rect::from_center_size(at, vec2(IMAGE, IMAGE)).round_to_pixels(ppp);
            p.image(*id, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), fg);
        }
        Icon::Blank => {}
    }
}

/// `text` on one line, ending in an ellipsis past `max` points.
fn fit(ui: &Ui, text: &str, font: FontId, color: Color32, max: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = TextWrapping { max_width: max.max(1.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.fonts(|f| f.layout_job(job))
}

/// Whether the pointer has rested on the thing `key` names for long enough. Asks for a repaint
/// when it will have, so nothing polls while it waits.
fn ready(ctx: &egui::Context, key: Id, hovered: bool) -> bool {
    if !hovered {
        ctx.data_mut(|d| d.remove::<f64>(key));
        return false;
    }
    let now = ctx.input(|i| i.time);
    let since = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(key, || now));
    let waited = now - since;
    if waited >= TIP_DELAY {
        return true;
    }
    ctx.request_repaint_after(Duration::from_secs_f64(TIP_DELAY - waited + 0.02));
    false
}

/// Where a pill of `size` goes: to the right of `anchor`, level with its middle, kept inside
/// `bounds`.
fn place(anchor: Rect, size: egui::Vec2, bounds: Rect) -> Rect {
    let x = (anchor.right() + TIP_OFFSET).min(bounds.right() - size.x).max(bounds.left());
    let y = (anchor.center().y - size.y / 2.0).clamp(bounds.top(), (bounds.bottom() - size.y).max(bounds.top()));
    Rect::from_min_size(pos2(x, y), size)
}

/// A row's name (or tip, a line or two), as a small glass pill beside the rail. The window is
/// transparent and only its registered islands are glass, so egui's own tooltip would be a stray
/// smudge; this one is an island of its own.
fn pill(ui: &mut Ui, anchor: Rect, text: &str, bounds: Rect, list: &mut Vec<Island>, solid: Option<&Style>) {
    let c = colors();
    let galley = ui.fonts(|f| f.layout_no_wrap(text.to_owned(), semibold_font(13.0), c.text));
    let size = vec2(galley.size().x + 2.0 * TIP_PAD, (galley.size().y + 2.0 * TIP_PAD_Y).max(TIP_H));
    let rect = place(anchor, size, bounds);
    // One line is a pill; more is a rounded card.
    let island = if size.y <= TIP_H { Island::pill(rect) } else { Island::new(rect, 14.0) };
    list.push(island);
    // Above everything, so neither the page nor the rail covers it.
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::new("rail-tip")));
    match solid {
        Some(style) => {
            for shape in islands::solid_shapes(&[island], style) {
                painter.add(shape);
            }
        }
        // Over glass the pill may stand partly on the page's island, where a second pane of
        // glass would not show against the first; a backing of its own does.
        None => {
            let r = if size.y <= TIP_H { (rect.height() / 2.0).round() as u8 } else { 14 };
            painter.rect(rect, r, c.popup, egui::Stroke::new(1.0_f32, c.hairline), egui::StrokeKind::Inside);
        }
    }
    let at = pos2(rect.left() + TIP_PAD, rect.center().y - galley.size().y / 2.0);
    painter.galley(at, galley, c.text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_windows_fold_without_changing_the_choice() {
        assert_eq!(mode(700.0, false), Mode::Narrow);
        assert_eq!(mode(700.0, true), Mode::Narrow);
        assert_eq!(mode(1000.0, false), Mode::Wide);
        assert_eq!(mode(1000.0, true), Mode::Folded);
    }

    #[test]
    fn an_apps_own_fold_picks_the_mode_over_the_default() {
        use crate::appearance::{Appearance, RailApp};
        let mut look = Appearance::default();
        look.set_rail_default(true);
        look.fold_rail(RailApp::Files, false);
        assert_eq!(mode(1000.0, look.rail_folded(RailApp::Files)), Mode::Wide);
        assert_eq!(mode(1000.0, look.rail_folded(RailApp::Tasks)), Mode::Folded);
        assert_eq!(mode(780.0, look.rail_folded(RailApp::Files)), Mode::Narrow);
    }

    #[test]
    fn the_wide_rail_fits_its_names_up_to_a_limit() {
        let short = wide_width(60.0);
        assert_eq!(short, 60.0 + ROW_PAD * 2.0 + ICON_COL + 2.0 * INSET);
        assert_eq!(wide_width(900.0), wide_width(LABEL_MAX));
        assert!(wide_width(LABEL_MAX) > FOLDED_W);
    }

    #[test]
    fn headings_are_dividers_when_folded_and_none_leads() {
        let e = [
            Entry::heading("Places"),
            Entry::item("home", 'a', "Home"),
            Entry::heading("Drives"),
            Entry::item("c", 'b', "C:"),
        ];
        assert_eq!(list_h(&e, Mode::Wide), 2.0 * HEADING_H + 2.0 * ROW_H + 3.0 * GAP);
        assert_eq!(list_h(&e, Mode::Folded), DIVIDER_H + 2.0 * ICON_CELL + 2.0 * GAP);
        assert_eq!(wanted_height(&e, Mode::Narrow), 2.0 * INSET + list_h(&e, Mode::Narrow));
        assert_eq!(wanted_height(&e, Mode::Folded), 2.0 * INSET + list_h(&e, Mode::Folded) + GAP + ICON_CELL);
    }

    #[test]
    fn the_pill_sits_right_of_the_row_level_with_it() {
        let bounds = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let icon = Rect::from_min_size(pos2(22.0, 100.0), vec2(34.0, 34.0));
        let r = place(icon, vec2(90.0, 32.0), bounds);
        assert_eq!(r.left(), icon.right() + TIP_OFFSET);
        assert_eq!(r.center().y, icon.center().y);
    }

    #[test]
    fn the_pill_is_never_clipped_by_the_window() {
        let bounds = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 300.0));
        let low = Rect::from_min_size(pos2(22.0, 290.0), vec2(34.0, 34.0));
        assert!(bounds.contains_rect(place(low, vec2(90.0, 32.0), bounds)));
        let far = Rect::from_min_size(pos2(180.0, 10.0), vec2(34.0, 34.0));
        assert!(bounds.contains_rect(place(far, vec2(90.0, 32.0), bounds)));
    }
}
