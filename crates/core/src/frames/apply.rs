//! The only calls made on other apps' windows: `DwmSetWindowAttribute` with the five frame
//! attributes. Colours are set per window; Windows redraws the frame itself.

use super::colors::{Border, Palette, COLOR_DEFAULT};
use super::colors::colorref;
use std::ffi::c_void;
use windows::Win32::Foundation::{E_ACCESSDENIED, HWND};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWINDOWATTRIBUTE,
};

/// DWMWCP_ROUND, and DWMWCP_DEFAULT to give the choice back.
const CORNER_ROUND: u32 = 2;
const CORNER_DEFAULT: u32 = 0;

#[derive(PartialEq, Eq, Debug)]
pub enum Outcome {
    Done,
    /// The window belongs to an elevated process (or another we may not touch): leave it alone.
    Denied,
}

fn set(hwnd: HWND, attr: DWMWINDOWATTRIBUTE, value: u32) -> Result<(), windows::core::Error> {
    unsafe { DwmSetWindowAttribute(hwnd, attr, (&raw const value).cast::<c_void>(), size_of::<u32>() as u32) }
}

/// Everything: dark or light, caption, text, border, corners.
pub fn style(hwnd: HWND, p: &Palette, border: Border) -> Outcome {
    let first = set(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, p.dark as u32);
    if matches!(&first, Err(e) if e.code() == E_ACCESSDENIED) {
        return Outcome::Denied;
    }
    let _ = set(hwnd, DWMWA_CAPTION_COLOR, colorref(p.caption));
    let _ = set(hwnd, DWMWA_TEXT_COLOR, colorref(p.text));
    let _ = set(hwnd, DWMWA_BORDER_COLOR, border.colorref(p));
    let _ = set(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, CORNER_ROUND);
    Outcome::Done
}

/// Only the border, when the foreground moves.
pub fn border(hwnd: HWND, p: &Palette, border: Border) {
    let _ = set(hwnd, DWMWA_BORDER_COLOR, border.colorref(p));
}

/// Back to Windows' colours and corners. The dark-mode flag is left as it is: the app's earlier
/// value is not known.
pub fn reset(hwnd: HWND) {
    let _ = set(hwnd, DWMWA_CAPTION_COLOR, COLOR_DEFAULT);
    let _ = set(hwnd, DWMWA_TEXT_COLOR, COLOR_DEFAULT);
    let _ = set(hwnd, DWMWA_BORDER_COLOR, COLOR_DEFAULT);
    let _ = set(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, CORNER_DEFAULT);
}
