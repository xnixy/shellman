//! The app grid shown while the search box is empty: an optional "most used" row, then every
//! listed app alphabetically, in rows of icons with names beneath, under small section labels.
//!
//! `Grid` is only layout and arrow-key movement (no egui), so it is unit-tested; `show` draws
//! just the rows in view.

use crate::fade::Fade;
use shellman_core::apps_index::App;
use shellman_core::thumbs::Thumbs;
use shellman_ui::theme::colors;

pub const COLS: usize = 6;
const CELL_W: f32 = 112.0;
const HEADER_H: f32 = 34.0;
const ROW_H: f32 = 116.0;
const ICON: f32 = 64.0;
/// Room after the last row, so it can scroll clear of the fade.
const END_PAD: f32 = 16.0;
const LABEL_SIZE: f32 = 12.5;

/// A cell: row and column in the grid.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Pos {
    pub row: usize,
    pub col: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

enum Row {
    Header(&'static str),
    /// Indices into the app list.
    Items(Vec<usize>),
}

pub struct Grid {
    rows: Vec<Row>,
    /// Top of each row in points, and the total height.
    tops: Vec<f32>,
    height: f32,
}

impl Grid {
    /// Lay out `all` (indices of the apps to list, in order), with `top` (indices of the most
    /// used apps) in a row of their own above, when there are any.
    pub fn new(all: &[usize], top: &[usize]) -> Self {
        let mut rows = Vec::new();
        if !top.is_empty() {
            rows.push(Row::Header("Most used"));
            rows.push(Row::Items(top.to_vec()));
        }
        rows.push(Row::Header("All apps"));
        rows.extend(all.chunks(COLS).map(|c| Row::Items(c.to_vec())));
        let mut tops = Vec::with_capacity(rows.len());
        let mut y = 0.0;
        for row in &rows {
            tops.push(y);
            y += match row {
                Row::Header(_) => HEADER_H,
                Row::Items(_) => ROW_H,
            };
        }
        Self {
            rows,
            tops,
            height: y + END_PAD,
        }
    }

    /// The grid's full height in points.
    pub fn height(&self) -> f32 {
        self.height
    }

    /// The first app's cell, where the selection starts.
    pub fn first(&self) -> Pos {
        let row = self
            .rows
            .iter()
            .position(|r| matches!(r, Row::Items(_)))
            .unwrap_or(0);
        Pos { row, col: 0 }
    }

    /// The app (index into the list) in a cell.
    pub fn app_at(&self, pos: Pos) -> Option<usize> {
        match self.rows.get(pos.row)? {
            Row::Items(items) => items.get(pos.col).copied(),
            Row::Header(_) => None,
        }
    }

    fn items(&self, row: usize) -> Option<&Vec<usize>> {
        match self.rows.get(row)? {
            Row::Items(items) => Some(items),
            Row::Header(_) => None,
        }
    }

    /// The cell one arrow key away. Left and right wrap to the neighbouring row; up and down
    /// keep the column, or stop at the last icon of a shorter row. At an end, stays put.
    pub fn step(&self, pos: Pos, dir: Dir) -> Pos {
        let len = self.items(pos.row).map_or(0, Vec::len);
        match dir {
            Dir::Left if pos.col > 0 => Pos { col: pos.col - 1, ..pos },
            Dir::Left => match self.items_row(pos.row, false) {
                Some(row) => Pos { row, col: self.items(row).map_or(0, |i| i.len() - 1) },
                None => pos,
            },
            Dir::Right if pos.col + 1 < len => Pos { col: pos.col + 1, ..pos },
            Dir::Right => match self.items_row(pos.row, true) {
                Some(row) => Pos { row, col: 0 },
                None => pos,
            },
            Dir::Up | Dir::Down => match self.items_row(pos.row, dir == Dir::Down) {
                Some(row) => Pos {
                    row,
                    col: pos.col.min(self.items(row).map_or(0, |i| i.len() - 1)),
                },
                None => pos,
            },
        }
    }

