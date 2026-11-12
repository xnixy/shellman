//! Pictures out of a Windows menu without showing it: an entry's bitmap as RGBA, and entries the
//! extension draws itself (owner-drawn rows, and icons left to a draw callback) rendered into an
//! off-screen bitmap through the same messages a shown menu would send.

use super::model::Icon;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontIndirectW, CreateSolidBrush, DeleteDC,
    DeleteObject, FillRect, GdiFlush, SelectObject, SetBkColor, SetBkMode, SetTextColor,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HFONT, HGDIOBJ,
    TRANSPARENT,
};
use windows::Win32::UI::Controls::{
    DRAWITEMSTRUCT, DRAWITEMSTRUCT_CTL_TYPE, MEASUREITEMSTRUCT, ODA_DRAWENTIRE, ODS_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, HMENU, NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

const WM_DRAWITEM: u32 = 0x002B;
const WM_MEASUREITEM: u32 = 0x002C;
const ODT_MENU: DRAWITEMSTRUCT_CTL_TYPE = DRAWITEMSTRUCT_CTL_TYPE(1);
/// Owner-drawn rows bigger than this are not menu rows; something is off, so they are skipped.
const MAX_W: u32 = 800;
const MAX_H: u32 = 120;

/// A menu entry's own bitmap (`hbmpItem`, or the checkmark bitmaps some old handlers use for an
/// icon) as RGBA. The small handle values Windows reserves for system glyphs (and -1, "drawn on
/// request") are not bitmaps. Real handles are 32-bit values sign-extended, so many are negative.
pub(super) fn bitmap(hbmp: HBITMAP) -> Option<Icon> {
    let v = hbmp.0 as isize;
    if v == -1 || (0..=11).contains(&v) {
        return None;
    }
    let img = unsafe { crate::thumbs::to_color_image(hbmp) }?;
    let rgba: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
    Some(Icon::new(img.size, rgba))
}

/// The colours the menu is drawn in, as COLORREFs (0x00BBGGRR).
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub text: u32,
    pub background: u32,
}

/// Renders entries the extension draws itself. One per walk: it holds a memory DC and the menu
/// font.
pub(super) struct Canvas {
    dc: HDC,
    font: HFONT,
    palette: Palette,
}

impl Canvas {
    pub(super) fn new(palette: Palette) -> Option<Self> {
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return None;
            }
            let mut metrics = NONCLIENTMETRICSW {
                cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
                ..Default::default()
            };
            let font = if SystemParametersInfoW(
                SPI_GETNONCLIENTMETRICS,
                metrics.cbSize,
                Some((&raw mut metrics).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
            .is_ok()
            {
                CreateFontIndirectW(&metrics.lfMenuFont)
            } else {
                HFONT::default()
            };
            Some(Self { dc, font, palette })
        }
    }

    /// Ask the extension how big the entry is, then have it draw the entry twice, on black and on
    /// white: where the two differ the pixel is see-through, so text and icons come out with
    /// their real edges over Files' glass. `send` passes one message to the menu's handler.
    pub(super) fn render(
        &self,
        send: &dyn Fn(u32, WPARAM, LPARAM) -> Option<LRESULT>,
        hmenu: HMENU,
        cmd: u32,
        data: usize,
    ) -> Option<Icon> {
        let mut measure = MEASUREITEMSTRUCT {
            CtlType: ODT_MENU,
            itemID: cmd,
            itemData: data,
            ..Default::default()
        };
        send(WM_MEASUREITEM, WPARAM(0), LPARAM(&raw mut measure as isize))?;
        let (w, h) = (measure.itemWidth, measure.itemHeight);
        if w == 0 || h == 0 || w > MAX_W || h > MAX_H {
            return None;
        }
        let dark = self.pass(send, hmenu, cmd, data, w, h, 0x000000)?;
        let light = self.pass(send, hmenu, cmd, data, w, h, 0xFFFFFF)?;
        let rgba = unmix(&dark, &light);
        // Nothing drawn at all is no picture.
        if rgba.chunks_exact(4).all(|p| p[3] == 0) {
            return None;
        }
        Some(Icon::new([w as usize, h as usize], rgba))
    }

    /// One drawing of the entry over `fill`, as BGRA bytes.
    #[allow(clippy::too_many_arguments)]
    fn pass(
        &self,
        send: &dyn Fn(u32, WPARAM, LPARAM) -> Option<LRESULT>,
        hmenu: HMENU,
        cmd: u32,
        data: usize,
        w: u32,
        h: u32,
        fill: u32,
    ) -> Option<Vec<u8>> {
        unsafe {
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(h as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let dib = CreateDIBSection(Some(self.dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
                .ok()?;
            let old_bmp = SelectObject(self.dc, HGDIOBJ(dib.0));
            let old_font = (!self.font.is_invalid()).then(|| SelectObject(self.dc, HGDIOBJ(self.font.0)));
            let rect = RECT { left: 0, top: 0, right: w as i32, bottom: h as i32 };
            let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(fill));
            FillRect(self.dc, &rect, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));
            SetBkMode(self.dc, TRANSPARENT);
            SetBkColor(self.dc, windows::Win32::Foundation::COLORREF(self.palette.background));
            SetTextColor(self.dc, windows::Win32::Foundation::COLORREF(self.palette.text));
            let mut draw = DRAWITEMSTRUCT {
                CtlType: ODT_MENU,
                itemID: cmd,
                itemAction: ODA_DRAWENTIRE,
                itemState: ODS_FLAGS(0),
                hwndItem: HWND(hmenu.0),
                hDC: self.dc,
                rcItem: rect,
                itemData: data,
                ..Default::default()
            };
            let drew = send(WM_DRAWITEM, WPARAM(0), LPARAM(&raw mut draw as isize)).is_some();
            let _ = GdiFlush();
            let out = drew.then(|| {
                std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize).to_vec()
            });
            if let Some(f) = old_font {
                SelectObject(self.dc, f);
            }
            SelectObject(self.dc, old_bmp);
            let _ = DeleteObject(HGDIOBJ(dib.0));
            out
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            if !self.font.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(self.font.0));
            }
            let _ = DeleteDC(self.dc);
        }
    }
}

/// Recovers colour and coverage from one drawing on black and one on white (BGRA bytes). Over
/// black a pixel is its colour times its coverage, which is exactly premultiplied colour; how far
/// white shows through gives the coverage.
pub(super) fn unmix(on_black: &[u8], on_white: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(on_black.len());
    for (b, w) in on_black.chunks_exact(4).zip(on_white.chunks_exact(4)) {
        let show: u32 = (0..3).map(|i| u32::from(w[i].saturating_sub(b[i]))).sum::<u32>() / 3;
        let alpha = 255 - show.min(255) as u8;
        // BGRA in, RGBA out; colour never above coverage, as premultiplied requires.
        out.extend_from_slice(&[b[2].min(alpha), b[1].min(alpha), b[0].min(alpha), alpha]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_backgrounds_give_colour_and_coverage() {
        // An opaque red pixel, a see-through one, and white text at half coverage (BGRA).
        let on_black = [0, 0, 200, 0, 0, 0, 0, 0, 128, 128, 128, 0];
        let on_white = [0, 0, 200, 0, 255, 255, 255, 0, 255, 255, 255, 0];
        let out = unmix(&on_black, &on_white);
        assert_eq!(&out[0..4], &[200, 0, 0, 255]);
        assert_eq!(&out[4..8], &[0, 0, 0, 0]);
        assert_eq!(out[11], 128);
        assert_eq!(&out[8..11], &[128, 128, 128]);
    }
}
