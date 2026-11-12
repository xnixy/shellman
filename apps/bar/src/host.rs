//! The bar process's main window: an invisible host that owns every bar window.
//!
//! eframe's main window drives the others: only its frames can open and close bar windows. A bar
//! is hidden while a fullscreen app covers its monitor, and a hidden window gets no frames, so no
//! bar can be the main window. The host is a 1-pixel window that is "cloaked" (Windows keeps it
//! visible, so it still gets frames, but never draws it), and it is never hidden.
//!
//! It also hears what changes the monitors, without polling: monitors added, removed or
//! re-arranged and resolution changes (`WM_DISPLAYCHANGE`), work-area changes
//! (`WM_SETTINGCHANGE`), and scale changes (`WM_DPICHANGED`, which bar windows forward here).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_SETWORKAREA, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_SETTINGCHANGE,
};

static CTX: OnceLock<egui::Context> = OnceLock::new();
/// The monitors (or something about them) changed since the last layout.
static LAYOUT: AtomicBool = AtomicBool::new(true);
/// Explorer said app bar positions changed: ask it again where the bars may go.
static REQUERY: AtomicBool = AtomicBool::new(false);

/// Make `hwnd`, eframe's main window, the host: never shown, never focused, and listening.
pub fn start(hwnd: isize, ctx: egui::Context) {
    let _ = CTX.set(ctx);
    crate::shape::never_activate(hwnd);
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        let on: u32 = 1;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_CLOAK, (&raw const on).cast(), 4);
        let _ = SetWindowSubclass(hwnd, Some(host_proc), 2, 0);
    }
    crate::fullscreen::start(hwnd.0 as isize);
}

/// Lay the bars out again on the next host frame. Safe from any thread.
pub fn request_layout() {
    LAYOUT.store(true, Ordering::Release);
    if let Some(ctx) = CTX.get() {
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

/// Ask Explorer again where the app bars may go, on the next host frame.
pub fn request_requery() {
    REQUERY.store(true, Ordering::Release);
    request_layout();
}

/// True once after a layout was asked for (and on the first call).
pub fn take_layout() -> bool {
    LAYOUT.swap(false, Ordering::AcqRel)
}

pub fn take_requery() -> bool {
    REQUERY.swap(false, Ordering::AcqRel)
}

unsafe extern "system" fn host_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        crate::fullscreen::WM_CHECK => {
            crate::fullscreen::check();
            return LRESULT(0);
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED => request_layout(),
        WM_SETTINGCHANGE if wparam.0 == SPI_SETWORKAREA.0 as usize => request_layout(),
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}
