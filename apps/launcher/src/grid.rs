//! The app grid shown while the search box is empty: an optional "most used" row, then every app
//! alphabetically, in rows of icons with names beneath.
//!
//! `Grid` is only layout and arrow-key movement (no egui), so it is unit-tested; `show` draws
//! just the rows in view.

use shellman_core::apps_index::App;
use shellman_core::thumbs::Thumbs;
use shellman_ui::theme::colors;
use shellman_ui::widgets::draw_text;

pub const COLS: usize = 7;
const HEADER_H: f32 = 32.0;
const ROW_H: f32 = 94.0;
const ICON: f32 = 48.0;

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
    /// Lay out `count` apps (in list order), with `top` (indices of most used apps) in a row of
    /// their own above, when there are any.
    pub fn new(count: usize, top: &[usize]) -> Self {
        let mut rows = Vec::new();
        if !top.is_empty() {
            rows.push(Row::Header("Most used"));
            rows.push(Row::Items(top.to_vec()));
            rows.push(Row::Header("All apps"));
        }
        let all: Vec<usize> = (0..count).collect();
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
            height: y,
        }
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
}

/// Draw the rows in view. `reveal` scrolls the selected cell into view (after arrow keys).
pub fn show(
    ui: &mut egui::Ui,
    grid: &Grid,
    apps: &[App],
    selected: Pos,
    thumbs: &mut Thumbs,
    reveal: bool,
    reset_scroll: bool,
) -> Outcome {
    let c = colors();
    let mut out = Outcome::default();
    let mut area = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded);
    if reset_scroll {
        area = area.vertical_scroll_offset(0.0);
    }
    area.show_viewport(ui, |ui, view| {
        let width = ui.available_width();
        let origin = ui.cursor().min;
        ui.allocate_space(egui::vec2(width, grid.height));
        let cell_w = width / COLS as f32;
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
                        origin + egui::vec2(6.0, top),
                        egui::vec2(width - 12.0, HEADER_H),
                    );
                    draw_text(ui, rect, title, egui::FontId::proportional(12.5), c.muted, false);
                }
                Row::Items(items) => {
                    for (col, &index) in items.iter().enumerate() {
                        let rect = egui::Rect::from_min_size(
                            origin + egui::vec2(col as f32 * cell_w, top),
                            egui::vec2(cell_w, ROW_H),
                        );
                        let pos = Pos { row: r, col };
                        let resp = ui.interact(rect, egui::Id::new(("cell", r, col)), egui::Sense::click());
                        if resp.clicked() {
                            out.launch = Some(index);
                        }
                        if resp.hovered() && moved {
                            out.hover = Some(pos);
                        }
                        let is_selected = pos == selected;
                        if is_selected && reveal {
                            ui.scroll_to_rect(rect, None);
                        }
                        draw_cell(ui, rect, &apps[index], is_selected, resp.hovered(), thumbs);
                    }
                }
            }
        }
    });
    out
}

fn draw_cell(
    ui: &egui::Ui,
    rect: egui::Rect,
    app: &App,
    selected: bool,
    hovered: bool,
    thumbs: &mut Thumbs,
) {
    let c = colors();
    let p = ui.painter();
    let inner = rect.shrink2(egui::vec2(3.0, 2.0));
    if selected {
        p.rect_filled(inner, 10.0, c.row_selected);
    } else if hovered {
        p.rect_filled(inner, 10.0, c.row_hover);
    }
    let icon = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 10.0 + ICON / 2.0),
        egui::Vec2::splat(ICON),
    );
    crate::icon::paint(ui, icon, app, true, thumbs);
    let label = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 6.0, icon.bottom() + 6.0),
        egui::pos2(rect.right() - 6.0, icon.bottom() + 24.0),
    );
    draw_centered(ui, label, &app.name);
}

/// A name centred under its icon, cut with an ellipsis when it is too long.
fn draw_centered(ui: &egui::Ui, rect: egui::Rect, text: &str) {
    let c = colors();
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat {
            font_id: egui::FontId::proportional(12.5),
            color: c.text,
            ..Default::default()
        },
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width());
    let galley = ui.fonts(|f| f.layout_job(job));
    let pos = egui::pos2(rect.center().x - galley.size().x / 2.0, rect.top());
    ui.painter().galley(pos, galley, c.text);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(row: usize, col: usize) -> Pos {
        Pos { row, col }
    }

    #[test]
    fn layout_without_most_used() {
        let g = Grid::new(15, &[]);
        // 15 apps in rows of 7: 7 + 7 + 1.
        assert_eq!(g.rows.len(), 3);
        assert_eq!(g.first(), p(0, 0));
        assert_eq!(g.app_at(p(2, 0)), Some(14));
        assert_eq!(g.app_at(p(2, 1)), None);
    }

    #[test]
    fn layout_with_most_used_has_headers() {
        let g = Grid::new(10, &[4, 2]);
        // Header, most used, header, two rows of apps.
        assert_eq!(g.rows.len(), 5);
        assert_eq!(g.first(), p(1, 0));
        assert_eq!(g.app_at(p(1, 0)), Some(4));
        assert_eq!(g.app_at(p(3, 0)), Some(0));
        assert_eq!(g.height, 2.0 * HEADER_H + 3.0 * ROW_H);
    }

    #[test]
    fn arrows_move_in_two_dimensions() {
        let g = Grid::new(15, &[]);
        assert_eq!(g.step(p(0, 3), Dir::Down), p(1, 3));
        assert_eq!(g.step(p(1, 3), Dir::Up), p(0, 3));
        assert_eq!(g.step(p(0, 0), Dir::Up), p(0, 0));
        assert_eq!(g.step(p(0, 3), Dir::Right), p(0, 4));
        assert_eq!(g.step(p(0, 3), Dir::Left), p(0, 2));
    }

    #[test]
    fn down_into_a_short_row_stops_at_its_last_icon() {
        let g = Grid::new(15, &[]);
        assert_eq!(g.step(p(1, 5), Dir::Down), p(2, 0));
        assert_eq!(g.step(p(2, 0), Dir::Down), p(2, 0));
    }

    #[test]
    fn left_and_right_wrap_to_the_next_row() {
        let g = Grid::new(15, &[]);
        assert_eq!(g.step(p(0, 6), Dir::Right), p(1, 0));
        assert_eq!(g.step(p(1, 0), Dir::Left), p(0, 6));
        assert_eq!(g.step(p(0, 0), Dir::Left), p(0, 0));
        assert_eq!(g.step(p(2, 0), Dir::Right), p(2, 0));
    }

    #[test]
    fn arrows_skip_headers() {
        let g = Grid::new(10, &[4, 2]);
        // From the most-used row down lands on the first app row, past the "All apps" header.
        assert_eq!(g.step(p(1, 1), Dir::Down), p(3, 1));
        assert_eq!(g.step(p(3, 1), Dir::Up), p(1, 1));
        // Right at the end of the most-used row goes on to the first app row.
        assert_eq!(g.step(p(1, 1), Dir::Right), p(3, 0));
        assert_eq!(g.step(p(3, 0), Dir::Left), p(1, 1));
        assert_eq!(g.step(p(1, 1), Dir::Left), p(1, 0));
    }
}
