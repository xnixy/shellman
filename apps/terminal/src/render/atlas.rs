//! Glyphs: rasterized by DirectWrite (through `crossfont`, with Windows' own font fallback for
//! what the main font lacks), once each, into one greyscale atlas the GPU draws from.
//!
//! DirectWrite hands back ClearType coverage (three values a pixel). The grid sits on
//! translucent glass, where the colour under a pixel is only known once DWM composes the window,
//! so subpixel coverage cannot be blended correctly; the three are averaged into one greyscale
//! coverage instead.

use crossfont::{BitmapBuffer, FontDesc, FontKey, GlyphKey, Rasterize, Rasterizer, Size, Slant, Style, Weight};
use std::collections::HashMap;

/// The atlas is a square of at least this many pixels a side (more for a large font).
pub const ATLAS: i32 = 2048;
/// Space between glyphs, so sampling never bleeds into a neighbour.
pub const PAD: i32 = 1;
/// How many cell-sized glyphs the greyscale page is sized to hold at least.
const CELLS_HELD: usize = 1024;

/// The side of a square page for `boxes` boxes of `w` x `h` (shelf-packed), a power of two, at
/// least `min` and at most `cap` (the GPU's largest texture, or less).
pub fn page_side(w: i32, h: i32, boxes: usize, min: i32, cap: i32) -> i32 {
    let need = ((boxes as f64) * ((w + PAD) as f64) * ((h + PAD) as f64)).sqrt().ceil() as i32 + PAD;
    let side = (need.max(1) as u32).next_power_of_two() as i32;
    side.max(min).min(cap.max(64))
}

/// How many boxes of `w` x `h` a page of `side` holds, shelf-packed.
pub fn capacity(side: i32, w: i32, h: i32) -> usize {
    let across = (side - PAD) / (w + PAD);
    let down = (side - PAD) / (h + PAD);
    (across.max(0) * down.max(0)) as usize
}

/// Families tried in order for the grid's font.
const FAMILIES: [&str; 3] = ["Cascadia Mono", "Cascadia Code", "Consolas"];

/// Regular, bold, italic, bold italic: the four faces a cell can ask for.
pub const STYLES: usize = 4;

/// Where a glyph is in the atlas, and where it sits relative to its cell's baseline.
#[derive(Clone, Copy, Default, Debug)]
pub struct Glyph {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    /// From the cell's left edge to the bitmap's.
    pub left: i16,
    /// From the baseline up to the bitmap's top.
    pub top: i16,
}

/// The grid font's measurements, in whole pixels.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub cell_w: i32,
    pub cell_h: i32,
    /// From the cell's top down to the baseline.
    pub baseline: i32,
    /// Top of the underline below the baseline, and its thickness.
    pub underline: i32,
    pub underline_thick: i32,
    /// Top of the strike-through line above the baseline, and its thickness.
    pub strike: i32,
    pub strike_thick: i32,
}

/// A rectangle of the atlas to send to the GPU: x, y, w, h and one byte per pixel.
pub struct Upload {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub pixels: Vec<u8>,
}

/// The atlas has no room for a glyph. Nothing is evicted: the renderer decides whether to start
/// the page over ([`Atlas::reset`]) or draw the glyph as [`Atlas::fallback`].
#[derive(Debug)]
pub struct Full;

pub struct Atlas {
    raster: Rasterizer,
    fonts: [FontKey; STYLES],
    size: Size,
    pub metrics: Metrics,
    glyphs: HashMap<(char, u8), Glyph>,
    /// Shelf packing: where the next glyph goes, and the current shelf's height.
    cursor: (i32, i32),
    shelf: i32,
    pub uploads: Vec<Upload>,
    /// The page's side in pixels (from the cell's size, see [`page_side`]).
    pub side: i32,
    /// A cell-sized box outline, always on the page: what a glyph that finds no room is drawn as.
    pub fallback: Glyph,
}

