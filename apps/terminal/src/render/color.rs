//! Colour glyphs (emoji): drawn by Direct2D with DirectWrite's colour fonts (Segoe UI Emoji's
//! colour layers) into a premultiplied BGRA bitmap, software-rendered through WIC (no GPU device,
//! no window), and kept on a colour atlas page of their own. Made the first time an emoji shows,
//! so a screen without one costs nothing.

use std::collections::HashMap;
use windows::core::w;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1RenderTarget, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap, IWICImagingFactory, WICBitmapCacheOnLoad};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};

/// The colour page is a square of this many pixels a side (BGRA).
pub const COLOR_ATLAS: i32 = 1024;
const PAD: i32 = 1;

/// Where a colour glyph is on its page.
#[derive(Clone, Copy, Default, Debug)]
pub struct ColorGlyph {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// Pixels for the colour page: x, y, w, h and four bytes a pixel (premultiplied BGRA).
pub struct Upload {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub pixels: Vec<u8>,
}

/// Direct2D drawing onto one WIC bitmap the size of a glyph box.
struct Painter {
    _factory: ID2D1Factory,
    wic: IWICImagingFactory,
    d2d: ID2D1Factory,
    format: IDWriteTextFormat,
    /// The bitmap and its target, for a box of this size.
    target: Option<((i32, i32), IWICBitmap, ID2D1RenderTarget)>,
}

impl Painter {
    fn new(em_px: f32) -> windows::core::Result<Self> {
        unsafe {
            // winit has set COM up on this thread already; this is a no-op then.
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let format = dw.CreateTextFormat(
                w!("Segoe UI Emoji"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                em_px,
                w!("en-us"),
            )?;
            format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            Ok(Self { _factory: d2d.clone(), wic, d2d, format, target: None })
        }
    }

    /// `text` drawn centred in a `w` x `h` box: premultiplied BGRA, row by row.
    fn paint(&mut self, text: &str, w: i32, h: i32) -> windows::core::Result<Vec<u8>> {
        unsafe {
            if self.target.as_ref().is_none_or(|(size, _, _)| *size != (w, h)) {
                let bitmap = self.wic.CreateBitmap(w as u32, h as u32, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)?;
                let props = D2D1_RENDER_TARGET_PROPERTIES {
                    r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                    pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    usage: D2D1_RENDER_TARGET_USAGE_NONE,
                    minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
                };
                let rt = self.d2d.CreateWicBitmapRenderTarget(&bitmap, &props)?;
                rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
                self.target = Some(((w, h), bitmap, rt));
            }
            let (_, bitmap, rt) = self.target.as_ref().unwrap();
            let brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)?;
            let utf16: Vec<u16> = text.encode_utf16().collect();
            rt.BeginDraw();
            rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            let rect = D2D_RECT_F { left: 0.0, top: 0.0, right: w as f32, bottom: h as f32 };
            rt.DrawText(&utf16, &self.format, &rect, &brush, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, DWRITE_MEASURING_MODE_NATURAL);
            rt.EndDraw(None, None)?;
            let stride = w as u32 * 4;
            let mut pixels = vec![0u8; (stride * h as u32) as usize];
            bitmap.CopyPixels(std::ptr::null(), stride, &mut pixels)?;
            Ok(pixels)
        }
    }
}

/// The colour page: clusters (an emoji with its joiners, modifiers, selectors) to places on it.
pub struct ColorAtlas {
    painter: Option<Painter>,
    failed: bool,
    em_px: f32,
    glyphs: HashMap<(String, u8), ColorGlyph>,
    cursor: (i32, i32),
    shelf: i32,
    pub uploads: Vec<Upload>,
}

/// The page is full: it was emptied, and everything placed before must be placed again.
pub struct Full;

impl ColorAtlas {
    /// For glyphs `em_px` pixels to the em (the grid font's size).
    pub fn new(em_px: f32) -> Self {
        Self { painter: None, failed: false, em_px, glyphs: HashMap::new(), cursor: (PAD, PAD), shelf: 0, uploads: Vec::new() }
    }

