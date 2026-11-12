#![windows_subsystem = "windows"]
//! Fileman: a file manager in Blurman's frosted style.

mod app;
mod fsops;
mod theme;
mod thumbs;
mod watch;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmDefWindowProc, DwmExtendFrameIntoClientArea, DwmGetWindowAttribute, DwmSetWindowAttribute,
    DWMWA_CAPTION_BUTTON_BOUNDS, DWMSBT_TRANSIENTWINDOW,
    DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_SYSTEMBACKDROP_TYPE,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DWMWINDOWATTRIBUTE,
};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, GetWindowRect, IsZoomed, SetWindowLongPtrW, GWL_STYLE, WS_CAPTION,
    WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME, SetWindowPos, NCCALCSIZE_PARAMS, SM_CXPADDEDBORDER, SM_CYFRAME,
    SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WM_NCACTIVATE, WM_NCCALCSIZE,
    WM_NCHITTEST,
};

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    pub accent: theme::AccentChoice,
    pub show_hidden: bool,
    /// Folders open in tabs when the window last closed.
    pub tabs: Vec<PathBuf>,
}

fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("fileman").join("settings.json"))
}

pub fn load_settings() -> Settings {
    settings_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_settings(s: &Settings) {
    let Some(path) = settings_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_vec_pretty(s) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}

fn main() -> eframe::Result {
    let settings = load_settings();
    // A folder passed on the command line opens in its own tab.
    let arg = std::env::args_os().nth(1).map(PathBuf::from).filter(|p| p.is_dir());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Fileman")
            .with_inner_size([1100.0, 700.0])
            .with_min_inner_size([640.0, 400.0])
            .with_transparent(true)
            .with_decorations(false),
        ..Default::default()
    };
    eframe::run_native(
        "Fileman",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            theme::set_light(cc.egui_ctx.theme() == egui::Theme::Light);
            theme::set_accent(&cc.egui_ctx, settings.accent.resolve());
            theme::apply(&cc.egui_ctx);
            let mut hwnd = 0isize;
            let mut glass = false;
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    hwnd = win32.hwnd.get();
                    glass = frost_window(HWND(hwnd as *mut _));
                    remove_caption(HWND(hwnd as *mut _), glass);
                }
            }
            Ok(Box::new(app::FileMan::new(
                cc.egui_ctx.clone(),
                settings,
                arg,
                hwnd,
                glass,
            )))
        }),
    )
}

fn set_dwm_u32(hwnd: HWND, attribute: DWMWINDOWATTRIBUTE, value: u32) {
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            attribute,
            (&raw const value).cast(),
            size_of::<u32>() as u32,
        );
    }
}

fn colorref(c: egui::Color32) -> u32 {
    (c.b() as u32) << 16 | (c.g() as u32) << 8 | c.r() as u32
}

/// Border in the palette's mode and accent tint, and no caption fill.
pub fn paint_frame(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    let hwnd = HWND(hwnd as *mut _);
    let c = theme::colors();
    set_dwm_u32(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(!c.light));
    set_dwm_u32(hwnd, DWMWA_BORDER_COLOR, colorref(theme::mix(c.sidebar_solid, c.accent, 0.35)));
    // DWMWA_COLOR_NONE: no caption fill, so the top of the window is the same glass as the rest.
    set_dwm_u32(hwnd, DWMWA_CAPTION_COLOR, 0xFFFF_FFFE);
}

/// The Windows 11 acrylic backdrop, so the window itself is frosted glass. False where Windows
/// has no system backdrops and the window stays opaque.
fn frost_window(hwnd: HWND) -> bool {
    paint_frame(hwnd.0 as isize);
    set_dwm_u32(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND.0 as u32);
    let margins = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };
    let value = DWMSBT_TRANSIENTWINDOW.0;
    let frosted = unsafe {
        DwmExtendFrameIntoClientArea(hwnd, &margins).is_ok()
            && DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                (&raw const value).cast(),
                size_of::<i32>() as u32,
            )
            .is_ok()
    };
    frosted
}

