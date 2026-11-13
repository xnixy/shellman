//! Glyphs: rasterized by DirectWrite (through `crossfont`, with Windows' own font fallback for
//! what the main font lacks), once each, into one greyscale atlas the GPU draws from.
//!
//! DirectWrite hands back ClearType coverage (three values a pixel). The grid sits on
//! translucent glass, where the colour under a pixel is only known once DWM composes the window,
//! so subpixel coverage cannot be blended correctly; the three are averaged into one greyscale
//! coverage instead.

use crossfont::{BitmapBuffer, FontDesc, FontKey, GlyphKey, Rasterize, Rasterizer, Size, Slant, Style, Weight};
use std::collections::HashMap;

/// The atlas is a square of this many pixels a side.
pub const ATLAS: i32 = 2048;
/// Space between glyphs, so sampling never bleeds into a neighbour.
const PAD: i32 = 1;

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

/// The atlas is full: it was emptied, and everything placed before must be placed again.
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
}

impl Atlas {
    /// Load the grid font at `px` pixels per em.
    pub fn new(px: f32) -> Result<Self, String> {
        let mut raster = Rasterizer::new().map_err(|e| e.to_string())?;
        let size = Size::from_px(px);
        let face = |raster: &mut Rasterizer, family: &str, weight, slant| {
            raster.load_font(&FontDesc::new(family, Style::Description { slant, weight }), size)
        };
        let (family, regular): (&str, _) = FAMILIES
            .iter()
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
        Ok(Self {
            raster,
            fonts: [regular, bold, italic, bold_italic],
            size,
            metrics,
            glyphs: HashMap::new(),
            cursor: (PAD, PAD),
            shelf: 0,
            uploads: Vec::new(),
        })
    }

    /// The glyph for `c` in face `style` (0 to 3), rasterized and placed the first time.
    pub fn glyph(&mut self, c: char, style: u8) -> Result<Glyph, Full> {
        if let Some(g) = self.glyphs.get(&(c, style)) {
            return Ok(*g);
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

    /// A free spot of `w` by `h`, on the current shelf or a new one. A full atlas is emptied.
    fn place(&mut self, w: i32, h: i32) -> Result<(i32, i32), Full> {
        if w + 2 * PAD > ATLAS || h + 2 * PAD > ATLAS {
            return Err(Full);
        }
        if self.cursor.0 + w + PAD > ATLAS {
            self.cursor = (PAD, self.cursor.1 + self.shelf + PAD);
            self.shelf = 0;
        }
        if self.cursor.1 + h + PAD > ATLAS {
            self.glyphs.clear();
            self.uploads.clear();
            self.cursor = (PAD, PAD);
            self.shelf = 0;
            return Err(Full);
        }
        let at = self.cursor;
        self.cursor.0 += w + PAD;
        self.shelf = self.shelf.max(h);
        Ok(at)
    }
}
