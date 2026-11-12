#![windows_subsystem = "windows"]
//! Shows one kind of glass for a few seconds, to look at and capture. Not part of the shell.
//!
//! `glass_probe <mode> <x> <y> <w> <h> <radius px> [seconds] [dx]`, all in physical pixels.
//! Modes:
//! - `panel`: shellman's `GlassPanel` (composition, host backdrop) under an empty window.
//!   With `dx`, the window moves right by `dx` half-way through, to see the panel follow.
//! - `accent`: the undocumented accent policy (acrylic blur-behind) plus a rounded window region.
//! - `system`: DWM's system backdrop (transient/acrylic) plus a rounded window region.

use shellman_ui::glass::{GlassPanel, Style};
use shellman_ui::theme;
use windows::core::{s, w, BOOL};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_TRANSIENTWINDOW,
    DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
    SelectObject, SetBkMode, SetTextColor, DT_CENTER, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, PostQuitMessage,
    RegisterClassW, SetTimer, SetWindowPos, ShowWindow, HWND_TOPMOST, MSG, SWP_NOACTIVATE,
    SWP_NOSIZE, SW_SHOWNOACTIVATE, WM_TIMER, WNDCLASSW, WS_EX_NOREDIRECTIONBITMAP,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, WM_PAINT};

#[repr(C)]
struct AccentPolicy {
    state: u32,
    flags: u32,
    gradient: u32,
    animation: u32,
}

#[repr(C)]
struct CompositionData {
    attribute: u32,
    data: *mut core::ffi::c_void,
    size: usize,
}

type SetWindowCompositionAttribute = unsafe extern "system" fn(HWND, *mut CompositionData) -> BOOL;

static mut MOVE: (i32, i32, i32) = (0, 0, 0);

/// A line in `glass_probe.log` next to the exe (a windowed app has no console to print to).
fn report(line: &str) {
    use std::io::Write;
    let path = std::env::current_exe()
        .unwrap()
        .with_file_name("glass_probe.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{line}");
    }
}

/// Paints bright stripes and large text, so blur (or its absence) is easy to see.
unsafe fn paint_stripes(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let dc = BeginPaint(hwnd, &mut ps);
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let colors = [
        0x00FFFFFF, 0x00E89A3C, 0x0000D7FF, 0x00101010, 0x004763FF, 0x0090EE90,
    ];
    let band = (rc.right / colors.len() as i32).max(1);
    for (i, c) in colors.iter().enumerate() {
        let r = RECT {
            left: band * i as i32,
            top: 0,
            right: band * (i as i32 + 1),
            bottom: rc.bottom,
        };
        let brush = CreateSolidBrush(COLORREF(*c));
        FillRect(dc, &r, brush);
        let _ = DeleteObject(brush.into());
    }
    let font = CreateFontW(
        120,
        0,
        0,
        0,
        700,
        0,
        0,
        0,
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        0,
        w!("Segoe UI"),
    );
    SelectObject(dc, font.into());
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, COLORREF(0));
    let mut text: Vec<u16> = "Backdrop text BEHIND the glass 0123"
        .encode_utf16()
        .collect();
    let mut tr = RECT {
        top: rc.bottom / 3,
        ..rc
    };
    DrawTextW(dc, &mut text, &mut tr, DT_CENTER);
    let _ = DeleteObject(font.into());
    let _ = EndPaint(hwnd, &ps);
}

unsafe extern "system" fn stripes_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_PAINT {
        paint_stripes(hwnd);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_TIMER {
        if wparam.0 == 1 {
            PostQuitMessage(0);
        } else {
            let _ = KillTimer(Some(hwnd), 2);
            let (x, y, dx) = MOVE;
            let _ = SetWindowPos(hwnd, None, x + dx, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        }
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args
        .get(1)
        .map(String::as_str)
        .unwrap_or("panel")
        .to_string();
    let n = |i: usize, d: i32| args.get(i).and_then(|a| a.parse().ok()).unwrap_or(d);
    let (x, y, w_, h, radius) = (n(2, 200), n(3, 200), n(4, 1080), n(5, 840), n(6, 36));
    let seconds = n(7, 6);
    let dx = n(8, 0);
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(None).unwrap();
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance.into(),
            lpszClassName: w!("GlassProbe"),
            ..Default::default()
        };
        RegisterClassW(&class);
        if mode == "stripes" {
            let class = WNDCLASSW {
                lpfnWndProc: Some(stripes_proc),
                hInstance: instance.into(),
                lpszClassName: w!("GlassProbeStripes"),
                ..Default::default()
            };
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                w!("GlassProbeStripes"),
                w!("glass probe stripes"),
                WS_POPUP,
                x,
                y,
                w_,
                h,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .unwrap();
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            SetTimer(Some(hwnd), 1, seconds as u32 * 1000, None);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_TIMER {
                    break;
                }
                DispatchMessageW(&msg);
            }
            return;
        }
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            w!("GlassProbe"),
            w!("glass probe"),
            WS_POPUP,
            x,
            y,
            w_,
            h,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();
        let dark: u32 = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw const dark).cast(),
            4,
        );
        let mut _panel = None;
        match mode.as_str() {
            "accent" => {
                let user32 = LoadLibraryW(w!("user32.dll")).unwrap();
                let f = GetProcAddress(user32, s!("SetWindowCompositionAttribute")).unwrap();
                let f: SetWindowCompositionAttribute = std::mem::transmute(f);
                // ACCENT_ENABLE_ACRYLICBLURBEHIND, a dark tint (0xAABBGGRR).
                let mut accent = AccentPolicy {
                    state: 4,
                    flags: 2,
                    gradient: 0x99201c1c,
                    animation: 0,
                };
                let mut data = CompositionData {
                    attribute: 19,
                    data: (&raw mut accent).cast(),
                    size: size_of::<AccentPolicy>(),
                };
                report(&format!("accent: {:?}", f(hwnd, &mut data)));
                let region = CreateRoundRectRgn(0, 0, w_ + 1, h + 1, radius * 2, radius * 2);
                SetWindowRgn(hwnd, Some(region), true);
            }
            "system" => {
                let margins = MARGINS {
                    cxLeftWidth: -1,
                    cxRightWidth: -1,
                    cyTopHeight: -1,
                    cyBottomHeight: -1,
                };
                let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
                let v = DWMSBT_TRANSIENTWINDOW.0;
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_SYSTEMBACKDROP_TYPE,
                    (&raw const v).cast(),
                    4,
                );
                let region = CreateRoundRectRgn(0, 0, w_ + 1, h + 1, radius * 2, radius * 2);
                SetWindowRgn(hwnd, Some(region), true);
            }
            _ => {
                // PROBE_LIGHT=1 for light mode; PROBE_GLASS=<0..1> for the glass amount.
                theme::set_light(std::env::var("PROBE_LIGHT").is_ok_and(|v| v == "1"));
                if let Some(g) = std::env::var("PROBE_GLASS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                {
                    theme::set_glass(&egui::Context::default(), g);
                }
                let style = Style::from_theme(24.0);
                _panel = GlassPanel::attach(hwnd.0 as isize, style);
                report(&format!("panel: {}", _panel.is_some()));
            }
        }
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w_, h, SWP_NOACTIVATE);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(Some(hwnd), 1, seconds as u32 * 1000, None);
        if dx != 0 {
            MOVE = (x, y, dx);
            SetTimer(Some(hwnd), 2, seconds as u32 * 500, None);
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    }
}
