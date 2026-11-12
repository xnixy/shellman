//! "Identify": a big number on each screen for a few seconds. Each is a small topmost borderless
//! window of its own, which takes no focus and is gone when the time is up.

use super::geom::Rect;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const SHOWN_FOR_MS: u32 = 3000;
static SHOWING: AtomicBool = AtomicBool::new(false);

/// Show each screen's number; does nothing while a previous round is still up.
pub fn show(screens: Vec<(Rect, u32)>) {
    if screens.is_empty() || SHOWING.swap(true, Ordering::SeqCst) {
        return;
    }
    let started = std::thread::Builder::new().name("display-identify".into()).spawn(move || {
        run(&screens);
        SHOWING.store(false, Ordering::SeqCst);
    });
    if started.is_err() {
        SHOWING.store(false, Ordering::SeqCst);
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: GDI calls on this window's own paint DC; the number was stored at creation.
    unsafe {
        match msg {
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut r = RECT::default();
                let _ = GetClientRect(hwnd, &mut r);
                let back = CreateSolidBrush(COLORREF(0x00_26_20_20));
                FillRect(dc, &r, back);
                let _ = DeleteObject(back.into());
                let font = CreateFontW(
                    -(r.bottom * 7 / 10), 0, 0, 0, 600, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                    CLEARTYPE_QUALITY, 0, w!("Segoe UI"),
                );
                let old = SelectObject(dc, font.into());
                SetBkMode(dc, TRANSPARENT);
                SetTextColor(dc, COLORREF(0x00_FF_FF_FF));
                let n = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
                let mut text: Vec<u16> = n.to_string().encode_utf16().collect();
                DrawTextW(dc, &mut text, &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                SelectObject(dc, old);
                let _ = DeleteObject(font.into());
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_TIMER => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn run(screens: &[(Rect, u32)]) {
    // SAFETY: windows and a class of this thread, destroyed before it ends.
    unsafe {
        let class = w!("ShellmanIdentify");
        let wc = WNDCLASSW { lpfnWndProc: Some(proc), lpszClassName: class, ..Default::default() };
        RegisterClassW(&wc);
        let mut made = Vec::new();
        for (r, number) in screens {
            let edge = (r.w.min(r.h) / 5).max(80);
            let (x, y) = (r.x + (r.w - edge) / 2, r.y + (r.h - edge) / 2);
            let ex = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
            let Ok(h) = CreateWindowExW(ex, class, w!(""), WS_POPUP, x, y, edge, edge, None, None, None, None) else { continue };
            SetWindowLongPtrW(h, GWLP_USERDATA, *number as isize);
            let _ = SetWindowRgn(h, Some(CreateRoundRectRgn(0, 0, edge + 1, edge + 1, edge / 6, edge / 6)), true);
            let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
            made.push(h);
        }
        if let Some(first) = made.first() {
            SetTimer(Some(*first), 1, SHOWN_FOR_MS, None);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        for h in made {
            let _ = DestroyWindow(h);
        }
        let _ = UnregisterClassW(class, None);
    }
}
