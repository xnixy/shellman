//! Where the Files window opens: the last place it was, if that is still fully on a monitor's
//! work area, else centred on the work area of the monitor under the pointer. All in physical
//! pixels, so negative-x monitors and mixed scales need no special cases.

use serde::{Deserialize, Serialize};

/// The window size a first start asks for, in points.
pub const DEFAULT_SIZE: (f32, f32) = (1100.0, 700.0);
/// The smallest window a saved rectangle may describe, in points (the viewport's minimum).
/// The layout needs this much room for its parts (see `app::plan`).
pub const MIN_SIZE: (f32, f32) = (760.0, 480.0);

/// A rectangle in physical pixels, edges as in a Win32 `RECT`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    fn contains(&self, inner: &Rect) -> bool {
        inner.left >= self.left
            && inner.top >= self.top
            && inner.right <= self.right
            && inner.bottom <= self.bottom
    }
}

/// The window's rectangle: `saved` when it lies fully within one of the `works` (the work areas of
/// every connected monitor) and is not absurdly small; otherwise the default size centred in
/// `work`, the work area of the monitor to open on, whose scale is `scale`. (The saved rectangle's
/// own monitor may have another scale, so only the minimum in points, as pixels, is checked; the
/// window's minimum size does the rest.)
pub fn place(work: Rect, scale: f32, saved: Option<Rect>, works: &[Rect]) -> Rect {
    if let Some(s) = saved {
        let big_enough = s.width() as f32 >= MIN_SIZE.0 && s.height() as f32 >= MIN_SIZE.1;
        if big_enough && works.iter().any(|w| w.contains(&s)) {
            return s;
        }
    }
    let w = ((DEFAULT_SIZE.0 * scale) as i32).min(work.width());
    let h = ((DEFAULT_SIZE.1 * scale) as i32).min(work.height());
    let left = work.left + (work.width() - w) / 2;
    let top = work.top + (work.height() - h) / 2;
    Rect { left, top, right: left + w, bottom: top + h }
}

/// The picker's size, in points: smaller than a browsing window, as a dialog is.
pub const PICK_SIZE: (f32, f32) = (920.0, 600.0);

/// Where the picker goes: `size` pixels centred over `owner` (the asking app's window) when there
/// is one, else centred in `work`; moved and cut to stay within `work` either way.
pub fn centre_over(owner: Option<Rect>, work: Rect, size: (i32, i32)) -> Rect {
    let w = size.0.min(work.width());
    let h = size.1.min(work.height());
    let (cx, cy) = match owner {
        Some(o) => ((o.left + o.right) / 2, (o.top + o.bottom) / 2),
        None => ((work.left + work.right) / 2, (work.top + work.bottom) / 2),
    };
    let left = (cx - w / 2).clamp(work.left, work.right - w);
    let top = (cy - h / 2).clamp(work.top, work.bottom - h);
    Rect { left, top, right: left + w, bottom: top + h }
}

#[cfg(windows)]
pub use win::{aware, monitor_of, move_to, pointer_monitor, scale_at, window_rect, work_areas};

#[cfg(windows)]
mod win {
    use super::Rect;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, MonitorFromRect, HDC, HMONITOR,
        MONITORINFO,
        MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::HiDpi::{
        GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        MDT_EFFECTIVE_DPI,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetWindowRect, IsZoomed, SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
    };

    fn rect(r: RECT) -> Rect {
        Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
    }

    /// Make this process per-monitor DPI aware, so the numbers below are real pixels. The window
    /// library asks for the same later; the first call wins and the second is refused harmlessly.
    pub fn aware() {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
    }

    fn scale(monitor: HMONITOR) -> f32 {
        let (mut dx, mut dy) = (96, 96);
        unsafe {
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        }
        dx as f32 / 96.0
    }

    /// The work area and scale of the monitor under the pointer: where the click that started
    /// this process happened.
    pub fn pointer_monitor() -> Option<(Rect, f32)> {
        aware();
        unsafe {
            let mut at = POINT::default();
            GetCursorPos(&mut at).ok()?;
            let monitor = MonitorFromPoint(at, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return None;
            }
            Some((rect(info.rcWork), scale(monitor)))
        }
    }