    /// The glyph for `text` in a box of `cells` cells of `cell_w` x `cell_h`; None when it cannot
    /// be drawn (it is then drawn as text).
    pub fn glyph(&mut self, text: &str, cells: u8, cell_w: i32, cell_h: i32) -> Result<Option<ColorGlyph>, Full> {
        let key = (text.to_owned(), cells);
        if let Some(g) = self.glyphs.get(&key) {
            return Ok(Some(*g));
        }
        if self.failed {
            return Ok(None);
        }
        if self.painter.is_none() {
            // Emoji are a little wider than tall: the size fits two cells across and the cell's
            // height.
            let em = self.em_px.min(cell_h as f32 * 0.92).min(2.0 * cell_w as f32 / 1.2);
            match Painter::new(em) {
                Ok(p) => self.painter = Some(p),
                Err(e) => {
                    eprintln!("terminal: colour glyphs are off: {e}");
                    self.failed = true;
                    return Ok(None);
                }
            }
        }
        let (w, h) = (cell_w * cells as i32, cell_h);
        let pixels = match self.painter.as_mut().unwrap().paint(text, w, h) {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };
        // Nothing drawn (no font has it): text instead.
        if pixels.chunks_exact(4).all(|p| p[3] == 0) {
            return Ok(None);
        }
        let (x, y) = self.place(w, h)?;
        self.uploads.push(Upload { x, y, w, h, pixels });
        let g = ColorGlyph { x: x as u16, y: y as u16, w: w as u16, h: h as u16 };
        self.glyphs.insert(key, g);
        Ok(Some(g))
    }

    fn place(&mut self, w: i32, h: i32) -> Result<(i32, i32), Full> {
        if w + 2 * PAD > COLOR_ATLAS || h + 2 * PAD > COLOR_ATLAS {
            return Err(Full);
        }
        if self.cursor.0 + w + PAD > COLOR_ATLAS {
            self.cursor = (PAD, self.cursor.1 + self.shelf + PAD);
            self.shelf = 0;
        }
        if self.cursor.1 + h + PAD > COLOR_ATLAS {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Segoe UI Emoji draws in colour: a grinning face has pixels that are not grey, and a
    /// family's ZWJ sequence is one glyph (it fits the two-cell box it is given).
    #[test]
    fn emoji_come_out_in_colour() {
        let mut atlas = ColorAtlas::new(20.0);
        let g = atlas.glyph("\u{1F600}", 2, 12, 24).ok().flatten().expect("drawn");
        assert_eq!((g.w, g.h), (24, 24));
        let px = &atlas.uploads[0].pixels;
        let coloured = px.chunks_exact(4).filter(|p| p[3] > 128 && (p[0] as i32 - p[2] as i32).abs() > 40).count();
        assert!(coloured > 20, "only {coloured} coloured pixels");
        let family = atlas.glyph("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}", 2, 12, 24).ok().flatten();
        assert!(family.is_some());
        // Joined, the family is one emoji's width; three side by side would fill the box edge
        // to edge (and beyond).
        let ink = |px: &[u8], w: usize| {
            let cols: Vec<usize> = (0..w).filter(|&x| px.chunks_exact(4).skip(x).step_by(w).any(|p| p[3] > 32)).collect();
            cols.last().unwrap() - cols.first().unwrap() + 1
        };
        let one = ink(&atlas.uploads[0].pixels, 24);
        let joined = ink(&atlas.uploads[1].pixels, 24);
        assert!(joined <= one + 3 && joined < 24, "family ink {joined} px against one emoji's {one}");
        // Asking again costs nothing: the same place, nothing new to send.
        let n = atlas.uploads.len();
        atlas.glyph("\u{1F600}", 2, 12, 24).ok();
        assert_eq!(atlas.uploads.len(), n);
    }
}
