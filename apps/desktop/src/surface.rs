//! What one wallpaper window shows, and how it paints it.
//!
//! A surface is a background colour plus at most one ready-made bitmap. Painting is a fill and a
//! blit, nothing else, so there is no work between repaints. The window procedure lives here too:
//! it keeps the window at the very bottom, never lets it take focus, and hands a right-click to the
//! shelf process, which shows the desktop's menu.

use crate::fit::Area;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, CreateSolidBrush, DeleteDC, DeleteObject, EndPaint, FillRect,
    HGDIOBJ, PAINTSTRUCT, SRCCOPY, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetCursorPos, LoadCursorW, PostMessageW, RegisterClassW, SetWindowPos, ShowWindow,
    HWND_BOTTOM, IDC_ARROW, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, WINDOWPOS, WM_DPICHANGED, WM_ERASEBKGND,
    WM_MOUSEACTIVATE, WM_NCDESTROY, WM_PAINT, WM_RBUTTONUP, WM_WINDOWPOSCHANGING, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_POPUP,
};

/// The bar's fullscreen check skips windows of this class, so keep the name exactly.
const CLASS: windows::core::PCWSTR = w!("ShellmanDesktop");

/// Sent to the controller window when a desktop window is told its DPI changed.
pub const WM_REFRESH_DISPLAY: u32 = 0x8000 + 2;

/// A GDI bitmap (a DIB section), kept as a number so it can cross threads, and deleted when the
/// last surface showing it lets go: monitors showing the same picture at the same size share one.
pub struct Bitmap(pub isize);

impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.0 as *mut _));
        }
    }
}

/// A decoded picture ready to blit: a bitmap of exactly `dst`'s size.
#[derive(Clone)]
pub struct Image {
    pub bitmap: Arc<Bitmap>,
    pub dst: Area,
    pub tile: bool,
}

/// What a window shows. Dropping it releases its bitmap (once no other window shows it).
pub struct Surface {
    /// A COLORREF.
    pub color: u32,
    pub image: Option<Image>,
}

thread_local! {
    /// The surface of each window, by handle. Only the UI thread touches this.
    static SURFACES: RefCell<HashMap<isize, Surface>> = RefCell::new(HashMap::new());
}

/// Register the window class (once).
pub fn register() {
    unsafe {
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
            lpszClassName: CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&class);
    }
}

/// A wallpaper window covering `area`, at the bottom of the z-order, shown without activation;
/// left hidden when `shown` is false (the wallpaper stands aside for Explorer's desktop).
pub fn create(area: Area, shown: bool) -> Option<HWND> {
    unsafe {
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS,
            w!("shellman desktop"),
            WS_POPUP,
            area.x,
            area.y,
            area.w,
            area.h,
            None,
            None,
            GetModuleHandleW(None).ok().map(Into::into),
            None,
        )
        .ok()?;
        let _ = SetWindowPos(hwnd, Some(HWND_BOTTOM), area.x, area.y, area.w, area.h, SWP_NOACTIVATE);
        if shown {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        Some(hwnd)
    }
}

/// Hide or show a wallpaper window, never activating it.
pub fn show(hwnd: HWND, shown: bool) {
    unsafe {
        let _ = ShowWindow(hwnd, if shown { SW_SHOWNOACTIVATE } else { SW_HIDE });
    }
}

/// Show `surface` in `hwnd`, replacing (and freeing) the previous one.
pub fn set(hwnd: HWND, surface: Surface) {
    // The old one is dropped outside the borrow.
    let old = SURFACES.with(|s| s.borrow_mut().insert(hwnd.0 as isize, surface));
    drop(old);
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, false);
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // The whole window is drawn in WM_PAINT; erasing first would flicker.
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        // Never take focus from the window in front: MA_NOACTIVATE.
        WM_MOUSEACTIVATE => LRESULT(3),
        // The wallpaper's menu is the shelf process's (egui and glass stay out of this one).
        WM_RBUTTONUP => {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_ok() {
                shellman_core::desktop_menu::request(pt.x, pt.y);
            }
            LRESULT(0)
        }
        // Whoever moves or restacks this window, it stays at the very bottom.
        WM_WINDOWPOSCHANGING => {
            let pos = &mut *(lparam.0 as *mut WINDOWPOS);
            pos.hwndInsertAfter = HWND_BOTTOM;
            pos.flags &= !SWP_NOZORDER;
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DPICHANGED => {
            // The controller owns monitor layout; hand it the news.
            if let Some(ctl) = crate::app::controller() {
                let _ = PostMessageW(Some(ctl), WM_REFRESH_DISPLAY, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let gone = SURFACES.with(|s| s.borrow_mut().remove(&(hwnd.0 as isize)));
            drop(gone);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut client = RECT::default();
    let _ = GetClientRect(hwnd, &mut client);
    SURFACES.with(|s| {
        let surfaces = s.borrow();
        let color = surfaces.get(&(hwnd.0 as isize)).map_or(0, |s| s.color);
        let image = surfaces.get(&(hwnd.0 as isize)).and_then(|s| s.image.as_ref());
        let covers = image.is_some_and(|i| i.dst.w >= client.right && i.dst.h >= client.bottom);
        if !covers {
            let brush = CreateSolidBrush(COLORREF(color));
            FillRect(hdc, &client, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }
        if let Some(image) = image {
            blit(hdc, image, &client);
        }
    });
    let _ = EndPaint(hwnd, &ps);
}

unsafe fn blit(hdc: windows::Win32::Graphics::Gdi::HDC, image: &Image, client: &RECT) {
    let mem = CreateCompatibleDC(Some(hdc));
    let old = SelectObject(mem, HGDIOBJ(image.bitmap.0 as *mut _));
    let d = image.dst;
    if image.tile {
        let mut y = 0;
        while y < client.bottom {
            let mut x = 0;
            while x < client.right {
                let _ = BitBlt(hdc, x, y, d.w, d.h, Some(mem), 0, 0, SRCCOPY);
                x += d.w;
            }
            y += d.h;
        }
    } else {
        let _ = BitBlt(hdc, d.x, d.y, d.w, d.h, Some(mem), 0, 0, SRCCOPY);
    }
    SelectObject(mem, old);
    let _ = DeleteDC(mem);
}
