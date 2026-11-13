#![windows_subsystem = "windows"]
//! Shellman's shelf: what sits on the wallpaper, and the wallpaper's right-click menu.
//!
//! The shelf is one slim island of glass on an edge of each monitor (or the primary only, as
//! Settings › Desktop says): a drop zone, then a stack per folder (Desktop, Downloads, Recent)
//! that fans open on click. It lives at desktop
//! level, above the wallpaper and below every window. The right-click menu is asked for by the
//! desktop process (see `shellman_core::desktop_menu`) and shown here, on any monitor.
//!
//! Its own process, apart from the desktop's: the desktop process stays plain Win32 (it carries
//! Windows' modern-app host and must not fall over), while this one loads egui, the GPU and the
//! shell's thumbnail and drag-and-drop handlers, any of which can crash; the session restarts it.
//!
//! `--menu x y` opens the menu at a point as it starts (the desktop starts it so when none runs).

mod actions;
mod app;
mod cards;
mod displays;
mod drag_image;
mod draw;
mod drop;
mod folders;
mod geometry;
mod log;
mod menu;
mod stacks;
mod text;
mod views;
mod wallpaper;
mod window;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_ui::appearance::Appearance;
use shellman_ui::glass::{GlassPanel, Style};
use shellman_ui::theme;
use std::sync::{Arc, Mutex};
use windows::core::{BOOL, HSTRING};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowW, GetWindowTextW, GetWindowThreadProcessId, ShowWindow, SW_SHOWNOACTIVATE,
};

/// Whether the shelf draws in light mode: Windows' app mode, or `SHELLMAN_FORCE_LIGHT` /
/// `SHELLMAN_FORCE_DARK` to look at one palette without changing Windows' setting.
pub fn light(ctx: &egui::Context) -> bool {
    if std::env::var_os("SHELLMAN_FORCE_LIGHT").is_some() {
        return true;
    }
    if std::env::var_os("SHELLMAN_FORCE_DARK").is_some() {
        return false;
    }
    ctx.theme() == egui::Theme::Light
}

/// Follow the light or dark mode in this frame.
pub fn theme_for(ctx: &egui::Context) {
    theme::set_light(light(ctx));
}

/// This process's window with the given title (egui makes extra windows without handing out
/// their handles).
pub fn own_window(title: &str) -> Option<isize> {
    struct Search<'a> {
        title: &'a str,
        pid: u32,
        found: Option<isize>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = &mut *(lparam.0 as *mut Search);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == search.pid {
            let mut buf = [0u16; 128];
            let n = GetWindowTextW(hwnd, &mut buf).max(0) as usize;
            if String::from_utf16_lossy(&buf[..n]) == search.title {
                search.found = Some(hwnd.0 as isize);
                return BOOL(0);
            }
        }
        BOOL(1)
    }
    let mut search = Search { title, pid: std::process::id(), found: None };
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM((&raw mut search) as isize));
    }
    search.found
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let menu_at = shellman_core::desktop_menu::parse_args(&args);
    // One shelf per session (per `SHELLMAN_INSTANCE` for a test copy). A second start with
    // `--menu` hands the request to the running one.
    let name = match std::env::var("SHELLMAN_INSTANCE").ok().filter(|i| !i.is_empty()) {
        Some(i) => format!("Local\\shellman-shelf-{i}"),
        None => "Local\\shellman-shelf".into(),
    };
    let _single = unsafe { CreateMutexW(None, false, &HSTRING::from(name)) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        if let Some((x, y)) = menu_at {
            if unsafe { FindWindowW(None, &HSTRING::from(shellman_core::desktop_menu::shelf_title())) }.is_ok() {
                shellman_core::desktop_menu::request(x, y);
            }
        }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(shellman_core::desktop_menu::shelf_title())
            .with_inner_size([geometry::window_width(), 600.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_taskbar(false)
            // Shown by the shelf once it is placed, without activation.
            .with_visible(false),
        ..Default::default()
    };
    eframe::run_native(
        "shellman-shelf",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            theme::set_light(light(&cc.egui_ctx));
            theme::apply(&cc.egui_ctx);
            let look = Appearance::load_or(Appearance::default());
            look.apply(&cc.egui_ctx);
            let hwnd = match cc.window_handle().map(|h| h.as_raw()) {
                Ok(RawWindowHandle::Win32(w)) => w.hwnd.get(),
                _ => return Err("the shelf has no window".into()),
            };
            window::prepare(hwnd, &cc.egui_ctx);
            let panel = GlassPanel::attach(hwnd, Style::from_theme(geometry::RADIUS));
            // Nothing shows or takes clicks until the first frame has laid the shelf out.
            panel.set_islands_only(&[], 1.0, None);
            let shared = Arc::new(Mutex::new(drop::Shared::default()));
            unsafe {
                let _ = windows::Win32::System::Ole::OleInitialize(None);
            }
            let target = drop::Target { shared: shared.clone(), ctx: cc.egui_ctx.clone() };
            if let Err(e) = shellman_core::transfer::target::register(hwnd, Box::new(target)) {
                log::line(format!("no drop target: {e}"));
            }
            unsafe {
                let _ = ShowWindow(HWND(hwnd as *mut _), SW_SHOWNOACTIVATE);
            }
            if let Some(at) = menu_at {
                window::ask_menu(at);
            }
            Ok(Box::new(app::Shelf::new(&cc.egui_ctx, hwnd, panel, look, shared)))
        }),
    )
}
