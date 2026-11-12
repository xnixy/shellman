//! Where the tiles go, in points: as large as they fit, in balanced rows, centred, never more
//! than most of the screen. Tiles shrink before they wrap past the screen; past the smallest
//! size, the oldest windows are left out.

/// A full-size tile's preview, in points. Labels keep their size as tiles shrink.
const PREVIEW_W: f32 = 216.0;
const PREVIEW_H: f32 = 136.0;
/// Around the preview, inside the tile.
pub const INSET: f32 = 8.0;
/// The icon and title under the preview.
pub const LABEL_H: f32 = 30.0;
/// Between tiles.
const GAP: f32 = 6.0;
/// Between the tiles and the panel's edge.
const PAD: f32 = 14.0;
/// Tiles shrink down to this share of their full size.
const MIN_SCALE: f32 = 0.5;
/// The panel covers at most this share of the screen's width and height.
const MAX_SHARE: f32 = 0.9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    /// The whole tile, which hovers and shows the selection.
    pub outer: Rect,
    /// Where the window's live preview (or its icon) goes.
    pub preview: Rect,
    /// The icon and title.
    pub label: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// The panel's size.
    pub w: f32,
    pub h: f32,
    /// Tiles per full row, for moving up and down.
    pub cols: usize,
    pub tiles: Vec<Tile>,
}

fn tile_size(scale: f32) -> (f32, f32) {
    (
        PREVIEW_W * scale + 2.0 * INSET,
        INSET + PREVIEW_H * scale + LABEL_H,
    )
}

/// How many tiles of `size` fit in `room` with gaps between them.
fn fitting(room: f32, size: f32) -> usize {
    (((room + GAP) / (size + GAP)).floor() as usize).max(1)
}

/// The most windows a screen of this size shows; the rest are left out.
pub fn capacity(screen: (f32, f32)) -> usize {
    let (tw, th) = tile_size(MIN_SCALE);
    let cols = fitting(screen.0 * MAX_SHARE - 2.0 * PAD, tw);
    let rows = fitting(screen.1 * MAX_SHARE - 2.0 * PAD, th);
    cols * rows
}

/// Tiles for `n` windows on a screen (work area) of `screen` points. Call with at most
/// `capacity(screen)` windows.
pub fn arrange(n: usize, screen: (f32, f32)) -> Layout {
    let (room_w, room_h) = (
        screen.0 * MAX_SHARE - 2.0 * PAD,
        screen.1 * MAX_SHARE - 2.0 * PAD,
    );
    // The largest scale, in steps of 5%, at which every tile fits.
    let mut scale = 1.0;
    let mut cols;
    loop {
        let (tw, th) = tile_size(scale);
        cols = fitting(room_w, tw).min(n.max(1));
        let rows = n.div_ceil(cols).max(1);
        let height = rows as f32 * (th + GAP) - GAP;
        if height <= room_h || scale <= MIN_SCALE {
            break;
        }
        scale = (scale - 0.05).max(MIN_SCALE);
    }
    // Balance the rows: five windows that need two rows go three and two, not four and one.
    let rows = n.div_ceil(cols).max(1);
    cols = n.div_ceil(rows).max(1);
    let (tw, th) = tile_size(scale);
    let w = 2.0 * PAD + cols as f32 * (tw + GAP) - GAP;
    let h = 2.0 * PAD + rows as f32 * (th + GAP) - GAP;
    let tiles = (0..n)
        .map(|i| {
            let (row, col) = (i / cols, i % cols);
            // A shorter last row is centred.
            let in_row = (n - row * cols).min(cols);
            let row_w = in_row as f32 * (tw + GAP) - GAP;
            let x = (w - row_w) / 2.0 + col as f32 * (tw + GAP);
            let y = PAD + row as f32 * (th + GAP);
            let outer = Rect { x, y, w: tw, h: th };
            Tile {
                outer,
                preview: Rect {
                    x: x + INSET,
                    y: y + INSET,
                    w: tw - 2.0 * INSET,
                    h: PREVIEW_H * scale,
                },
                label: Rect {
                    x: x + INSET,
                    y: y + INSET + PREVIEW_H * scale,
                    w: tw - 2.0 * INSET,
                    h: LABEL_H,
                },
            }
        })
        .collect();
    Layout { w, h, cols, tiles }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (f32, f32) = (1920.0, 1040.0);

    fn inside(layout: &Layout) -> bool {
        layout.tiles.iter().all(|t| {
            t.outer.x >= 0.0
                && t.outer.y >= 0.0
                && t.outer.x + t.outer.w <= layout.w + 0.01
                && t.outer.y + t.outer.h <= layout.h + 0.01
        })
    }

    #[test]
    fn few_windows_are_one_full_size_row() {
        let l = arrange(3, SCREEN);
        assert_eq!(l.cols, 3);
        assert_eq!(l.tiles.len(), 3);
        assert_eq!(l.tiles[0].preview.w, PREVIEW_W);
        assert!(l.tiles.iter().all(|t| t.outer.y == l.tiles[0].outer.y));
        assert!(inside(&l));
    }

    #[test]
    fn rows_are_balanced_and_the_last_is_centred() {
        // Seven full-size tiles fit across; nine wrap into two rows of five and four.
        let l = arrange(9, SCREEN);
        assert_eq!(l.cols, 5);
        let last = l.tiles[5..].iter().map(|t| t.outer.x).fold(f32::MAX, f32::min);
        let first = l.tiles[0].outer.x;
        assert!(last > first, "the shorter last row starts further in");
        let row_mid = (l.tiles[5].outer.x + l.tiles[8].outer.x + l.tiles[8].outer.w) / 2.0;
        assert!((row_mid - l.w / 2.0).abs() < 0.01, "and is centred");
        assert!(inside(&l));
    }

    #[test]
    fn many_windows_shrink_but_stay_on_screen() {
        let n = 40;
        assert!(n <= capacity(SCREEN));
        let l = arrange(n, SCREEN);
        assert!(l.tiles[0].preview.w < PREVIEW_W, "tiles shrink");
        assert!(l.w <= SCREEN.0 * MAX_SHARE && l.h <= SCREEN.1 * MAX_SHARE);
        assert!(inside(&l));
    }

    #[test]
    fn tiles_never_overlap() {
        let l = arrange(23, SCREEN);
        for (i, a) in l.tiles.iter().enumerate() {
            for b in &l.tiles[i + 1..] {
                let apart = a.outer.x + a.outer.w <= b.outer.x
                    || b.outer.x + b.outer.w <= a.outer.x
                    || a.outer.y + a.outer.h <= b.outer.y
                    || b.outer.y + b.outer.h <= a.outer.y;
                assert!(apart);
            }
        }
    }

    #[test]
    fn a_small_screen_holds_fewer() {
        assert!(capacity((800.0, 600.0)) < capacity(SCREEN));
        assert!(capacity((100.0, 100.0)) >= 1);
        let l = arrange(1, (100.0, 100.0));
        assert_eq!(l.tiles.len(), 1);
    }

    #[test]
    fn no_windows_is_an_empty_panel() {
        assert!(arrange(0, SCREEN).tiles.is_empty());
    }
}
