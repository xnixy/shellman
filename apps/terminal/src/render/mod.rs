//! The grid renderer: a snapshot of cells in, one OpenGL pass out, inside egui's frame.
//!
//! [`Renderer::prepare`] runs in the frame's update: it hashes each row of the snapshot and, for
//! the rows that changed, turns the cells into instances (background runs, glyphs from the
//! atlas, underline and strike runs). [`Renderer::paint`] then gives egui a paint callback that
//! sends those rows and any new glyphs to the GPU and draws the whole grid with three instanced
//! draws. Nothing here knows about the emulator or the shell.

mod atlas;
mod boxdraw;
mod color;
mod gpu;

pub use atlas::Metrics;
use egui_glow::glow;
use gpu::{Gpu, OVER_PER_COL};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

/// Cell flags.
pub const BOLD: u8 = 1;
pub const ITALIC: u8 = 2;
pub const UNDERLINE: u8 = 4;
pub const DOUBLE_UNDERLINE: u8 = 8;
pub const STRIKE: u8 = 16;
/// The glyph takes this cell and the next.
pub const WIDE: u8 = 32;
/// A colour glyph (emoji): `ch` is not a character but the cluster's number in the frame's
/// [`Clusters`].
pub const COLOR: u8 = 64;

/// Emoji clusters (an emoji with its joiners, skin tone, selectors) by number, so a cell can
/// name one in its `ch`. Numbers are never reused, so a row's hash stays true to what it shows.
#[derive(Default)]
pub struct Clusters {
    map: HashMap<String, u32>,
    list: Vec<String>,
}

impl Clusters {
    /// The number for `text`, as the `char` a cell holds; None past the numbers a `char` can
    /// carry (the cell is then drawn as text).
    pub fn intern(&mut self, text: String) -> Option<char> {
        if let Some(&n) = self.map.get(&text) {
            return char::from_u32(n);
        }
        let n = self.list.len() as u32;
        let c = char::from_u32(n).filter(|_| n < 0xD000)?;
        self.list.push(text.clone());
        self.map.insert(text, n);
        Some(c)
    }

    pub fn get(&self, c: char) -> Option<&str> {
        self.list.get(c as usize).map(String::as_str)
    }
}

/// One cell as the renderer sees it: colours already resolved (inverse, dim, the palette).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Cell {
    pub ch: char,
    pub fg: [u8; 4],
    /// Alpha 0 leaves the glass showing.
    pub bg: [u8; 4],
    pub flags: u8,
}


/// A cursor drawn over the cells. A focused block cursor is part of the cells instead (its cell
/// gets the cursor's colours), so only these shapes are drawn on top.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CursorShape {
    Beam,
    Underline,
    /// The outline a block becomes when the window is not focused.
    Hollow,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Cursor {
    pub col: usize,
    pub row: usize,
    pub shape: CursorShape,
    pub color: [u8; 4],
    /// Two cells wide (on a wide character).
    pub wide: bool,
}

/// The visible grid: `rows` rows of `cols` cells, row by row.
pub struct Frame {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<Cell>,
    pub cursor: Option<Cursor>,
    /// The cursor blinks (a program asked for it, and the window has the focus).
    pub blinking: bool,
    /// The emoji the cells name.
    pub clusters: Clusters,
}

impl Frame {
    pub fn row(&self, r: usize) -> &[Cell] {
        &self.cells[r * self.cols..(r + 1) * self.cols]
    }
}

/// A rectangle instance: position and size in pixels, and a colour.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RectInst {
    pos: [i16; 2],
    size: [u16; 2],
    color: [u8; 4],
}

/// A glyph instance: where it goes, where it is in the atlas, its size and colour.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct GlyphInst {
    pos: [i16; 2],
    uv: [u16; 2],
    size: [u16; 2],
    color: [u8; 4],
}

/// One changed row's instances, each list exactly as long as the row's slots.
struct RowUpdate {
    index: usize,
    bg: Vec<RectInst>,
    glyphs: Vec<GlyphInst>,
    over: Vec<RectInst>,
    /// Colour glyphs, one slot a column; None for a row with none (and none before).
    emoji: Option<Vec<GlyphInst>>,
}

/// What the GPU has not seen yet. Several updates can pile up before a paint.
#[derive(Default)]
struct Update {
    resize: Option<(usize, usize)>,
    glyphs: Vec<atlas::Upload>,
    color_glyphs: Vec<color::Upload>,
    rows: Vec<RowUpdate>,
    /// Whether any row on screen has a colour glyph (else that draw is skipped).
    any_emoji: bool,
}

/// The GPU side, shared with the paint callback (which egui needs to be `Send + Sync`).
#[derive(Default)]
struct Shared {
    update: Update,
    gpu: Option<Gpu>,
    failed: bool,
}

