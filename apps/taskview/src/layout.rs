//! Where everything goes, in points of the overlay (the whole monitor): the header pill along the
//! top, and under it one tile per window in balanced rows, as large as they fit, centred.
//!
//! A tile is a title row (the app's icon, the window's title, a close button) over the window's
//! live preview. The compositor paints previews above everything the overlay draws, so nothing
//! else of a tile (selection, focus ring, close button) may reach into its preview: the tests
//! hold the layout to that. Tiles shrink down to a floor; past it, the windows used longest ago
//! are left out (`capacity`). Nothing scrolls, since a preview can't be clipped.

use egui::{pos2, vec2, Rect};

/// Between islands and around the edge, as everywhere in the islands language.
pub const GAP: f32 = 12.0;
pub const HEADER_H: f32 = 46.0;
/// A tile's title row, above its preview.
pub const TITLE_H: f32 = 40.0;
/// Inside a tile, left, right and below the preview.
pub const PAD: f32 = 10.0;
pub const CLOSE: f32 = 30.0;
pub const ICON: f32 = 20.0;
/// A preview's frame is 16:10; each window's picture is fitted inside, keeping its own shape.
const ASPECT: f32 = 1.6;
/// Preview widths: never larger (a lone window stays a tile, not a second screen), never smaller.
const MAX_PREVIEW_W: f32 = 480.0;
const MIN_PREVIEW_W: f32 = 150.0;
/// Most tiles a row may hold at once (a very wide screen still makes rows, not one long strip).
const MAX_COLS: usize = 8;
/// Never asked about more windows than this.
const MOST: usize = 120;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    /// The whole tile: hover, selection and clicks.
    pub outer: Rect,
    pub icon: Rect,
    pub title: Rect,
    pub close: Rect,
    /// The frame the live preview (or the app's icon) is fitted into.
    pub preview: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub header: Rect,
    pub tiles: Vec<Tile>,
}

/// The room under the header for the tiles.
fn room(screen: egui::Vec2) -> Rect {
    let top = GAP + HEADER_H + GAP;
    Rect::from_min_max(pos2(GAP, top), pos2((screen.x - GAP).max(GAP), (screen.y - GAP).max(top)))
}

/// The preview width every one of `n` tiles gets with `cols` per row in `room`.
fn preview_width(room: Rect, n: usize, cols: usize) -> f32 {
    let rows = n.div_ceil(cols);
    let by_w = (room.width() - (cols - 1) as f32 * GAP) / cols as f32 - 2.0 * PAD;
    let by_h = ((room.height() - (rows - 1) as f32 * GAP) / rows as f32 - TITLE_H - PAD) * ASPECT;
    by_w.min(by_h).min(MAX_PREVIEW_W)
}

/// The number of tiles per row that makes `n` tiles largest, and that width.
fn best(room: Rect, n: usize) -> (usize, f32) {
    (1..=n.clamp(1, MAX_COLS))
        .map(|cols| (cols, preview_width(room, n.max(1), cols)))
        // For the same size, more columns: fewer rows read more easily (and tiles at their
        // largest tie often).
        .fold((1, f32::MIN), |a, b| if b.1 >= a.1 - 0.01 { b } else { a })
}

/// The most windows this screen shows; the rest are left out.
pub fn capacity(screen: egui::Vec2) -> usize {
    let room = room(screen);
    (1..=MOST).take_while(|&n| best(room, n).1 >= MIN_PREVIEW_W).last().unwrap_or(1)
}

/// The overlay for `n` windows on a screen of `screen` points. Call with at most
/// `capacity(screen)` windows.
pub fn arrange(n: usize, screen: egui::Vec2) -> Layout {
    let header = Rect::from_min_size(pos2(GAP, GAP), vec2((screen.x - 2.0 * GAP).max(0.0), HEADER_H));
    if n == 0 {
        return Layout { header, tiles: Vec::new() };
    }
    let room = room(screen);
    let (cols, p) = best(room, n);
    let p = p.max(1.0);
    // Balanced rows: seven that need two rows go four and three, not six and one.
    let rows = n.div_ceil(cols);
    let cols = n.div_ceil(rows);
    let (tw, th) = (p + 2.0 * PAD, TITLE_H + p / ASPECT + PAD);
    let grid_h = rows as f32 * (th + GAP) - GAP;
    let top = room.top() + ((room.height() - grid_h) / 2.0).max(0.0);
    let tiles = (0..n)
        .map(|i| {
            let (row, col) = (i / cols, i % cols);
            let in_row = (n - row * cols).min(cols);
            let row_w = in_row as f32 * (tw + GAP) - GAP;
            let x = room.center().x - row_w / 2.0 + col as f32 * (tw + GAP);
            let y = top + row as f32 * (th + GAP);
            tile(Rect::from_min_size(pos2(x, y), vec2(tw, th)), p)
        })
        .collect();
    Layout { header, tiles }
}

