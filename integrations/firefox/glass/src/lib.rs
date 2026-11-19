//! shellman_glass.dll: shellman's floating glass islands under a Firefox window.
//!
//! The Firefox script `shellman-glass.sys.mjs` loads this with js-ctypes and calls it on the
//! browser's main thread, the thread that owns the window, exactly as a shellman app uses
//! [`GlassPanel`] for its own window: the glass is a second window of this thread right under the
//! browser window, with one composition visual per island (blur, tint, rim, shadow), moved in the
//! same window message that moved the browser window.
//!
//! What it does to the browser window, and undoes in [`shellman_glass_detach`]:
//! - no system backdrop (`DWMWA_SYSTEMBACKDROP_TYPE` none) and no Windows frame rendering
//!   (`DWMWA_NCRENDERING_POLICY` disabled). Measured on Firefox 160 / Windows 11 26300: with
//!   both, every pixel Firefox leaves transparent shows what is behind the window, sharp, whatever
//!   frame margins Firefox sets (with the backdrop off but frame rendering on, the part outside
//!   Firefox's frame margins shows black, and Firefox resets those margins on every layout);
//! - its non-client resize border (Firefox keeps a thin one at the sides and bottom) painted
//!   transparent: with frame rendering off Windows would paint a classic frame there. The border
//!   still takes the mouse, so the window resizes from its edges as before. GlassPanel's window
//!   region (the whole window) takes away Windows' rectangular shadow; the islands cast their own.
//!
//! Firefox sets the backdrop again on some changes (theme, size mode), so a subclass sets both
//! attributes again after the messages that go with those, and [`shellman_glass_refresh`] lets the
//! script ask for it.
//!
//! The C ABI never unwinds into Firefox: every entry point catches panics and reports failure.
//! Failure to attach leaves the window as it was (the script then keeps Windows' Acrylic look).

use egui::{pos2, Color32, Rect};
use shellman_ui::glass::{GlassPanel, Island, Style};
use shellman_ui::theme::Colors;
use std::cell::Cell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_NCRENDERING_POLICY,
    DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWINDOWATTRIBUTE,
};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, ExcludeClipRect, FillRect, GetStockObject, GetWindowDC, RedrawWindow,
    ReleaseDC, BLACK_BRUSH, HBRUSH, RDW_FRAME, RDW_INVALIDATE,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetWindowRect, GetWindowThreadProcessId, IsWindow, WM_ACTIVATE, WM_DPICHANGED,
    WM_DWMCOMPOSITIONCHANGED, WM_NCACTIVATE, WM_NCDESTROY, WM_NCPAINT, WM_SETTINGCHANGE, WM_SIZE,
    WM_STYLECHANGED, WM_THEMECHANGED,
};

/// Bumped when the ABI changes; the script refuses a DLL of another version.
pub const ABI_VERSION: u32 = 1;

/// The subclass id on the browser window (GlassPanel's own differs).
const SUBCLASS: usize = 0x6666_676c;

/// One island, in device pixels of the window's client area (CSS pixels times
/// `devicePixelRatio`, from the top-left of the browser document).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IslandRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    /// Corner radius in device pixels; more than half the shorter side makes a pill.
    pub radius: f32,
    /// 0 to 1; how much of the glass shows.
    pub opacity: f32,
    /// Non-zero: the island casts the style's shadow.
    pub shadow: u32,
}

/// The islands the script sent, as GlassPanel islands at one pixel per point. Pieces that are
/// empty or not finite are dropped; radius and opacity are brought into range.
pub fn to_islands(rects: &[IslandRect]) -> Vec<Island> {
    rects
        .iter()
        .filter(|r| {
            [r.left, r.top, r.right, r.bottom]
                .iter()
                .all(|v| v.is_finite())
                && r.right > r.left
                && r.bottom > r.top
        })
        .map(|r| {
            let rect = Rect::from_min_max(pos2(r.left, r.top), pos2(r.right, r.bottom));
            let half = rect.width().min(rect.height()) / 2.0;
            let radius = if r.radius.is_finite() {
                r.radius.clamp(0.0, half)
            } else {
                half
            };
            let opacity = if r.opacity.is_finite() {
                r.opacity.clamp(0.0, 1.0)
            } else {
                1.0
            };
            Island {
                rect,
                radius,
                opacity,
                shadow: r.shadow != 0,
            }
        })
        .collect()
}

