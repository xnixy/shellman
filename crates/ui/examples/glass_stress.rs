#![windows_subsystem = "windows"]
//! Drives one `GlassPanel` through hostile sizes and transitions, to prove the glass never hands
//! DWM an impossible size. Not part of the shell.
//!
//! `glass_stress <x> <y> [rounds]`, physical pixels: put it on a secondary monitor. One round
//! walks every step below, 60 ms apart; the window is at most 400x300 except for the steps that
//! ask for absurd shapes (which the panel must clamp or hide). Afterwards check
//! `Get-WinEvent -FilterHashtable @{LogName='Application'; Id=1000}` for a new `dwm.exe` entry.
//! Lines go to `glass_stress.log` beside the exe; `SHELLMAN_GLASS_LOG=1` adds the panel's own.

use egui::{pos2, Rect};
use shellman_ui::glass::{GlassPanel, Island, Style, REFRESH_MESSAGE};
use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW, PostMessageW,
    RegisterClassW, RegisterWindowMessageW, SendMessageW, SetWindowPos, ShowWindow, MSG,
    PM_REMOVE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SW_HIDE, SW_MINIMIZE, SW_RESTORE,
    SW_SHOWNOACTIVATE, WM_DPICHANGED, WNDCLASSW, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

fn report(line: &str) {
    use std::io::Write;
    let path = std::env::current_exe().unwrap().with_file_name("glass_stress.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> windows::Win32::Foundation::LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Run the window's messages for `ms` milliseconds.
fn pump(ms: u64) {
    let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < until {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn rect(l: i32, t: i32, r: i32, b: i32) -> RECT {
    RECT { left: l, top: t, right: r, bottom: b }
}

fn size(hwnd: HWND, w: i32, h: i32) {
    unsafe {
        let _ = SetWindowPos(hwnd, None, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n = |i: usize, d: i32| args.get(i).and_then(|a| a.parse().ok()).unwrap_or(d);
    let (x, y, rounds) = (n(1, -3000), n(2, 300), n(3, 1));
    // A windowed app has no console: panics go to the log too.
    std::panic::set_hook(Box::new(|info| report(&format!("PANIC: {info}"))));
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(None).unwrap();
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance.into(),
            lpszClassName: w!("GlassStress"),
            ..Default::default()
        });
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            w!("GlassStress"),
            w!("glass stress"),
            WS_POPUP,
            x,
            y,
            400,
            300,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();
        let refresh = RegisterWindowMessageW(&HSTRING::from(REFRESH_MESSAGE));
        let mut panel = Some(GlassPanel::attach(hwnd.0 as isize, Style::from_theme(24.0)));
        report(&format!("start at {x},{y}: composed {}", panel.as_ref().unwrap().composed()));
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        pump(300);
        let island = |l: f32, t: f32, r: f32, b: f32, radius: f32| Island::new(Rect::from_min_max(pos2(l, t), pos2(r, b)), radius);
        for round in 0..rounds {
            let p = || panel.as_ref().unwrap();
            let step = |name: &str, f: &dyn Fn()| {
                f();
                pump(60);
                report(&format!("round {round}: {name}"));
            };
            step("0x0", &|| size(hwnd, 0, 0));
            step("1x1", &|| size(hwnd, 1, 1));
            step("10x10, smaller than the shadow room", &|| size(hwnd, 10, 10));
            step("400x300", &|| size(hwnd, 400, 300));
            step("shape inverted", &|| p().set_shape(Some(rect(300, 200, 10, 10)), None));
            step("shape empty", &|| p().set_shape(Some(rect(50, 50, 50, 50)), None));
            step("shape huge", &|| p().set_shape(Some(rect(-100_000, -100_000, 100_000, 100_000)), None));
            step("shape extreme", &|| p().set_shape(Some(rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX)), Some(rect(0, 0, 5, 5))));
            step("shape normal", &|| p().set_shape(Some(rect(20, 20, 380, 280)), None));
            step("whole window", &|| p().set_shape(None, None));
            step("islands zero-size", &|| p().set_islands(&[island(10.0, 10.0, 10.0, 10.0, 8.0), island(50.0, 50.0, 40.0, 40.0, 8.0)], 1.0));
            step("islands NaN", &|| p().set_islands(&[island(f32::NAN, 0.0, 100.0, f32::NAN, f32::NAN)], f32::NAN));
            step("islands huge", &|| p().set_islands(&[island(-1e9, -1e9, 1e9, 1e9, 1e9)], 1.0));
            step("islands scale 0", &|| p().set_islands(&[island(10.0, 10.0, 200.0, 100.0, 12.0)], 0.0));
            step("islands scale inf", &|| p().set_islands(&[island(10.0, 10.0, 200.0, 100.0, 12.0)], f32::INFINITY));
            step("islands radius past the piece", &|| p().set_islands(&[island(10.0, 10.0, 60.0, 30.0, 500.0)], 1.0));
            step("islands only, empty", &|| p().set_islands_only(&[], 1.0, None));
            step("islands normal", &|| p().set_islands(&[island(10.0, 10.0, 390.0, 60.0, 24.0), island(10.0, 80.0, 390.0, 290.0, 16.0)], 1.0));
            step("one shape again", &|| p().set_shape(None, None));
            step("minimise", &|| { let _ = ShowWindow(hwnd, SW_MINIMIZE); });
            step("restore", &|| { let _ = ShowWindow(hwnd, SW_RESTORE); });
            step("hide", &|| { let _ = ShowWindow(hwnd, SW_HIDE); });
            step("show", &|| { let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE); });
            step("resize while shrinking to nothing", &|| {
                for w in (0..=400).rev().step_by(25) {
                    size(hwnd, w, w * 3 / 4);
                    pump(5);
                }
                size(hwnd, 400, 300);
            });
            step("fake DPI change", &|| {
                let r = rect(x, y, x + 400, y + 300);
                let _ = SendMessageW(hwnd, WM_DPICHANGED, Some(WPARAM(0x0090_0090)), Some(LPARAM(&r as *const _ as isize)));
            });
            step("refresh x20 (rebuilds coalesced and spaced)", &|| {
                for _ in 0..20 {
                    let _ = PostMessageW(Some(hwnd), refresh, WPARAM(0), LPARAM(0));
                    pump(10);
                }
            });
            // Attaching per frame: the panel must stop making backdrop windows after a burst.
            let mut composed = 0;
            for _ in 0..30 {
                drop(panel.take());
                panel = Some(GlassPanel::attach(hwnd.0 as isize, Style::from_theme(24.0)));
                composed += usize::from(panel.as_ref().unwrap().composed());
                pump(16);
            }
            report(&format!("round {round}: attached 30 times in ~0.5 s, composed {composed}"));
            pump(1000);
        }
        // Let the rate limit refill, then show the glass is still fine.
        pump(3000);
        drop(panel.take());
        panel = Some(GlassPanel::attach(hwnd.0 as isize, Style::from_theme(24.0)));
        report(&format!("end: composed {}", panel.as_ref().unwrap().composed()));
        pump(1500);
        drop(panel);
        let _ = DestroyWindow(hwnd);
        report("done");
    }
}