impl Atlas {
    /// Load the grid font at `px` pixels per em: `family` if it is installed (and not empty),
    /// else the first of [`FAMILIES`] that is.
    pub fn new(px: f32, family: &str, cap: i32) -> Result<Self, String> {
        let mut raster = Rasterizer::new().map_err(|e| e.to_string())?;
        let size = Size::from_px(px);
        let face = |raster: &mut Rasterizer, family: &str, weight, slant| {
            raster.load_font(&FontDesc::new(family, Style::Description { slant, weight }), size)
        };
        let wanted = (!family.trim().is_empty()).then_some(family.trim());
        let (family, regular): (&str, _) = wanted
            .iter()
            .chain(FAMILIES.iter())
            .find_map(|f| face(&mut raster, f, Weight::Normal, Slant::Normal).ok().map(|k| (*f, k)))
            .ok_or("no monospace font (Cascadia Mono, Consolas) is installed")?;
        // DirectWrite picks the nearest face; a family without bold gets its regular.
        let mut other = |weight, slant| face(&mut raster, family, weight, slant).unwrap_or(regular);
        let bold = other(Weight::Bold, Slant::Normal);
        let italic = other(Weight::Normal, Slant::Italic);
        let bold_italic = other(Weight::Bold, Slant::Italic);
        let m = raster.metrics(regular, size).map_err(|e| e.to_string())?;
        let cell_w = (m.average_advance.round() as i32).max(1);
        let cell_h = (m.line_height.round() as i32).max(1);
        // Any line gap goes above the text (Cascadia and Consolas have none).
        let baseline = (cell_h as f32 + m.descent).round() as i32;
        let thick = |t: f32| (t.round() as i32).max(1);
        let metrics = Metrics {
            cell_w,
            cell_h,
            baseline,
            underline: (-m.underline_position).round().max(1.0) as i32,
            underline_thick: thick(m.underline_thickness),
            strike: (m.strikeout_position + m.strikeout_thickness / 2.0).round() as i32,
            strike_thick: thick(m.strikeout_thickness),
        };
        let mut atlas = Self {
            raster,
            fonts: [regular, bold, italic, bold_italic],
            size,
            metrics,
            glyphs: HashMap::new(),
            cursor: (PAD, PAD),
            shelf: 0,
            uploads: Vec::new(),
            side: page_side(cell_w, cell_h, CELLS_HELD, ATLAS, cap),
            fallback: Glyph::default(),
        };
        atlas.reset();
        Ok(atlas)
    }

    /// Start the page over: every glyph is forgotten (and placed again when next asked for),
    /// and the fallback box is placed first.
    pub fn reset(&mut self) {
        self.glyphs.clear();
        self.uploads.clear();
        self.cursor = (PAD, PAD);
        self.shelf = 0;
        let m = self.metrics;
        let (w, h) = (m.cell_w, m.cell_h);
        self.fallback = match self.place(w, h) {
            Ok((x, y)) => {
                let t = (w / 10).max(1);
                let (x0, x1, y0, y1) = (w / 8, w - w / 8, h / 5, h - h / 5);
                let pixels = (0..h)
                    .flat_map(|py| (0..w).map(move |px| (px, py)))
                    .map(|(px, py)| {
                        let inside = px >= x0 && px < x1 && py >= y0 && py < y1;
                        let edge = px < x0 + t || px >= x1 - t || py < y0 + t || py >= y1 - t;
                        if inside && edge { 255 } else { 0 }
                    })
                    .collect();
                self.uploads.push(Upload { x, y, w, h, pixels });
                Glyph { x: x as u16, y: y as u16, w: w as u16, h: h as u16, left: 0, top: m.baseline as i16 }
            }
            Err(Full) => Glyph::default(),
        };
    }

    /// How many cell-sized glyphs the page holds.
    pub fn capacity(&self) -> usize {
        capacity(self.side, self.metrics.cell_w, self.metrics.cell_h)
    }