    /// The nearest row of icons above or below `row`, skipping headers.
    fn items_row(&self, row: usize, down: bool) -> Option<usize> {
        if down {
            (row + 1..self.rows.len()).find(|&r| self.items(r).is_some())
        } else {
            (0..row).rev().find(|&r| self.items(r).is_some())
        }
    }
}

/// What the user did in the grid this frame.
#[derive(Default)]
pub struct Outcome {
    /// An app was clicked (index into the list).
    pub launch: Option<usize>,
    /// The mouse moved onto a cell.
    pub hover: Option<Pos>,
    /// An app was right-clicked (its cell, its index into the list, where the pointer was).
    pub menu: Option<(Pos, usize, egui::Pos2)>,
}

/// Draw the rows in view. `reveal` scrolls the selected cell into view (after arrow keys).
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    grid: &Grid,
    apps: &[App],
    selected: Pos,
    thumbs: &mut Thumbs,
    fade: Fade,
    reveal: bool,
    reset_scroll: bool,
) -> Outcome {
    let mut out = Outcome::default();
    let mut area = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    if reset_scroll {
        area = area.vertical_scroll_offset(0.0);
    }
    area.show_viewport(ui, |ui, view| {
        let width = ui.available_width();
        let origin = ui.cursor().min;
        ui.allocate_space(egui::vec2(width, grid.height));
        let cell_w = CELL_W.min(width / COLS as f32);
        let left = (width - cell_w * COLS as f32) / 2.0;
        let moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
        for (r, row) in grid.rows.iter().enumerate() {
            let top = grid.tops[r];
            let h = match row {
                Row::Header(_) => HEADER_H,
                Row::Items(_) => ROW_H,
            };
            if top + h < view.min.y || top > view.max.y {
                continue;
            }
            match row {
                Row::Header(title) => {
                    let rect = egui::Rect::from_min_size(
                        origin + egui::vec2(left + 10.0, top),
                        egui::vec2(width - 2.0 * left - 20.0, HEADER_H),
                    );
                    fade.paint(ui, rect, |p| crate::section::label(p, rect, title));
                    let resp = ui.interact(rect, egui::Id::new(("header", r)), egui::Sense::hover());
                    shellman_ui::theme::speak(&resp, egui::WidgetType::Label, || title.to_string());
                }
                Row::Items(items) => {
                    for (col, &index) in items.iter().enumerate() {
                        let rect = egui::Rect::from_min_size(
                            origin + egui::vec2(left + col as f32 * cell_w, top),
                            egui::vec2(cell_w, ROW_H),
                        );
                        let pos = Pos { row: r, col };
                        let resp = ui.interact(rect, egui::Id::new(("cell", r, col)), egui::Sense::click());
                        // The search field keeps the keyboard; arrows move the selection, which
                        // a screen reader sees as this tile's selected state.
                        shellman_ui::theme::speak_selected(&resp, egui::WidgetType::SelectableLabel, pos == selected, || apps[index].name.clone());
                        if resp.clicked() {
                            out.launch = Some(index);
                        }
                        if resp.secondary_clicked() {
                            out.menu = Some((pos, index, resp.interact_pointer_pos().unwrap_or(rect.center())));
                        }
                        if resp.hovered() && moved {
                            out.hover = Some(pos);
                        }
                        let is_selected = pos == selected;
                        if is_selected && reveal {
                            ui.scroll_to_rect(rect, None);
                        }
                        let lines = name_lines(ui, &apps[index].grid_label(), cell_w - 12.0);
                        let hovered = resp.hovered();
                        fade.paint(ui, rect, |p| {
                            draw_cell(p, rect, &apps[index], &lines, is_selected, hovered, thumbs)
                        });
                    }
                }
            }
        }
    });
    out
}

/// The name as up to two lines that fit `width`.
fn name_lines(ui: &egui::Ui, name: &str, width: f32) -> Vec<String> {
    let fits = |s: &str| {
        let font = egui::FontId::proportional(LABEL_SIZE);
        ui.fonts(|f| f.layout_no_wrap(s.to_owned(), font, egui::Color32::WHITE).size().x) <= width
    };
    crate::label::wrap(name, 2, &fits)
}

