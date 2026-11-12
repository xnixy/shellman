//! Live window previews: Windows' compositor draws another window, kept up to date, into a
//! rectangle of one of ours. Nothing is copied or redrawn by us; the preview costs no CPU here.
//!
//! The compositor paints the preview above the window's own content, so whatever we draw under
//! its rectangle is hidden while it shows.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmQueryThumbnailSourceSize, DwmRegisterThumbnail, DwmUnregisterThumbnail,
    DwmUpdateThumbnailProperties, DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY,
    DWM_TNP_RECTDESTINATION, DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
};

/// A rectangle in physical pixels, relative to the top-left of the window it is drawn into.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PxRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// One window's live preview inside one of ours. Dropping it removes the preview.
pub struct LiveThumb {
    id: isize,
    source: isize,
}

impl LiveThumb {
    /// Preview `source` inside `dest`. Fails for windows the compositor can't preview (gone, or
    /// not a top-level window). Nothing shows until `show`.
    pub fn register(dest: isize, source: isize) -> Option<Self> {
        let id = unsafe {
            DwmRegisterThumbnail(HWND(dest as *mut _), HWND(source as *mut _)).ok()?
        };
        Some(Self { id, source })
    }

    /// The window being previewed.
    pub fn source(&self) -> isize {
        self.source
    }

    /// The previewed window's size in pixels, or None if it can't be asked.
    pub fn source_size(&self) -> Option<(f32, f32)> {
        let size = unsafe { DwmQueryThumbnailSourceSize(self.id) }.ok()?;
        (size.cx > 0 && size.cy > 0).then_some((size.cx as f32, size.cy as f32))
    }

    /// Draw the whole window, frame included, into `at`.
    pub fn show(&self, at: PxRect) {
        let props = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: DWM_TNP_RECTDESTINATION
                | DWM_TNP_VISIBLE
                | DWM_TNP_OPACITY
                | DWM_TNP_SOURCECLIENTAREAONLY,
            rcDestination: RECT {
                left: at.x.round() as i32,
                top: at.y.round() as i32,
                right: (at.x + at.w).round() as i32,
                bottom: (at.y + at.h).round() as i32,
            },
            opacity: 255,
            fVisible: true.into(),
            fSourceClientAreaOnly: false.into(),
            ..Default::default()
        };
        unsafe {
            let _ = DwmUpdateThumbnailProperties(self.id, &props);
        }
    }

    /// Stop drawing the preview but keep it registered.
    pub fn hide(&self) {
        let props = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: DWM_TNP_VISIBLE,
            fVisible: false.into(),
            ..Default::default()
        };
        unsafe {
            let _ = DwmUpdateThumbnailProperties(self.id, &props);
        }
    }
}

impl Drop for LiveThumb {
    fn drop(&mut self) {
        unsafe {
            let _ = DwmUnregisterThumbnail(self.id);
        }
    }
}

/// The largest rectangle with the source's shape that fits in `area`, centred, never scaled up
/// past the source's own size (a small dialog stays small rather than blurry).
pub fn fit(source: (f32, f32), area: PxRect) -> PxRect {
    let (sw, sh) = source;
    if sw <= 0.0 || sh <= 0.0 || area.w <= 0.0 || area.h <= 0.0 {
        return PxRect { x: area.x + area.w / 2.0, y: area.y + area.h / 2.0, w: 0.0, h: 0.0 };
    }
    let scale = (area.w / sw).min(area.h / sh).min(1.0);
    let (w, h) = (sw * scale, sh * scale);
    PxRect {
        x: area.x + (area.w - w) / 2.0,
        y: area.y + (area.h - h) / 2.0,
        w,
        h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: PxRect = PxRect { x: 10.0, y: 20.0, w: 200.0, h: 100.0 };

    #[test]
    fn wide_source_fills_the_width() {
        let r = fit((1600.0, 400.0), AREA);
        assert_eq!(r, PxRect { x: 10.0, y: 45.0, w: 200.0, h: 50.0 });
    }

    #[test]
    fn tall_source_fills_the_height() {
        let r = fit((500.0, 1000.0), AREA);
        assert_eq!(r, PxRect { x: 85.0, y: 20.0, w: 50.0, h: 100.0 });
    }

    #[test]
    fn small_source_keeps_its_size() {
        let r = fit((40.0, 20.0), AREA);
        assert_eq!(r, PxRect { x: 90.0, y: 60.0, w: 40.0, h: 20.0 });
    }

    #[test]
    fn empty_source_is_an_empty_rect() {
        assert_eq!(fit((0.0, 10.0), AREA).w, 0.0);
    }

    /// A window previewed into another of this process's windows: registers, reports the
    /// source's size, and unregisters on drop.
    #[test]
    fn previews_a_window() {
        use crate::winlist::tests::test_window;
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, WINDOW_EX_STYLE};
        let dest = test_window(w!("shellman thumb dest"), WINDOW_EX_STYLE(0));
        let src = test_window(w!("shellman thumb source"), WINDOW_EX_STYLE(0));
        let thumb = LiveThumb::register(dest.0 as isize, src.0 as isize).expect("registered");
        assert_eq!(thumb.source(), src.0 as isize);
        let (w, h) = thumb.source_size().expect("source size");
        assert!(w > 0.0 && h > 0.0);
        thumb.show(PxRect { x: 0.0, y: 0.0, w: 100.0, h: 50.0 });
        thumb.hide();
        drop(thumb);
        unsafe {
            let _ = DestroyWindow(src);
            let _ = DestroyWindow(dest);
        }
    }
}