/// shellman's glass for a mode, an accent (0xRRGGBB) and a glass amount (0 solid, 1 clearest;
/// anything else is taken as 1, the theme's default).
pub fn style(light: bool, rgb: u32, glass: f32) -> Style {
    let accent = Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
    let glass = if glass.is_finite() {
        glass.clamp(0.0, 1.0)
    } else {
        1.0
    };
    // The radius only sizes the room left for the shadow; every island has its own.
    Style::new(&Colors::new(light, accent), glass, shellman_ui::theme::shape::ISLAND)
}

/// One attached browser window.
pub struct Glass {
    hwnd: HWND,
    panel: Option<GlassPanel>,
    /// The backdrop Firefox had chosen, put back on detach.
    backdrop: u32,
    /// False once the window is destroyed.
    alive: Cell<bool>,
}

fn set(hwnd: HWND, attribute: DWMWINDOWATTRIBUTE, value: u32) {
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, attribute, (&raw const value).cast(), 4);
    }
}

/// No backdrop, no frame rendering, no Windows corners or border.
fn bare(hwnd: HWND) {
    set(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, 1); // DWMSBT_NONE
    set(hwnd, DWMWA_NCRENDERING_POLICY, 1); // DWMNCRP_DISABLED
    set(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, 1); // DWMWCP_DONOTROUND
    set(hwnd, DWMWA_BORDER_COLOR, 0xFFFF_FFFE); // DWMWA_COLOR_NONE
}

/// Back to what Firefox set up: its backdrop, Windows' frame rendering, corners and border.
fn restore(hwnd: HWND, backdrop: u32) {
    set(hwnd, DWMWA_NCRENDERING_POLICY, 0); // DWMNCRP_USEWINDOWSTYLE
    set(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, 0); // DWMWCP_DEFAULT
    set(hwnd, DWMWA_BORDER_COLOR, 0xFFFF_FFFF); // DWMWA_COLOR_DEFAULT
    set(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, backdrop);
}

fn guarded<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}

/// The ABI version, so the script can tell an old DLL.
#[no_mangle]
pub extern "C" fn shellman_glass_version() -> u32 {
    ABI_VERSION
}

/// Put glass under `hwnd`, a top-level window of the calling thread. Returns null (and leaves
/// the window as it was) when the window is not this thread's or Windows cannot compose glass
/// (composition unavailable, `SHELLMAN_NO_COMPOSITION` set).
#[no_mangle]
pub extern "C" fn shellman_glass_attach(
    hwnd: isize,
    light: i32,
    rgb: u32,
    glass: f32,
) -> *mut Glass {
    guarded(std::ptr::null_mut(), || unsafe {
        let window = HWND(hwnd as *mut _);
        if hwnd == 0 || !IsWindow(Some(window)).as_bool() {
            return std::ptr::null_mut();
        }
        if GetWindowThreadProcessId(window, None) != GetCurrentThreadId() {
            return std::ptr::null_mut();
        }
        let mut backdrop: u32 = 0;
        let _ = DwmGetWindowAttribute(
            window,
            DWMWA_SYSTEMBACKDROP_TYPE,
            (&raw mut backdrop).cast(),
            4,
        );
        let panel = GlassPanel::attach(hwnd, style(light != 0, rgb, glass));
        if !panel.composed() {
            drop(panel);
            restore(window, backdrop);
            return std::ptr::null_mut();
        }
        // No islands until the script sends them, so nothing shows before the page is laid out.
        panel.set_islands(&[], 1.0);
        bare(window);
        let glass = Box::into_raw(Box::new(Glass {
            hwnd: window,
            panel: Some(panel),
            backdrop,
            alive: Cell::new(true),
        }));
        let _ = SetWindowSubclass(window, Some(keep_bare), SUBCLASS, glass as usize);
        // Clear the frame Windows painted before.
        let _ = RedrawWindow(Some(window), None, None, RDW_FRAME | RDW_INVALIDATE);
        glass
    })
}

/// Change the look: light or dark, accent 0xRRGGBB, glass amount 0..1. Returns 1 on success.
///
/// # Safety
/// `glass` is null or a handle from [`shellman_glass_attach`] not yet detached.
#[no_mangle]
pub unsafe extern "C" fn shellman_glass_set_style(
    glass: *mut Glass,
    light: i32,
    rgb: u32,
    amount: f32,
) -> i32 {
    guarded(0, || {
        let Some(g) = glass.as_ref() else { return 0 };
        match (&g.panel, g.alive.get()) {
            (Some(panel), true) => {
                panel.set_style(style(light != 0, rgb, amount));
                1
            }
            _ => 0,
        }
    })
}