    /// The work area and scale of the monitor that holds most of `r`.
    pub fn monitor_of(r: Rect) -> Option<(Rect, f32)> {
        aware();
        let at = RECT { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
        unsafe {
            let monitor = MonitorFromRect(&at, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return None;
            }
            Some((rect(info.rcWork), scale(monitor)))
        }
    }

    /// The scale of the monitor holding the pixel (`x`, `y`).
    pub fn scale_at(x: i32, y: i32) -> f32 {
        scale(unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) })
    }

    /// The work area of every connected monitor.
    pub fn work_areas() -> Vec<Rect> {
        unsafe extern "system" fn visit(m: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> BOOL {
            let out = &mut *(out.0 as *mut Vec<Rect>);
            let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            if GetMonitorInfoW(m, &mut info).as_bool() {
                out.push(rect(info.rcWork));
            }
            BOOL(1)
        }
        let mut out: Vec<Rect> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(None, None, Some(visit), LPARAM(&mut out as *mut _ as isize));
        }
        out
    }

    /// Put the window exactly on `to`, in pixels. The window library takes points and converts
    /// them with the scale of the monitor it first creates the window on, which is not always the
    /// one the rectangle is on; this is exact. The new scale's resize message then finds the
    /// size already right.
    pub fn move_to(hwnd: isize, to: Rect) {
        let hwnd = HWND(hwnd as *mut _);
        let set = || unsafe {
            let _ = SetWindowPos(hwnd, None, to.left, to.top, to.width(), to.height(), SWP_NOZORDER | SWP_NOACTIVATE);
        };
        // Moving onto a monitor with another scale makes the window library resize the window
        // by the ratio of the scales; the second call puts the size right again.
        set();
        set();
    }

    /// The window's rectangle, unless it is maximized (its next start should be the normal size).
    pub fn window_rect(hwnd: isize) -> Option<Rect> {
        let hwnd = HWND(hwnd as *mut _);
        unsafe {
            if IsZoomed(hwnd).as_bool() {
                return None;
            }
            let mut r = RECT::default();
            GetWindowRect(hwnd, &mut r).ok()?;
            Some(rect(r))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect { left, top, right, bottom }
    }

    // A 4K monitor at 150% with a 32 pt top strip (48 px) and a 60 px dock taken out.
    const MAIN: Rect = Rect { left: 0, top: 48, right: 3840, bottom: 2100 };
    // The same to the left of it: negative x.
    const LEFT: Rect = Rect { left: -3840, top: 48, right: 0, bottom: 2100 };
    // A 4K monitor at 300%, below and to the right.
    const LOW: Rect = Rect { left: 3746, top: 2160 + 96, right: 7586, bottom: 4320 - 120 };

    #[test]
    fn first_start_is_centred_at_the_default_size() {
        let p = place(MAIN, 1.5, None, &[MAIN, LEFT, LOW]);
        assert_eq!((p.width(), p.height()), (1650, 1050));
        assert_eq!(p, r(1095, 549, 2745, 1599));
    }

    #[test]
    fn first_start_on_the_negative_monitor_stays_on_it() {
        let p = place(LEFT, 1.5, None, &[MAIN, LEFT, LOW]);
        assert_eq!(p, r(-2745, 549, -1095, 1599));
        assert!(LEFT.contains(&p));
    }

    #[test]
    fn first_start_at_300_percent_is_wider_and_cut_to_the_work_height() {
        let p = place(LOW, 3.0, None, &[MAIN, LEFT, LOW]);
        assert_eq!((p.width(), p.height()), (3300, 1944));
        assert!(LOW.contains(&p));
        assert_eq!(p.left - LOW.left, LOW.right - p.right);
        assert_eq!(p.top - LOW.top, LOW.bottom - p.bottom);
    }

    #[test]
    fn a_small_work_area_shrinks_the_window_to_fit() {
        let small = r(0, 32, 1280, 700);
        let p = place(small, 1.0, None, &[small]);
        assert_eq!(p, r(90, 32, 1190, 700));
        let tiny = r(0, 0, 800, 500);
        assert_eq!(place(tiny, 1.0, None, &[tiny]), tiny);
    }

    #[test]
    fn a_saved_rect_on_a_connected_monitor_is_restored() {
        let saved = r(-3000, 300, -1500, 1300);
        assert_eq!(place(MAIN, 1.5, Some(saved), &[MAIN, LEFT, LOW]), saved);
    }

    #[test]
    fn a_saved_rect_under_the_top_bar_or_off_a_monitor_is_not() {
        let works = [MAIN, LEFT, LOW];
        let under_bar = r(100, 0, 1800, 1000);
        assert_eq!(place(MAIN, 1.5, Some(under_bar), &works), place(MAIN, 1.5, None, &works));
        // Spanning the seam between two monitors is not fully on either.
        let seam = r(-800, 300, 800, 1300);
        assert_eq!(place(MAIN, 1.5, Some(seam), &works), place(MAIN, 1.5, None, &works));
        // A monitor that was unplugged.
        let gone = r(8000, 300, 9600, 1300);
        assert_eq!(place(MAIN, 1.5, Some(gone), &works), place(MAIN, 1.5, None, &works));
    }

    #[test]
    fn a_saved_rect_restores_whatever_monitor_the_pointer_is_on() {
        let saved = r(200, 300, 1700, 1100);
        assert_eq!(place(LOW, 3.0, Some(saved), &[MAIN, LEFT, LOW]), saved);
    }

    #[test]
    fn the_picker_centres_over_its_owner_but_stays_on_the_work_area() {
        let owner = r(1000, 500, 2000, 1300);
        let p = centre_over(Some(owner), MAIN, (800, 600));
        assert_eq!(p, r(1100, 600, 1900, 1200));
        // An owner near the top left pushes the picker back onto the work area.
        let corner = r(-100, 0, 300, 200);
        let p = centre_over(Some(corner), MAIN, (800, 600));
        assert_eq!((p.left, p.top), (MAIN.left, MAIN.top));
        // No owner: the middle of the work area.
        let p = centre_over(None, MAIN, (800, 600));
        assert_eq!(p.left - MAIN.left, MAIN.right - p.right);
        // Too big: cut to the work area.
        let small = r(0, 0, 640, 480);
        assert_eq!(centre_over(Some(owner), small, (800, 600)), small);
    }

    #[test]
    fn a_saved_rect_below_the_minimum_is_not_restored() {
        let sliver = r(100, 100, 400, 300);
        assert_ne!(place(MAIN, 1.5, Some(sliver), &[MAIN]), sliver);
    }
}