pub struct Renderer {
    atlas: atlas::Atlas,
    color: color::ColorAtlas,
    /// Each row's count of colour glyphs as last sent.
    emoji_rows: Vec<u16>,
    /// The font size in pixels and the family the atlas was made for.
    px: f32,
    family: String,
    /// Each row's hash as last sent, and the grid size they are for.
    hashes: Vec<u64>,
    cols: usize,
    shared: Arc<Mutex<Shared>>,
}

impl Renderer {
    /// A renderer for the grid font at `px` pixels per em.
    pub fn new(px: f32, family: &str) -> Result<Self, String> {
        Ok(Self {
            atlas: atlas::Atlas::new(px, family)?,
            color: color::ColorAtlas::new(px),
            emoji_rows: Vec::new(),
            px,
            family: family.to_owned(),
            hashes: Vec::new(),
            cols: 0,
            shared: Arc::default(),
        })
    }

    pub fn metrics(&self) -> Metrics {
        self.atlas.metrics
    }


    /// Change the font size (zoom, or a move to a monitor of another scale). Every glyph is
    /// made again.
    pub fn set_px(&mut self, px: f32) -> Result<(), String> {
        if px == self.px {
            return Ok(());
        }
        let atlas = atlas::Atlas::new(px, &self.family)?;
        self.atlas = atlas;
        self.color = color::ColorAtlas::new(px);
        self.px = px;
        self.hashes.clear();
        Ok(())
    }

    /// Change the font family (Settings): every glyph is made again.
    pub fn set_family(&mut self, family: &str) -> Result<(), String> {
        if family == self.family {
            return Ok(());
        }
        self.atlas = atlas::Atlas::new(self.px, family)?;
        self.color = color::ColorAtlas::new(self.px);
        self.family = family.to_owned();
        self.hashes.iter_mut().for_each(|h| *h = 0);
        Ok(())
    }

    /// Bring the GPU's copy up to `frame`: only rows whose cells (or cursor) changed are
    /// rebuilt and sent. Returns how many rows that was.
    pub fn prepare(&mut self, frame: &Frame) -> usize {
        if frame.cols != self.cols || frame.rows != self.hashes.len() {
            self.cols = frame.cols;
            self.hashes = vec![0; frame.rows];
            self.emoji_rows = vec![0; frame.rows];
            let mut shared = self.shared.lock().unwrap();
            shared.update.resize = Some((frame.cols, frame.rows));
            shared.update.rows.clear();
            // Fresh buffers start empty, so every row is sent (a zero hash never matches).
        }
        let mut changed = Vec::new();
        for r in 0..frame.rows {
            let hash = row_hash(frame, r);
            if hash != self.hashes[r] {
                changed.push((r, hash));
            }
        }
        if changed.is_empty() {
            return 0;
        }
        let rows = match self.build(frame, &changed) {
            Ok(rows) => rows,
            Err(Full) => {
                // The atlas was emptied: every row is placed again from a fresh one.
                self.hashes.iter_mut().for_each(|h| *h = 0);
                changed = (0..frame.rows).map(|r| (r, row_hash(frame, r))).collect();
                match self.build(frame, &changed) {
                    Ok(rows) => rows,
                    // Not even one screen fits: draw what was there, try again next time.
                    Err(Full) => return 0,
                }
            }
        };
        for &(r, hash) in &changed {
            self.hashes[r] = hash;
        }
        let n = rows.len();
        for row in &rows {
            self.emoji_rows[row.index] = row.emoji.as_ref().map_or(0, |e| e.iter().filter(|g| g.size[0] > 0).count() as u16);
        }
        let mut shared = self.shared.lock().unwrap();
        shared.update.glyphs.append(&mut self.atlas.uploads);
        shared.update.color_glyphs.append(&mut self.color.uploads);
        shared.update.rows.extend(rows);
        shared.update.any_emoji = self.emoji_rows.iter().any(|&n| n > 0);
        n
    }

    fn build(&mut self, frame: &Frame, changed: &[(usize, u64)]) -> Result<Vec<RowUpdate>, Full> {
        changed.iter().map(|&(r, _)| self.build_row(frame, r)).collect()
    }

