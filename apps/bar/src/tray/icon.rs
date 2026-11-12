//! Turning an app's tray icon handle into pixels the bar can draw.
//!
//! Apps destroy or replace their icons whenever they like, so each one is converted as soon as
//! it arrives and the handle is never kept.

use egui::ColorImage;
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
};
use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, HICON, ICONINFO};

/// The icon's pixels, or None for a handle that is gone or a black-and-white icon.
pub fn to_image(icon: isize) -> Option<ColorImage> {
    let mut info = ICONINFO::default();
    unsafe { GetIconInfo(HICON(icon as *mut _), &mut info) }.ok()?;
    let image = if info.hbmColor.is_invalid() {
        None
    } else {
        pixels(info.hbmColor).map(|(w, h, mut bgra)| {
            // Icons without an alpha channel are opaque except where their mask says otherwise.
            if bgra.chunks_exact(4).all(|p| p[3] == 0) {
                let mask = pixels(info.hbmMask).map(|(_, _, m)| m);
                for (i, p) in bgra.chunks_exact_mut(4).enumerate() {
                    let transparent = mask.as_ref().is_some_and(|m| m.get(i * 4).is_some_and(|&v| v != 0));
                    p[3] = if transparent { 0 } else { 255 };
                }
            }
            for p in bgra.chunks_exact_mut(4) {
                p.swap(0, 2);
            }
            ColorImage::from_rgba_unmultiplied([w, h], &bgra)
        })
    };
    unsafe {
        let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
        let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
    }
    image
}

/// A bitmap's pixels as top-down 32-bit BGRA.
fn pixels(bitmap: HBITMAP) -> Option<(usize, usize, Vec<u8>)> {
    let mut bm = BITMAP::default();
    let got = unsafe { GetObjectW(HGDIOBJ(bitmap.0), size_of::<BITMAP>() as i32, Some((&raw mut bm).cast())) };
    let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
    if got == 0 || w <= 0 || h <= 0 || w > 256 || h > 256 {
        return None;
    }
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut buf = vec![0u8; (w * h * 4) as usize];
    unsafe {
        let dc = GetDC(None);
        let lines = GetDIBits(dc, bitmap, 0, h as u32, Some(buf.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        (lines != 0).then_some((w as usize, h as usize, buf))
    }
}