/// Show glass at `count` islands (device pixels of the client area); 0 islands shows none.
/// Free when nothing changed. Returns 1 on success.
///
/// # Safety
/// `glass` as for [`shellman_glass_set_style`]; `rects` points to `count` islands (or is null
/// with `count` 0).
#[no_mangle]
pub unsafe extern "C" fn shellman_glass_set_islands(
    glass: *mut Glass,
    rects: *const IslandRect,
    count: u32,
) -> i32 {
    guarded(0, || {
        let Some(g) = glass.as_ref() else { return 0 };
        let rects = if rects.is_null() || count == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(rects, count.min(256) as usize)
        };
        match (&g.panel, g.alive.get()) {
            (Some(panel), true) => {
                panel.set_islands(&to_islands(rects), 1.0);
                1
            }
            _ => 0,
        }
    })
}

/// Turn the backdrop and frame rendering off again (after Firefox may have set them).
///
/// # Safety
/// As for [`shellman_glass_set_style`].
#[no_mangle]
pub unsafe extern "C" fn shellman_glass_refresh(glass: *mut Glass) -> i32 {
    guarded(0, || {
        let Some(g) = glass.as_ref() else { return 0 };
        if !g.alive.get() {
            return 0;
        }
        bare(g.hwnd);
        1
    })
}

/// Remove the glass and give the window back its own look. The handle is invalid afterwards.
///
/// # Safety
/// As for [`shellman_glass_set_style`]; never twice for one handle.
#[no_mangle]
pub unsafe extern "C" fn shellman_glass_detach(glass: *mut Glass) {
    guarded((), || {
        if glass.is_null() {
            return;
        }
        let mut g = Box::from_raw(glass);
        if g.alive.get() {
            let _ = RemoveWindowSubclass(g.hwnd, Some(keep_bare), SUBCLASS);
        }
        // GlassPanel removes its own subclass, backdrop window and region (or knows the window
        // is gone).
        drop(g.panel.take());
        if g.alive.get() {
            restore(g.hwnd, g.backdrop);
            let _ = RedrawWindow(Some(g.hwnd), None, None, RDW_FRAME | RDW_INVALIDATE);
        }
    })
}

/// Paint the non-client border transparent: GDI's black is all zeros, alpha included, which the
/// window's surface shows as clear now that Windows does not render the frame.
fn clear_frame(hwnd: HWND) {
    unsafe {
        let mut window = RECT::default();
        let mut client = RECT::default();
        if GetWindowRect(hwnd, &mut window).is_err() || GetClientRect(hwnd, &mut client).is_err() {
            return;
        }
        let mut origin = POINT::default();
        let _ = ClientToScreen(hwnd, &mut origin);
        let dc = GetWindowDC(Some(hwnd));
        if dc.is_invalid() {
            return;
        }
        let (x, y) = (origin.x - window.left, origin.y - window.top);
        ExcludeClipRect(dc, x, y, x + client.right, y + client.bottom);
        let all = RECT {
            left: 0,
            top: 0,
            right: window.right - window.left,
            bottom: window.bottom - window.top,
        };
        FillRect(dc, &all, HBRUSH(GetStockObject(BLACK_BRUSH).0));
        ReleaseDC(Some(hwnd), dc);
    }
}