fn tile(outer: Rect, p: f32) -> Tile {
    let mid = outer.top() + TITLE_H / 2.0;
    let icon = Rect::from_center_size(pos2(outer.left() + PAD + ICON / 2.0, mid), vec2(ICON, ICON));
    let close = Rect::from_center_size(pos2(outer.right() - PAD / 2.0 - CLOSE / 2.0, mid), vec2(CLOSE, CLOSE));
    let title = Rect::from_min_max(pos2(icon.right() + 8.0, outer.top()), pos2((close.left() - 6.0).max(icon.right() + 8.0), outer.top() + TITLE_H));
    let preview = Rect::from_min_size(pos2(outer.left() + PAD, outer.top() + TITLE_H), vec2(p, p / ASPECT));
    Tile { outer, icon, title, close, preview }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// The tile an arrow key moves to from `sel`. Left and right go through the tiles in order,
/// wrapping; up and down go to the nearest tile across in the row above or below, and stay put
/// at the top and bottom.
pub fn neighbour(tiles: &[Tile], sel: usize, dir: Dir) -> usize {
    let n = tiles.len();
    if n == 0 {
        return 0;
    }
    let sel = sel.min(n - 1);
    let here = tiles[sel].outer.center();
    let row_of = |t: &Tile| t.outer.center().y;
    match dir {
        Dir::Left => (sel + n - 1) % n,
        Dir::Right => (sel + 1) % n,
        Dir::Up | Dir::Down => {
            let ahead = |y: f32| if dir == Dir::Up { y < here.y - 0.5 } else { y > here.y + 0.5 };
            // The nearest row that way, then the tile in it nearest across.
            let Some(row) = tiles
                .iter()
                .map(row_of)
                .filter(|&y| ahead(y))
                .min_by(|a, b| (a - here.y).abs().total_cmp(&(b - here.y).abs()))
            else {
                return sel;
            };
            (0..n)
                .filter(|&i| (row_of(&tiles[i]) - row).abs() < 0.5)
                .min_by(|&a, &b| {
                    let d = |i: usize| (tiles[i].outer.center().x - here.x).abs();
                    d(a).total_cmp(&d(b))
                })
                .unwrap_or(sel)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FHD: egui::Vec2 = vec2(1920.0, 1080.0);

    fn apart(a: Rect, b: Rect) -> bool {
        a.right() <= b.left() || b.right() <= a.left() || a.bottom() <= b.top() || b.bottom() <= a.top()
    }

    fn check(l: &Layout, screen: egui::Vec2) {
        let room = room(screen);
        for (i, t) in l.tiles.iter().enumerate() {
            assert!(room.expand(0.01).contains_rect(t.outer), "tile {i} in the room under the header");
            assert!(apart(t.outer, l.header), "tile {i} clear of the header");
            for part in [t.icon, t.title, t.close, t.preview] {
                assert!(t.outer.expand(0.01).contains_rect(part), "tile {i}'s parts are inside it");
            }
            // The compositor paints over the preview: nothing else may reach into it.
            for part in [t.icon, t.title, t.close] {
                assert!(apart(part, t.preview), "tile {i}: {part:?} stays out of the preview");
            }
            assert!(apart(t.close, t.title) && apart(t.icon, t.title));
            // The selection outline and the focus ring (4 points outside) stay clear of every
            // other tile's preview.
            for (j, u) in l.tiles.iter().enumerate() {
                if i != j {
                    assert!(apart(t.outer.expand(4.0), u.preview), "tile {i}'s ring clear of tile {j}'s preview");
                    assert!(apart(t.outer, u.outer), "tiles {i} and {j} overlap");
                }
            }
        }
    }

    #[test]
    fn one_window_is_one_tile_of_the_largest_size_centred() {
        let l = arrange(1, FHD);
        assert_eq!(l.tiles.len(), 1);
        assert_eq!(l.tiles[0].preview.width(), MAX_PREVIEW_W);
        let r = room(FHD);
        assert!((l.tiles[0].outer.center().x - r.center().x).abs() < 0.01);
        assert!((l.tiles[0].outer.center().y - r.center().y).abs() < 0.01);
        check(&l, FHD);
    }

    #[test]
    fn rows_are_balanced_and_a_short_last_row_is_centred() {
        let l = arrange(7, FHD);
        let rows: Vec<f32> = l.tiles.iter().map(|t| t.outer.top()).collect();
        let first = rows.iter().filter(|&&y| y == rows[0]).count();
        assert!(first == 4 || first == 7, "four and three, or one row of seven: {first}");
        if first == 4 {
            let last = &l.tiles[4..];
            let mid = (last[0].outer.left() + last[2].outer.right()) / 2.0;
            assert!((mid - room(FHD).center().x).abs() < 0.01);
        }
        check(&l, FHD);
    }

    #[test]
    fn every_count_up_to_capacity_fits_on_common_screens() {
        for screen in [FHD, vec2(1280.0, 720.0), vec2(2560.0, 1440.0), vec2(1080.0, 1920.0), vec2(3840.0, 1080.0)] {
            let cap = capacity(screen);
            assert!(cap >= 6, "{screen:?} holds a reasonable number: {cap}");
            for n in 1..=cap {
                let l = arrange(n, screen);
                assert_eq!(l.tiles.len(), n);
                assert!(l.tiles[0].preview.width() >= MIN_PREVIEW_W - 0.01, "{screen:?} n={n}");
                check(&l, screen);
            }
        }
    }

    #[test]
    fn more_windows_make_smaller_tiles() {
        let few = arrange(3, FHD).tiles[0].preview.width();
        let many = arrange(20, FHD).tiles[0].preview.width();
        assert!(many < few);
    }

    #[test]
    fn a_tiny_screen_still_shows_one() {
        let screen = vec2(200.0, 150.0);
        assert_eq!(capacity(screen), 1);
        let l = arrange(1, screen);
        assert_eq!(l.tiles.len(), 1);
        assert!(l.tiles[0].preview.width() >= 1.0);
    }

    #[test]
    fn no_windows_is_just_the_header() {
        let l = arrange(0, FHD);
        assert!(l.tiles.is_empty());
        assert_eq!(l.header, Rect::from_min_size(pos2(GAP, GAP), vec2(1920.0 - 2.0 * GAP, HEADER_H)));
    }

    #[test]
    fn arrows_move_through_the_grid() {
        // Seven tiles: 0 1 2 3 / 4 5 6, the second row centred under the first.
        let l = arrange(7, FHD);
        let t = &l.tiles;
        if t[4].outer.top() == t[0].outer.top() {
            return; // laid out in one row on this screen; covered by the one-row test below
        }
        assert_eq!(neighbour(t, 3, Dir::Right), 4, "right runs on into the next row");
        assert_eq!(neighbour(t, 0, Dir::Left), 6, "and wraps");
        assert_eq!(neighbour(t, 1, Dir::Up), 1, "stays at the top");
        assert_eq!(neighbour(t, 5, Dir::Down), 5, "and at the bottom");
        // Down from 1 and 2: the centred row's tiles nearest across.
        let below_1 = neighbour(t, 1, Dir::Down);
        let below_2 = neighbour(t, 2, Dir::Down);
        assert!((4..7).contains(&below_1) && (4..7).contains(&below_2) && below_1 < below_2);
        assert_eq!(neighbour(t, 0, Dir::Down), 4);
        assert_eq!(neighbour(t, 3, Dir::Down), 6);
        // Back up from the centred row: a tile sitting between two goes to either, never further.
        assert!([0, 1].contains(&neighbour(t, 4, Dir::Up)));
        assert!([2, 3].contains(&neighbour(t, 6, Dir::Up)));
    }

    #[test]
    fn arrows_in_one_row_and_edge_cases() {
        let l = arrange(3, FHD);
        assert_eq!(neighbour(&l.tiles, 0, Dir::Down), 0);
        assert_eq!(neighbour(&l.tiles, 2, Dir::Right), 0);
        assert_eq!(neighbour(&[], 0, Dir::Right), 0);
        assert_eq!(neighbour(&l.tiles, 9, Dir::Up), 2, "a stale selection is kept in range");
    }
}