    fn build_row(&mut self, frame: &Frame, r: usize) -> Result<RowUpdate, Full> {
        let m = self.atlas.metrics;
        let cols = frame.cols;
        let top = (r as i32 * m.cell_h) as i16;
        let mut bg: Vec<RectInst> = Vec::with_capacity(cols);
        let mut glyphs = vec![GlyphInst::default(); cols];
        // A row that had colour glyphs gets its slots cleared even when it has none now.
        let mut emoji: Option<Vec<GlyphInst>> = (self.emoji_rows.get(r).copied().unwrap_or(0) > 0).then(|| vec![GlyphInst::default(); cols]);
        let mut over: Vec<RectInst> = Vec::with_capacity(cols * OVER_PER_COL);
        // The run being extended: start column, end column, colour.
        let mut bg_run: Option<(usize, usize, [u8; 4])> = None;
        let mut lines: [Option<(usize, usize, [u8; 4])>; 3] = [None; 3];
        let rect = |from: usize, to: usize, y: i32, h: i32, color: [u8; 4]| RectInst {
            pos: [(from as i32 * m.cell_w) as i16, y as i16],
            size: [((to - from) as i32 * m.cell_w) as u16, h as u16],
            color,
        };
        let line_y = [
            (m.baseline + m.underline - m.underline_thick / 2, m.underline_thick),
            (m.baseline + m.underline - m.underline_thick / 2 + 2 * m.underline_thick, m.underline_thick),
            (m.baseline - m.strike, m.strike_thick),
        ];
        let row = frame.row(r);
        for (c, cell) in row.iter().enumerate() {
            let span = if cell.flags & WIDE != 0 { 2 } else { 1 };
            // Backgrounds: runs of one colour become one rectangle.
            let want = (cell.bg[3] > 0).then_some(cell.bg);
            match (&mut bg_run, want) {
                (Some((_, end, color)), Some(w)) if *end == c && *color == w => *end = c + span,
                (run, want) => {
                    if let Some((s, e, color)) = run.take() {
                        bg.push(rect(s, e, top as i32, m.cell_h, color));
                    }
                    *run = want.map(|w| (c, c + span, w));
                }
            }
            // Lines: underline, double underline's second line, strike-through.
            let has = [
                cell.flags & (UNDERLINE | DOUBLE_UNDERLINE) != 0,
                cell.flags & DOUBLE_UNDERLINE != 0,
                cell.flags & STRIKE != 0,
            ];
            for (k, run) in lines.iter_mut().enumerate() {
                let want = has[k].then_some(cell.fg);
                match (&mut *run, want) {
                    (Some((_, end, color)), Some(w)) if *end == c && *color == w => *end = c + span,
                    (run, want) => {
                        if let Some((s, e, color)) = run.take() {
                            over.push(rect(s, e, top as i32 + line_y[k].0, line_y[k].1, color));
                        }
                        *run = want.map(|w| (c, c + span, w));
                    }
                }
            }
            if cell.flags & COLOR != 0 {
                let text = frame.clusters.get(cell.ch).unwrap_or(" ");
                if let Some(g) = self.color.glyph(text, span as u8, m.cell_w, m.cell_h).map_err(|_| Full)? {
                    let slots = emoji.get_or_insert_with(|| vec![GlyphInst::default(); cols]);
                    slots[c] = GlyphInst { pos: [(c as i32 * m.cell_w) as i16, top], uv: [g.x, g.y], size: [g.w, g.h], color: [255, 255, 255, 255] };
                    continue;
                }
            }
            if cell.ch == ' ' || cell.ch == '\0' || cell.fg[3] == 0 {
                continue;
            }
            let style = (cell.flags & BOLD != 0) as u8 | ((cell.flags & ITALIC != 0) as u8) << 1;
            // A colour cluster no font could draw in colour: its first character, as text.
            let ch = if cell.flags & COLOR != 0 { frame.clusters.get(cell.ch).and_then(|t| t.chars().next()).unwrap_or(' ') } else { cell.ch };
            let g = self.atlas.glyph(ch, style).map_err(|_| Full)?;
            if g.w == 0 {
                continue;
            }
            glyphs[c] = GlyphInst {
                pos: [(c as i32 * m.cell_w + g.left as i32) as i16, top + (m.baseline - g.top as i32) as i16],
                uv: [g.x, g.y],
                size: [g.w, g.h],
                color: cell.fg,
            };
        }
        if let Some((s, e, color)) = bg_run {
            bg.push(rect(s, e, top as i32, m.cell_h, color));
        }
        for (k, run) in lines.into_iter().enumerate() {
            if let Some((s, e, color)) = run {
                over.push(rect(s, e, top as i32 + line_y[k].0, line_y[k].1, color));
            }
        }
        if let Some(cur) = frame.cursor.filter(|c| c.row == r && c.col < cols) {
            let x = cur.col as i32 * m.cell_w;
            let w = m.cell_w * if cur.wide { 2 } else { 1 };
            let thin = (m.cell_w / 8).max(1);
            let at = |x: i32, y: i32, w: i32, h: i32| RectInst {
                pos: [x as i16, (top as i32 + y) as i16],
                size: [w as u16, h as u16],
                color: cur.color,
            };
            match cur.shape {
                CursorShape::Beam => over.push(at(x, 0, thin.max(2), m.cell_h)),
                CursorShape::Underline => over.push(at(x, m.cell_h - thin.max(2), w, thin.max(2))),
                CursorShape::Hollow => {
                    over.push(at(x, 0, w, thin));
                    over.push(at(x, m.cell_h - thin, w, thin));
                    over.push(at(x, 0, thin, m.cell_h));
                    over.push(at(x + w - thin, 0, thin, m.cell_h));
                }
            }
        }
        bg.resize(cols, RectInst::default());
        over.truncate(cols * OVER_PER_COL);
        over.resize(cols * OVER_PER_COL, RectInst::default());
        Ok(RowUpdate { index: r, bg, glyphs, over, emoji })
    }

