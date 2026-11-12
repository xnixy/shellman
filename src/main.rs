#![windows_subsystem = "windows"]
//! Fileman: a file manager in Blurman's frosted style.

mod app;
mod fsops;
mod theme;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_TRANSIENTWINDOW,
    DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_TEXT_COLOR,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DWMWINDOWATTRIBUTE,
};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::WM_NCACTIVATE;

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
    if frosted {
        unsafe {
            let _ = SetWindowSubclass(hwnd, Some(stay_frosted), 1, 0);
        }
    }
    frosted
}

/// Windows swaps the backdrop for a flat color while the window is inactive. Always drawing the
/// frame active keeps the glass.
unsafe extern "system" fn stay_frosted(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_NCACTIVATE {
        return DefSubclassProc(hwnd, msg, WPARAM(1), lparam);
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}