fn draw_cell(
    p: &egui::Painter,
    rect: egui::Rect,
    app: &App,
    lines: &[String],
    selected: bool,
    hovered: bool,
    thumbs: &mut Thumbs,
) {
    let c = colors();
    let inner = rect.shrink2(egui::vec2(2.0, 0.0));
    // High contrast: a highlight outline around ordinary text for the selected tile, a text
    // outline for hover, never a fill.
    let fill = selected && !c.high_contrast;
    let ink = if fill { c.on_accent } else { c.text };
    if fill {
        p.rect_filled(inner, 14.0, c.accent_fill);
    } else if c.high_contrast && (selected || hovered) {
        let stroke = if selected { egui::Stroke::new(2.0_f32, c.row_selected) } else { egui::Stroke::new(1.0_f32, c.text) };
        p.rect_stroke(inner, 14.0, stroke, egui::StrokeKind::Inside);
    } else if hovered {
        p.rect_filled(inner, 14.0, c.text.gamma_multiply(0.08));
    }
    let icon = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 10.0 + ICON / 2.0),
        egui::Vec2::splat(ICON),
    );
    crate::icon::paint(p, icon, app, thumbs);
    for (i, line) in lines.iter().enumerate() {
        p.text(
            egui::pos2(rect.center().x, icon.bottom() + 7.0 + i as f32 * 16.0),
            egui::Align2::CENTER_TOP,
            line,
            egui::FontId::proportional(LABEL_SIZE),
            ink,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(row: usize, col: usize) -> Pos {
        Pos { row, col }
    }

    /// Apps 0..n, all listed.
    fn all(n: usize) -> Vec<usize> {
        (0..n).collect()
    }

    #[test]
    fn layout_without_most_used() {
        let g = Grid::new(&all(15), &[]);
        // "All apps" header, then 15 apps in rows of 6: 6 + 6 + 3.
        assert_eq!(g.rows.len(), 4);
        assert_eq!(g.first(), p(1, 0));
        assert_eq!(g.app_at(p(3, 2)), Some(14));
        assert_eq!(g.app_at(p(3, 3)), None);
    }

    #[test]
    fn layout_with_most_used_has_two_headers() {
        let g = Grid::new(&all(10), &[4, 2]);
        // Header, most used, header, two rows of apps (6 + 4).
        assert_eq!(g.rows.len(), 5);
        assert_eq!(g.first(), p(1, 0));
        assert_eq!(g.app_at(p(1, 0)), Some(4));
        assert_eq!(g.app_at(p(3, 0)), Some(0));
        assert_eq!(g.height, 2.0 * HEADER_H + 3.0 * ROW_H + END_PAD);
    }

    #[test]
    fn arrows_move_in_two_dimensions() {
        let g = Grid::new(&all(15), &[]);
        assert_eq!(g.step(p(1, 3), Dir::Down), p(2, 3));
        assert_eq!(g.step(p(2, 3), Dir::Up), p(1, 3));
        assert_eq!(g.step(p(1, 0), Dir::Up), p(1, 0));
        assert_eq!(g.step(p(1, 3), Dir::Right), p(1, 4));
        assert_eq!(g.step(p(1, 3), Dir::Left), p(1, 2));
    }

    #[test]
    fn down_into_a_short_row_stops_at_its_last_icon() {
        let g = Grid::new(&all(15), &[]);
        assert_eq!(g.step(p(2, 5), Dir::Down), p(3, 2));
        assert_eq!(g.step(p(3, 2), Dir::Down), p(3, 2));
    }

    #[test]
    fn left_and_right_wrap_to_the_next_row() {
        let g = Grid::new(&all(15), &[]);
        assert_eq!(g.step(p(1, 5), Dir::Right), p(2, 0));
        assert_eq!(g.step(p(2, 0), Dir::Left), p(1, 5));
        assert_eq!(g.step(p(1, 0), Dir::Left), p(1, 0));
        assert_eq!(g.step(p(3, 2), Dir::Right), p(3, 2));
    }

    #[test]
    fn arrows_skip_headers() {
        let g = Grid::new(&all(10), &[4, 2]);
        // From the most-used row down lands on the first app row, past the "All apps" header.
        assert_eq!(g.step(p(1, 1), Dir::Down), p(3, 1));
        assert_eq!(g.step(p(3, 1), Dir::Up), p(1, 1));
        // Right at the end of the most-used row goes on to the first app row.
        assert_eq!(g.step(p(1, 1), Dir::Right), p(3, 0));
        assert_eq!(g.step(p(3, 0), Dir::Left), p(1, 1));
        assert_eq!(g.step(p(1, 1), Dir::Left), p(1, 0));
    }

    #[test]
    fn six_columns_a_row() {
        assert_eq!(COLS, 6);
        let g = Grid::new(&all(7), &[]);
        assert_eq!(g.step(p(1, 5), Dir::Right), p(2, 0));
    }
}