unsafe extern "system" fn keep_bare(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    match msg {
        // The frame is ours to paint (clear), not Windows'.
        WM_NCPAINT => {
            guarded((), || clear_frame(hwnd));
            return LRESULT(0);
        }
        // Activation repaints the frame unless told not to (lParam -1); the rest of it
        // (Firefox's own handling, the return value) is unchanged.
        WM_NCACTIVATE => return DefSubclassProc(hwnd, msg, wparam, LPARAM(-1)),
        _ => {}
    }
    let result = DefSubclassProc(hwnd, msg, wparam, lparam);
    guarded((), || {
        let g = &*(data as *const Glass);
        match msg {
            WM_NCDESTROY => {
                g.alive.set(false);
                let _ = RemoveWindowSubclass(hwnd, Some(keep_bare), SUBCLASS);
            }
            WM_ACTIVATE
            | WM_SIZE
            | WM_DWMCOMPOSITIONCHANGED
            | WM_THEMECHANGED
            | WM_SETTINGCHANGE
            | WM_DPICHANGED
            | WM_STYLECHANGED => bare(hwnd),
            _ => {}
        }
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(l: f32, t: f32, r: f32, b: f32, radius: f32) -> IslandRect {
        IslandRect {
            left: l,
            top: t,
            right: r,
            bottom: b,
            radius,
            opacity: 1.0,
            shadow: 1,
        }
    }

    #[test]
    fn the_layout_is_what_the_script_measured() {
        // The ABI: seven 4-byte fields, as the script's ctypes.StructType declares them.
        assert_eq!(std::mem::size_of::<IslandRect>(), 28);
        let islands = to_islands(&[
            rect(36.0, 36.0, 174.0, 174.0, 69.0),
            rect(10.5, 20.0, 30.0, 400.0, 6.0),
        ]);
        assert_eq!(islands.len(), 2);
        assert_eq!(
            islands[0].rect,
            Rect::from_min_max(pos2(36.0, 36.0), pos2(174.0, 174.0))
        );
        assert_eq!(islands[0].radius, 69.0);
        assert!(islands[0].shadow && islands[0].opacity == 1.0);
        assert_eq!(islands[1].rect.left(), 10.5);
    }

    #[test]
    fn a_huge_radius_makes_a_pill() {
        let i = &to_islands(&[rect(0.0, 0.0, 300.0, 138.0, 2997.0)])[0];
        assert_eq!(i.radius, 69.0);
        let i = &to_islands(&[rect(0.0, 0.0, 300.0, 138.0, f32::NAN)])[0];
        assert_eq!(i.radius, 69.0);
        let i = &to_islands(&[rect(0.0, 0.0, 300.0, 138.0, -4.0)])[0];
        assert_eq!(i.radius, 0.0);
    }

    #[test]
    fn empty_and_broken_pieces_are_dropped() {
        let list = [
            rect(10.0, 10.0, 10.0, 50.0, 4.0),
            rect(10.0, 50.0, 40.0, 10.0, 4.0),
            rect(f32::NAN, 0.0, 10.0, 10.0, 4.0),
            rect(0.0, 0.0, f32::INFINITY, 10.0, 4.0),
            rect(0.0, 0.0, 10.0, 10.0, 4.0),
        ];
        assert_eq!(to_islands(&list).len(), 1);
        assert!(to_islands(&[]).is_empty());
    }

    #[test]
    fn opacity_and_shadow_come_through_in_range() {
        let mut r = rect(0.0, 0.0, 10.0, 10.0, 2.0);
        r.opacity = 3.0;
        r.shadow = 0;
        let i = to_islands(&[r])[0];
        assert_eq!(i.opacity, 1.0);
        assert!(!i.shadow);
        r.opacity = -1.0;
        assert_eq!(to_islands(&[r])[0].opacity, 0.0);
    }

    #[test]
    fn style_follows_mode_accent_and_glass() {
        let dark = style(false, 0x0067c0, 1.0);
        let light = style(true, 0x0067c0, 1.0);
        assert!(dark.tint.r() < light.tint.r());
        // More glass, a thinner tint; out-of-range amounts are clamped, NaN is the default.
        let solid = style(false, 0x0067c0, 0.0);
        assert!(solid.tint.a() > dark.tint.a());
        assert_eq!(style(false, 0x0067c0, 7.0), dark);
        assert_eq!(style(false, 0x0067c0, f32::NAN), dark);
        assert_eq!(style(false, 0x0067c0, -2.0), solid);
    }

    #[test]
    fn null_handles_are_refused_not_crashed_on() {
        unsafe {
            assert_eq!(shellman_glass_set_style(std::ptr::null_mut(), 0, 0, 1.0), 0);
            assert_eq!(
                shellman_glass_set_islands(std::ptr::null_mut(), std::ptr::null(), 0),
                0
            );
            assert_eq!(shellman_glass_refresh(std::ptr::null_mut()), 0);
            shellman_glass_detach(std::ptr::null_mut());
        }
        assert!(shellman_glass_attach(0, 0, 0, 1.0).is_null());
        assert_eq!(shellman_glass_version(), ABI_VERSION);
    }
}
