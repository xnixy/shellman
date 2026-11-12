#![windows_subsystem = "windows"]
//! Fileman: a file manager in Blurman's frosted style.

mod app;
mod fsops;
mod theme;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_TRANSIENTWINDOW,
    DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_TEXT_COLOR,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DWMWINDOWATTRIBUTE,
};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, IsZoomed, SetWindowPos, NCCALCSIZE_PARAMS, SM_CXPADDEDBORDER, SM_CYFRAME,
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
            .with_transparent(true),
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

/// Title bar and border in the palette's mode and accent tint.
pub fn paint_frame(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    let hwnd = HWND(hwnd as *mut _);
    let c = theme::colors();
    set_dwm_u32(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(!c.light));
    set_dwm_u32(hwnd, DWMWA_BORDER_COLOR, colorref(theme::mix(c.sidebar_solid, c.accent, 0.35)));
    set_dwm_u32(hwnd, DWMWA_CAPTION_COLOR, colorref(c.sidebar_solid));
    set_dwm_u32(hwnd, DWMWA_TEXT_COLOR, colorref(c.text));
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

/// Drop the Windows caption and let the tab strip be the title bar. The frame stays, so resizing,
/// Snap, the shadow, and rounded corners all still work.
fn remove_caption(hwnd: HWND, glass: bool) {
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(frame_proc), 1, usize::from(glass));
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

/// How thick the sizing frame is at the window's DPI.
unsafe fn frame_thickness(hwnd: HWND) -> i32 {
    let dpi = GetDpiForWindow(hwnd);
    GetSystemMetricsForDpi(SM_CYFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
}

/// The window's frame, without a caption.
///
/// - The client area grows up over where the caption was. Left, right and bottom keep their
///   (invisible on Windows 11) sizing borders.
/// - The top few pixels still resize the window, since the caption is gone.
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
    match msg {
        WM_NCACTIVATE if glass != 0 => DefSubclassProc(hwnd, msg, WPARAM(1), lparam),
        WM_NCCALCSIZE if wparam.0 != 0 => {
            let params = lparam.0 as *mut NCCALCSIZE_PARAMS;
            let top = (*params).rgrc[0].top;
            let result = DefSubclassProc(hwnd, msg, wparam, lparam);
            (*params).rgrc[0].top = top;
            // A maximized window hangs its frame off the screen edge; keep content on screen.
            if IsZoomed(hwnd).as_bool() {
                (*params).rgrc[0].top += frame_thickness(hwnd);
            }
            result
        }
        WM_NCHITTEST => {
            const HTCLIENT: isize = 1;
            const HTTOP: isize = 12;
            const HTTOPLEFT: isize = 13;
            const HTTOPRIGHT: isize = 14;
            let hit = DefSubclassProc(hwnd, msg, wparam, lparam);
            if hit.0 != HTCLIENT || IsZoomed(hwnd).as_bool() {
                return hit;
            }
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let mut rc = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rc);
            let border = frame_thickness(hwnd);
            if y < rc.top + border {
                let corner = border * 2;
                return LRESULT(if x < rc.left + corner {
                    HTTOPLEFT
                } else if x >= rc.right - corner {
                    HTTOPRIGHT
                } else {
                    HTTOP
                });
            }
            hit
        }
        _ => DefSubclassProc(hwnd, msg, wparam, lparam),
    }
}
