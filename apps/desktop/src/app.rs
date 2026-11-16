//! The desktop process: a hidden controller window, one wallpaper window per monitor, and a
//! worker thread that prepares the pictures.
//!
//! Nothing runs until Windows says something changed: the message loop sleeps, and the controller
//! wakes for a wallpaper change (`WM_SETTINGCHANGE`), a monitor change (`WM_DISPLAYCHANGE`, or a
//! DPI change told by a wallpaper window) or a resume from sleep. Decoding happens on a worker
//! thread; the UI thread only swaps in the finished bitmaps.

use crate::decode::{self, Target};
use crate::fit::Area;
use crate::frames;
use crate::monitors;
use crate::source;
use crate::frames::WM_FRAMES_POLL;
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
/// Sent to top-level windows when the Windows accent colour changes.
const WM_DWMCOLORIZATIONCOLORCHANGED: u32 = 0x0320;
/// PBT_APMRESUMEAUTOMATIC: the machine woke from sleep.
const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;

/// The controller window, so wallpaper windows can reach it.
static CONTROLLER: AtomicIsize = AtomicIsize::new(0);

/// The worker's finished surfaces, waiting for the UI thread.
static READY: Mutex<Option<(u64, Vec<Surface>)>> = Mutex::new(None);

struct State {
    monitors: Vec<Area>,
    windows: Vec<HWND>,
    jobs: Jobs,
    /// The session handed over to Explorer and told the wallpaper to stand aside
    /// (`core::desktop_aside`): the windows are kept, with their pictures, but hidden.
    aside: bool,
}

thread_local! {
    static STATE: RefCell<State> =
        const { RefCell::new(State { monitors: Vec::new(), windows: Vec::new(), jobs: Jobs::new(), aside: false }) };
}

/// The picture workers, coalesced: at most one runs at a time. A refresh while one runs only
/// marks the work as pending; when the running one finishes, one more run is made for the
/// latest state. A result is shown only when it is for the latest generation (the windows it
/// was made for are the ones that exist). One monitor or DPI change on three monitors used to
/// start three or more 4K decodes at once, and an older one finishing last overwrote the newer
/// result, which was then dropped as stale: the new windows stayed black until the next change.
struct Jobs {
    /// The latest generation asked for (bumped by every refresh).
    current: u64,
    /// The generation a worker is preparing right now.
    running: Option<u64>,
    /// A refresh came while a worker ran: run once more when it finishes.
    pending: bool,
}

impl Jobs {
    const fn new() -> Jobs {
        Jobs { current: 0, running: None, pending: false }
    }

    /// A refresh: the generation to start a worker for now, or `None` when one runs already.
    fn ask(&mut self) -> Option<u64> {
        self.current += 1;
        if self.running.is_some() {
            self.pending = true;
            return None;
        }
        self.running = Some(self.current);
        Some(self.current)
    }

    /// The worker for `generation` finished: whether its result is current, and the generation
    /// to start the next worker for, if one is pending.
    fn done(&mut self, generation: u64) -> (bool, Option<u64>) {
        if self.running == Some(generation) {
            self.running = None;
        }
        let rerun = (self.running.is_none() && std::mem::take(&mut self.pending)).then(|| {
            self.running = Some(self.current);
            self.current
        });
        (generation == self.current, rerun)
    }
}

/// Put a finished result in the slot, unless the slot already holds a newer one.
fn offer<T>(slot: &mut Option<(u64, T)>, generation: u64, value: T) {
    if slot.as_ref().is_none_or(|(g, _)| *g < generation) {
        *slot = Some((generation, value));
    }
}

pub fn controller() -> Option<HWND> {
    let h = CONTROLLER.load(Ordering::Relaxed);
    (h != 0).then_some(HWND(h as *mut _))
}