    /// The paint callback that draws the grid into `rect` (which should be whole pixels).
    pub fn paint(&self, rect: egui::Rect) -> egui::PaintCallback {
        let shared = self.shared.clone();
        egui::PaintCallback {
            rect,
            callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                let gl = painter.gl();
                let mut shared = shared.lock().unwrap();
                let shared = &mut *shared;
                if shared.gpu.is_none() && !shared.failed {
                    match Gpu::new(gl) {
                        Ok(gpu) => shared.gpu = Some(gpu),
                        Err(e) => {
                            eprintln!("terminal: the grid renderer could not start: {e}");
                            shared.failed = true;
                        }
                    }
                }
                let Some(gpu) = shared.gpu.as_mut() else { return };
                gpu.apply(gl, &mut shared.update);
                let view = info.viewport_in_pixels();
                gpu.draw(gl, [view.width_px as f32, view.height_px as f32]);
            })),
        }
    }

    /// Free the GPU objects (the GL context is going away).
    pub fn destroy(&self, gl: &glow::Context) {
        if let Some(gpu) = self.shared.lock().unwrap().gpu.take() {
            gpu.destroy(gl);
        }
    }
}

/// An atlas page is full: it was emptied, and every row is placed again.
struct Full;

/// A row's cells and, on the cursor's row, the cursor, as one number. Never 0.
fn row_hash(frame: &Frame, r: usize) -> u64 {
    let mut h = Fx(0);
    frame.row(r).hash(&mut h);
    if let Some(c) = frame.cursor.filter(|c| c.row == r) {
        c.hash(&mut h);
    }
    h.0 | 1
}

/// A small fast hasher (FxHash's step): the grid is hashed every frame there is new output.
struct Fx(u64);

impl Hasher for Fx {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut v = [0u8; 8];
            v[..chunk.len()].copy_from_slice(chunk);
            self.add(u64::from_le_bytes(v));
        }
    }
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
}

impl Fx {
    fn add(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Cell {
        const BLANK: Cell = Cell { ch: ' ', fg: [0; 4], bg: [0; 4], flags: 0 };
    }

    fn frame(cells: Vec<Cell>, cols: usize) -> Frame {
        Frame { cols, rows: cells.len() / cols, cells, cursor: None, blinking: false, clusters: Clusters::default() }
    }

    #[test]
    fn a_changed_cell_changes_only_its_rows_hash() {
        let a = Cell { ch: 'a', fg: [255; 4], ..Cell::BLANK };
        let mut f = frame(vec![a; 6], 3);
        let before = [row_hash(&f, 0), row_hash(&f, 1)];
        assert_eq!(before[0], before[1]);
        f.cells[4].ch = 'b';
        assert_eq!(row_hash(&f, 0), before[0]);
        assert_ne!(row_hash(&f, 1), before[1]);
    }

    #[test]
    fn clusters_keep_their_numbers() {
        let mut c = Clusters::default();
        let a = c.intern("\u{1F600}".into()).unwrap();
        let b = c.intern("\u{1F44D}\u{1F3FD}".into()).unwrap();
        assert_ne!(a, b);
        assert_eq!(c.intern("\u{1F600}".into()), Some(a));
        assert_eq!(c.get(b), Some("\u{1F44D}\u{1F3FD}"));
    }

    #[test]
    fn the_cursor_belongs_to_its_rows_hash() {
        let mut f = frame(vec![Cell::BLANK; 6], 3);
        let before = row_hash(&f, 1);
        f.cursor = Some(Cursor { col: 1, row: 1, shape: CursorShape::Beam, color: [255; 4], wide: false });
        assert_ne!(row_hash(&f, 1), before);
        assert_eq!(row_hash(&f, 0), row_hash(&frame(vec![Cell::BLANK; 6], 3), 0));
    }
}