    /// The glyph for `c` in face `style` (0 to 3), rasterized and placed the first time.
    pub fn glyph(&mut self, c: char, style: u8) -> Result<Glyph, Full> {
        if let Some(g) = self.glyphs.get(&(c, style)) {
            return Ok(*g);
        }
        // Lines and blocks: drawn to fill the cell exactly, so neighbours join.
        let m = self.metrics;
        if let Some(pixels) = super::boxdraw::draw(c, m.cell_w as usize, m.cell_h as usize, m.underline_thick.max(1) as usize) {
            let (x, y) = self.place(m.cell_w, m.cell_h)?;
            self.uploads.push(Upload { x, y, w: m.cell_w, h: m.cell_h, pixels });
            let glyph = Glyph { x: x as u16, y: y as u16, w: m.cell_w as u16, h: m.cell_h as u16, left: 0, top: m.baseline as i16 };
            self.glyphs.insert((c, style), glyph);
            return Ok(glyph);
        }
        let key = GlyphKey { character: c, font_key: self.fonts[style as usize & 3], size: self.size };
        let raster = match self.raster.get_glyph(key) {
            Ok(g) => g,
            // No font has it: DirectWrite's missing-glyph box, like other terminals show.
            Err(crossfont::Error::MissingGlyph(g)) => g,
            Err(_) => {
                self.glyphs.insert((c, style), Glyph::default());
                return Ok(Glyph::default());
            }
        };
        let (w, h) = (raster.width, raster.height);
        let mut glyph = Glyph { left: raster.left as i16, top: raster.top as i16, ..Glyph::default() };
        if w > 0 && h > 0 {
            let (x, y) = self.place(w, h)?;
            let pixels = match raster.buffer {
                BitmapBuffer::Rgb(rgb) => {
                    rgb.chunks_exact(3).map(|p| ((p[0] as u16 + p[1] as u16 + p[2] as u16) / 3) as u8).collect()
                }
                // Colour glyphs (DirectWrite through crossfont gives none today): their alpha.
                BitmapBuffer::Rgba(rgba) => rgba.chunks_exact(4).map(|p| p[3]).collect(),
            };
            self.uploads.push(Upload { x, y, w, h, pixels });
            glyph = Glyph { x: x as u16, y: y as u16, w: w as u16, h: h as u16, ..glyph };
        }
        self.glyphs.insert((c, style), glyph);
        Ok(glyph)
    }

    /// A free spot of `w` by `h`, on the current shelf or a new one; Full when there is none
    /// (the page is left as it is).
    fn place(&mut self, w: i32, h: i32) -> Result<(i32, i32), Full> {
        place(&mut self.cursor, &mut self.shelf, self.side, w, h)
    }
}

/// Shelf packing on a page of `side`: where a `w` x `h` box goes, moving `cursor` and `shelf`
/// on; Full (and nothing moved) when it does not fit.
pub fn place(cursor: &mut (i32, i32), shelf: &mut i32, side: i32, w: i32, h: i32) -> Result<(i32, i32), Full> {
    if w + 2 * PAD > side || h + 2 * PAD > side {
        return Err(Full);
    }
    let (mut at, mut height) = (*cursor, *shelf);
    if at.0 + w + PAD > side {
        at = (PAD, at.1 + height + PAD);
        height = 0;
    }
    if at.1 + h + PAD > side {
        return Err(Full);
    }
    *cursor = (at.0 + w + PAD, at.1);
    *shelf = height.max(h);
    Ok(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_grow_with_the_cell_up_to_the_cap() {
        // An ordinary font: the old 2048 page.
        assert_eq!(page_side(11, 24, CELLS_HELD, ATLAS, 4096), 2048);
        // 36 pt on a 300% monitor: bigger, so a screen's worth fits.
        let big = page_side(80, 171, CELLS_HELD, ATLAS, 4096);
        assert_eq!(big, 4096);
        assert!(capacity(big, 80, 171) >= 1000);
        // Never past what the GPU takes.
        assert_eq!(page_side(80, 171, CELLS_HELD, ATLAS, 2048), 2048);
    }

    #[test]
    fn a_full_page_is_left_as_it_is() {
        let (mut cursor, mut shelf) = ((PAD, PAD), 0);
        let mut placed = 0;
        while place(&mut cursor, &mut shelf, 64, 10, 20).is_ok() {
            placed += 1;
        }
        assert_eq!(placed, capacity(64, 10, 20));
        let before = (cursor, shelf);
        assert!(place(&mut cursor, &mut shelf, 64, 10, 20).is_err());
        assert_eq!((cursor, shelf), before, "a refusal moves nothing");
        // A smaller box may still fit at the end of the shelf.
        assert!(place(&mut cursor, &mut shelf, 64, 1, 1).is_ok());
    }
}