/// Create the windows and run until the process is closed. With `frames_only` there is no
/// wallpaper, just the controller and the frames service.
pub fn run(frames_only: bool) {
    unsafe {
        if !frames_only {
            surface::register();
        }
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
        if !frames_only {
            refresh(ctl);
        }
        frames::start(ctl);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        // Windows we restyled go back to their own colours.
        frames::stop();
    }
}

/// Registers the controller's window class and returns its name.
unsafe fn register_class() -> windows::core::PCWSTR {
    let name = shellman_core::desktop_aside::CONTROLLER_CLASS;
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
        WM_FRAMES_POLL => {
            frames::poll();
            LRESULT(0)
        }
        // Windows' light/dark or accent changed (other changes make the service find no difference).
        WM_SETTINGCHANGE | WM_DWMCOLORIZATIONCOLORCHANGED => {
            frames::refresh();
            LRESULT(0)
        }
        WM_POWERBROADCAST if wparam.0 == PBT_APMRESUMEAUTOMATIC => {
            refresh(hwnd);
            LRESULT(0)
        }
        WM_READY => {
            install(hwnd, wparam.0 as u64);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        // A registered message, so not a constant to match on.
        m if m == shellman_core::desktop_aside::message() => {
            stand_aside(wparam.0 != 0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Bring the windows in line with the monitors (re-creating them if those changed), then start
/// preparing the pictures, or mark that as pending while a worker runs (`Jobs`).
fn refresh(ctl: HWND) {
    let now = monitors::list();
    let start = STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.monitors != now || s.windows.is_empty() {
            for w in s.windows.drain(..) {
                unsafe {
                    let _ = DestroyWindow(w);
                }
            }
            let shown = !s.aside;
            s.windows = now.iter().filter_map(|m| surface::create(*m, shown)).collect();
            s.monitors = now.clone();
        }
        s.jobs.ask()
    });
    if let Some(generation) = start {
        spawn_worker(generation, now, ctl);
    }
}

/// Hide the wallpaper windows for Explorer's desktop, or show them again. Windows made
/// meanwhile (a monitor change) follow it too.
fn stand_aside(aside: bool) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.aside = aside;
        for w in &s.windows {
            surface::show(*w, !aside);
        }
    });
}

fn spawn_worker(generation: u64, monitors: Vec<Area>, ctl: HWND) {
    let ctl = ctl.0 as isize;
    let started = std::thread::Builder::new().name("wallpaper".into()).spawn(move || {
        // Whatever happens, the UI thread hears that this generation is over, or no worker
        // would ever run again.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| prepare(generation, monitors)));
        unsafe {
            let _ = PostMessageW(Some(HWND(ctl as *mut _)), WM_READY, WPARAM(generation as usize), LPARAM(0));
        }
    });
    if started.is_err() {
        unsafe {
            let _ = PostMessageW(Some(HWND(ctl as *mut _)), WM_READY, WPARAM(generation as usize), LPARAM(0));
        }
    }
}

/// Worker: read the settings, decode and scale each monitor's picture, hand the lot over.
fn prepare(generation: u64, monitors: Vec<Area>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let settings = source::read(&monitors);
    let factory = decode::factory();
    let union = monitors::union(&monitors);
    let mut rendered = decode::Rendered::default();
    let surfaces: Vec<Surface> = monitors
        .iter()
        .zip(settings)
        .map(|(mon, setting)| {
            let target = Target { mon: *mon, union, fit: setting.fit };
            let image = match (&factory, &setting.path) {
                (Some(f), Some(path)) => rendered.render(f, path, &target),
                _ => None,
            };
            Surface { color: setting.color, image }
        })
        .collect();
    drop(rendered);
    drop(factory);
    unsafe {
        CoUninitialize();
    }
    if let Ok(mut slot) = READY.lock() {
        offer(&mut slot, generation, surfaces);
    }
}

