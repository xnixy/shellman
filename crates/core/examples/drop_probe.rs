//! A window that takes drops and only writes down what it was offered: every format of the data
//! object, the files in `CF_HDROP`, the effects the source allowed, and the keys held. It never
//! touches the files. For testing a drag source (Files' drag out) by hand or by script.
//!
//! `cargo run -p shellman-core --example drop_probe -- <log file> [x y w h]`

use std::cell::RefCell;
use std::io::Write;
use windows::core::{implement, w, Ref, Result};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINTL, WPARAM};
use windows::Win32::System::Com::{IDataObject, DATADIR_GET, FORMATETC};
use windows::Win32::System::DataExchange::GetClipboardFormatNameW;
use windows::Win32::System::Ole::{
    IDropTarget, IDropTarget_Impl, OleInitialize, RegisterDragDrop, DROPEFFECT,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostQuitMessage,
    RegisterClassW, TranslateMessage, CW_USEDEFAULT, MSG, WINDOW_EX_STYLE, WM_DESTROY, WNDCLASSW,
    WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

fn log(line: &str) {
    let path = std::env::args().nth(1).unwrap_or_else(|| "drop_probe.log".into());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

fn format_name(cf: u16) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClipboardFormatNameW(u32::from(cf), &mut buf) };
    if n > 0 {
        return String::from_utf16_lossy(&buf[..n as usize]);
    }
    match cf {
        1 => "CF_TEXT".into(),
        2 => "CF_BITMAP".into(),
        7 => "CF_OEMTEXT".into(),
        8 => "CF_DIB".into(),
        13 => "CF_UNICODETEXT".into(),
        15 => "CF_HDROP".into(),
        16 => "CF_LOCALE".into(),
        17 => "CF_DIBV5".into(),
        _ => format!("#{cf}"),
    }
}

fn formats(obj: &IDataObject) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(e) = (unsafe { obj.EnumFormatEtc(DATADIR_GET.0 as u32) }) else { return out };
    loop {
        let mut f = [FORMATETC::default()];
        let mut got = 0u32;
        if unsafe { e.Next(&mut f, Some(&mut got)) }.is_err() || got == 0 {
            break;
        }
        out.push(format!("{} (tymed {})", format_name(f[0].cfFormat), f[0].tymed));
    }
    out
}

#[implement(IDropTarget)]
struct Probe {
    last: RefCell<u32>,
}

impl IDropTarget_Impl for Probe_Impl {
    fn DragEnter(&self, obj: Ref<'_, IDataObject>, keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let obj = obj.ok()?;
        let allowed = unsafe { (*effect).0 };
        log(&format!("enter allowed={allowed} keys={:#x}", keys.0));
        for f in formats(obj) {
            log(&format!("  format {f}"));
        }
        for p in shellman_core::transfer::data::files(obj) {
            log(&format!("  file {}", p.display()));
        }
        unsafe { *effect = DROPEFFECT(allowed & 1) };
        Ok(())
    }

    fn DragOver(&self, keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        if keys.0 != *self.last.borrow() {
            *self.last.borrow_mut() = keys.0;
            log(&format!("over keys={:#x}", keys.0));
        }
        unsafe { *effect = DROPEFFECT((*effect).0 & 1) };
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        log("leave");
        Ok(())
    }

    fn Drop(&self, obj: Ref<'_, IDataObject>, keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let allowed = unsafe { (*effect).0 };
        let files = obj.ok().map(shellman_core::transfer::data::files).unwrap_or_default();
        log(&format!("drop allowed={allowed} keys={:#x} files={}", keys.0, files.len()));
        // Takes it as a copy and does nothing with it.
        unsafe { *effect = DROPEFFECT(allowed & 1) };
        Ok(())
    }
}

unsafe extern "system" fn proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if m == WM_DESTROY {
        PostQuitMessage(0);
        return LRESULT(0);
    }
    DefWindowProcW(h, m, wp, lp)
}

fn main() {
    let a: Vec<i32> = std::env::args().skip(2).filter_map(|s| s.parse().ok()).collect();
    let (x, y, w_, h_) = match a.as_slice() {
        [x, y, w, h] => (*x, *y, *w, *h),
        _ => (CW_USEDEFAULT, CW_USEDEFAULT, 400, 300),
    };
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
        let _ = OleInitialize(None);
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            lpszClassName: w!("shellman-drop-probe"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("shellman-drop-probe"),
            w!("Drop probe"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            x,
            y,
            w_,
            h_,
            None,
            None,
            None,
            None,
        )
        .expect("window");
        let target: IDropTarget = Probe { last: RefCell::new(0) }.into();
        RegisterDragDrop(hwnd, &target).expect("drop target");
        log("ready");
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