/// Let the tab strip be the title bar. The window keeps a thick frame (no caption), so it still
/// gets Snap, the shadow, and rounded corners. Over glass, DWM draws the real Windows 11
/// minimize, maximize and close buttons; `frame_proc` routes them through `DwmDefWindowProc`, so
/// they hover, click, and open Snap layouts like any window's.
fn remove_caption(hwnd: HWND, glass: bool) {
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(frame_proc), 1, usize::from(glass));
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) & !(WS_CAPTION.0 as isize);
        let style = style | (WS_THICKFRAME.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0) as isize;
        SetWindowLongPtrW(hwnd, GWL_STYLE, style);
        // Ask Windows to measure the frame again, now through `frame_proc`.
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        );
    }
}

/// Size of the Windows caption buttons in physical pixels, or None when Windows draws none.
pub fn caption_buttons_size(hwnd: isize) -> Option<(i32, i32)> {
    if hwnd == 0 {
        return None;
    }
    let mut rc = RECT::default();
    unsafe {
        DwmGetWindowAttribute(
            HWND(hwnd as *mut _),
            DWMWA_CAPTION_BUTTON_BOUNDS,
            (&raw mut rc).cast(),
            size_of::<RECT>() as u32,
        )
    }
    .ok()?;
    let size = (rc.right - rc.left, rc.bottom - rc.top);
    (size.0 > 0 && size.1 > 0).then_some(size)
}

/// How thick the sizing frame is at the window's DPI.
unsafe fn frame_thickness(hwnd: HWND) -> i32 {
    let dpi = GetDpiForWindow(hwnd);
    GetSystemMetricsForDpi(SM_CYFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
}

/// The window's frame, without a caption.
///
/// - The client area is the whole window; a maximized window is inset by its frame, which hangs
///   off the screen edge.
/// - The outer few pixels of the client area resize the window.
/// - With glass, the frame is always drawn active: Windows otherwise swaps the backdrop for a flat
///   color while the window is inactive.
unsafe extern "system" fn frame_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    glass: usize,
) -> LRESULT {
    if glass != 0 {
        let mut handled = LRESULT(0);
        if DwmDefWindowProc(hwnd, msg, wparam, lparam, &mut handled).as_bool() {
            return handled;
        }
    }
    match msg {
        WM_NCACTIVATE if glass != 0 => DefSubclassProc(hwnd, msg, WPARAM(1), lparam),
        WM_NCCALCSIZE if wparam.0 != 0 => {
            if IsZoomed(hwnd).as_bool() {
                let params = lparam.0 as *mut NCCALCSIZE_PARAMS;
                let f = frame_thickness(hwnd);
                let r = &mut (*params).rgrc[0];
                r.left += f;
                r.top += f;
                r.right -= f;
                r.bottom -= f;
            }
            LRESULT(0)
        }
        WM_NCHITTEST => {
            const HTCLIENT: isize = 1;
            let hit = DefSubclassProc(hwnd, msg, wparam, lparam);
            if hit.0 != HTCLIENT || IsZoomed(hwnd).as_bool() {
                return hit;
            }
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let mut rc = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rc);
            LRESULT(edge_hit(x, y, &rc, frame_thickness(hwnd)).unwrap_or(HTCLIENT))
        }
        _ => DefSubclassProc(hwnd, msg, wparam, lparam),
    }
}

/// Which sizing edge or corner a point is on, if any. Corners are twice as forgiving.
fn edge_hit(x: i32, y: i32, rc: &RECT, border: i32) -> Option<isize> {
    let corner = border * 2;
    let left = x < rc.left + border;
    let right = x >= rc.right - border;
    let top = y < rc.top + border;
    let bottom = y >= rc.bottom - border;
    let near_left = x < rc.left + corner;
    let near_right = x >= rc.right - corner;
    let near_top = y < rc.top + corner;
    let near_bottom = y >= rc.bottom - corner;
    // HTLEFT 10, HTRIGHT 11, HTTOP 12, HTTOPLEFT 13, HTTOPRIGHT 14, HTBOTTOM 15,
    // HTBOTTOMLEFT 16, HTBOTTOMRIGHT 17.
    Some(match () {
        _ if (top && near_left) || (left && near_top) => 13,
        _ if (top && near_right) || (right && near_top) => 14,
        _ if (bottom && near_left) || (left && near_bottom) => 16,
        _ if (bottom && near_right) || (right && near_bottom) => 17,
        _ if top => 12,
        _ if bottom => 15,
        _ if left => 10,
        _ if right => 11,
        _ => return None,
    })
}