/// UI thread: the worker for `generation` is over. Show its result if it is still the latest,
/// and start the pending run if a refresh came meanwhile.
fn install(ctl: HWND, generation: u64) {
    let result = READY.lock().ok().and_then(|mut slot| match slot.as_ref() {
        Some((g, _)) if *g <= generation => slot.take(),
        _ => None,
    });
    let rerun = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let (current, rerun) = s.jobs.done(generation);
        if let Some((got, surfaces)) = result {
            if current && got == generation && surfaces.len() == s.windows.len() {
                for (hwnd, surface) in s.windows.iter().zip(surfaces) {
                    surface::set(*hwnd, surface);
                }
            }
        }
        rerun.map(|g| (g, s.monitors.clone()))
    });
    if let Some((g, monitors)) = rerun {
        spawn_worker(g, monitors, ctl);
    }
}

#[cfg(test)]
mod tests {
    use super::{offer, Jobs};

    /// A fake decoder: each worker's "picture" is the monitor layout it was started for.
    struct Desk {
        jobs: Jobs,
        layout: u32,
        /// Workers running: (generation, layout they decode for).
        workers: Vec<(u64, u32)>,
        slot: Option<(u64, u32)>,
        shown: Option<u32>,
        started: usize,
    }

    impl Desk {
        fn new() -> Desk {
            Desk { jobs: Jobs::new(), layout: 0, workers: Vec::new(), slot: None, shown: None, started: 0 }
        }
        fn refresh(&mut self, layout: u32) {
            self.layout = layout;
            if let Some(g) = self.jobs.ask() {
                self.workers.push((g, layout));
                self.started += 1;
            }
        }
        /// Worker `i` (in start order) finishes and the UI thread hears of it.
        fn finish(&mut self, i: usize) {
            let (g, layout) = self.workers.remove(i);
            offer(&mut self.slot, g, layout);
            let result = match self.slot {
                Some((sg, _)) if sg <= g => self.slot.take(),
                _ => None,
            };
            let (current, rerun) = self.jobs.done(g);
            if let Some((got, picture)) = result {
                if current && got == g {
                    self.shown = Some(picture);
                }
            }
            if let Some(g) = rerun {
                self.workers.push((g, self.layout));
                self.started += 1;
            }
        }
    }

    #[test]
    fn refreshes_during_a_decode_coalesce_into_one_more_run() {
        let mut d = Desk::new();
        d.refresh(1);
        // A monitor change on three monitors: several refreshes while the first decode runs.
        d.refresh(2);
        d.refresh(2);
        d.refresh(3);
        assert_eq!(d.workers.len(), 1, "one decode at a time");
        d.finish(0);
        assert_eq!(d.shown, None, "the first result is for windows that are gone");
        assert_eq!(d.workers, [(4, 3)], "one more run, for the latest layout");
        d.finish(0);
        assert_eq!(d.shown, Some(3));
        assert!(d.workers.is_empty());
        assert_eq!(d.started, 2);
    }

    #[test]
    fn a_quiet_refresh_shows_its_result() {
        let mut d = Desk::new();
        d.refresh(1);
        d.finish(0);
        assert_eq!(d.shown, Some(1));
        d.refresh(1);
        d.finish(0);
        assert_eq!(d.shown, Some(1));
        assert_eq!(d.started, 2);
    }

    #[test]
    fn an_older_result_never_replaces_a_newer_one() {
        let mut slot = None;
        offer(&mut slot, 5, "new");
        offer(&mut slot, 4, "old");
        assert_eq!(slot, Some((5, "new")));
        offer(&mut slot, 6, "newer");
        assert_eq!(slot, Some((6, "newer")));
    }

    #[test]
    fn a_worker_that_failed_still_frees_the_next_run() {
        let mut j = Jobs::new();
        assert_eq!(j.ask(), Some(1));
        assert_eq!(j.ask(), None);
        // Generation 1 ended without a result (a panic in the decoder): the pending run starts.
        assert_eq!(j.done(1), (false, Some(2)));
        assert_eq!(j.done(2), (true, None));
        assert_eq!(j.ask(), Some(3));
    }
}
