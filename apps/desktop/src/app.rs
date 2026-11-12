//! The desktop process: a hidden controller window, one wallpaper window per monitor, and a
//! worker thread that prepares the pictures.
//!
//! Nothing runs until Windows says something changed: the message loop sleeps, and the controller
//! wakes for a wallpaper change (`WM_SETTINGCHANGE`), a monitor change (`WM_DISPLAYCHANGE`, or a
//! DPI change told by a wallpaper window) or a resume from sleep. Decoding happens on a worker
//! thread; the UI thread only swaps in the finished bitmaps.

use crate::decode::{self, Target};
use crate::fit::Area;
use crate::monitors;
use crate::source;
use crate::surface::{self, Surface, WM_REFRESH_DISPLAY};
use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW, PostQuitMessage,
    RegisterClassW, TranslateMessage, MSG, WINDOW_EX_STYLE, WM_DESTROY, WM_DISPLAYCHANGE, WM_POWERBROADCAST,
    WM_SETTINGCHANGE, WNDCLASSW, WS_POPUP,
};

/// The worker finished: `wparam` is the generation it worked for.
const WM_READY: u32 = 0x8000 + 1;
/// SPI_SETDESKWALLPAPER, as sent in `WM_SETTINGCHANGE`.
const SPI_SETDESKWALLPAPER: usize = 0x0014;
/// PBT_APMRESUMEAUTOMATIC: the machine woke from sleep.
const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;

/// The controller window, so wallpaper windows can reach it.
static CONTROLLER: AtomicIsize = AtomicIsize::new(0);

/// The worker's finished surfaces, waiting for the UI thread.
static READY: Mutex<Option<(u32, Vec<Surface>)>> = Mutex::new(None);

struct State {
    monitors: Vec<Area>,
    windows: Vec<HWND>,
    /// Bumped for every worker run; results for an older number are dropped.
    generation: u32,
}

thread_local! {
    static STATE: RefCell<State> = const { RefCell::new(State { monitors: Vec::new(), windows: Vec::new(), generation: 0 }) };
}

pub fn controller() -> Option<HWND> {
    let h = CONTROLLER.load(Ordering::Relaxed);
    (h != 0).then_some(HWND(h as *mut _))
}

/// Create the windows and run until the process is closed.
pub fn run() {
    unsafe {
        surface::register();
        let class = register_class();
        let Ok(ctl) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("shellman desktop controller"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            GetModuleHandleW(None).ok().map(Into::into),
            None,
        ) else {
            return;
        };
        CONTROLLER.store(ctl.0 as isize, Ordering::Relaxed);
        refresh(ctl);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Registers the controller's window class and returns its name.
unsafe fn register_class() -> windows::core::PCWSTR {
    let name = w!("ShellmanDesktopController");
    let class = WNDCLASSW {
        lpfnWndProc: Some(proc),
        hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
        lpszClassName: name,
        ..Default::default()
    };
    RegisterClassW(&class);
    name
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_DISPLAYCHANGE | WM_REFRESH_DISPLAY => {
            refresh(hwnd);
            LRESULT(0)
        }
        WM_SETTINGCHANGE if wparam.0 == SPI_SETDESKWALLPAPER => {
            refresh(hwnd);
            LRESULT(0)
        }
        WM_POWERBROADCAST if wparam.0 == PBT_APMRESUMEAUTOMATIC => {
            refresh(hwnd);
            LRESULT(0)
        }
        WM_READY => {
            install();
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Bring the windows in line with the monitors (re-creating them if those changed), then start
/// preparing the pictures.
fn refresh(ctl: HWND) {
    let now = monitors::list();
    let generation = STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.monitors != now || s.windows.is_empty() {
            for w in s.windows.drain(..) {
                unsafe {
                    let _ = DestroyWindow(w);
                }
            }
            s.windows = now.iter().filter_map(|m| surface::create(*m)).collect();
            s.monitors = now.clone();
        }
        s.generation = s.generation.wrapping_add(1);
        s.generation
    });
    let ctl = ctl.0 as isize;
    std::thread::spawn(move || prepare(generation, now, ctl));
}

/// Worker: read the settings, decode and scale each monitor's picture, hand the lot over.
fn prepare(generation: u32, monitors: Vec<Area>, ctl: isize) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let settings = source::read(&monitors);
    let factory = decode::factory();
    let union = monitors::union(&monitors);
    let surfaces: Vec<Surface> = monitors
        .iter()
        .zip(settings)
        .map(|(mon, setting)| {
            let target = Target { mon: *mon, union, fit: setting.fit };
            let image = match (&factory, &setting.path) {
                (Some(f), Some(path)) => decode::render(f, path, &target),
                _ => None,
            };
            Surface { color: setting.color, image }
        })
        .collect();
    drop(factory);
    unsafe {
        CoUninitialize();
    }
    let stale = READY.lock().ok().and_then(|mut slot| slot.replace((generation, surfaces)));
    if let Some((_, old)) = stale {
        old.into_iter().for_each(Surface::discard);
    }
    unsafe {
        let _ = PostMessageW(Some(HWND(ctl as *mut _)), WM_READY, WPARAM(generation as usize), LPARAM(0));
    }
}

/// UI thread: show the worker's result if it is still the latest.
fn install() {
    let Some((got, surfaces)) = READY.lock().ok().and_then(|mut slot| slot.take()) else { return };
    STATE.with(|s| {
        let s = s.borrow();
        if got == s.generation && surfaces.len() == s.windows.len() {
            for (hwnd, surface) in s.windows.iter().zip(surfaces) {
                surface::set(*hwnd, surface);
            }
        } else {
            surfaces.into_iter().for_each(Surface::discard);
        }
    });
}
