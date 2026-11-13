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
//!
//! Work-area changes and Explorer's `TaskbarCreated` also start a timer: once things are quiet
//! and no Explorer taskbar runs, shellman's work areas are put back (`workarea::Keeper`).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::core::w;
use windows::Win32::UI::WindowsAndMessaging::{
    KillTimer, RegisterWindowMessageW, SetTimer, SPI_SETWORKAREA, WM_DISPLAYCHANGE, WM_DPICHANGED,
    WM_SETTINGCHANGE, WM_TIMER,
};

static CTX: OnceLock<egui::Context> = OnceLock::new();
/// The monitors (or something about them) changed since the last layout.
static LAYOUT: AtomicBool = AtomicBool::new(true);
/// Explorer said app bar positions changed: ask it again where the bars may go.
static REQUERY: AtomicBool = AtomicBool::new(false);
/// The work areas are to be put back (if someone else changed them) on the next layout.
static REASSERT: AtomicBool = AtomicBool::new(false);
/// The registered "TaskbarCreated" message: an Explorer taskbar just started (or a shell host
/// was replaced and the tray re-announced).
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
/// The work-area timer's id on the host window.
const WORK_AREA_TIMER: usize = 0x5741;
/// Windows said the power status changed (on battery, charging, the level): the top bars read
/// the battery again.
static POWER: AtomicBool = AtomicBool::new(false);
/// Repaints the top bars (set once at the start).
static TOPS: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
/// The clock jumped (set by hand, a new time zone, daylight saving) or the machine woke: what
/// shows the time or counts from it is drawn again, and the calendar read again.
static CLOCK: AtomicBool = AtomicBool::new(false);
const WM_POWERBROADCAST: u32 = 0x0218;
const WM_TIMECHANGE: u32 = 0x001E;
const PBT_APMRESUMESUSPEND: usize = 0x0007;
const PBT_APMPOWERSTATUSCHANGE: usize = 0x000A;
const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;

/// What a message to the host says changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// On battery or not, the level.
    Power,
    /// The time, or the machine woke (wait timeouts do not count sleep, so the minute timer is
    /// late after it).
    Clock,
}

/// Which change a message is, if any.
pub fn change_of(msg: u32, wparam: usize) -> Option<Change> {
    match (msg, wparam) {
        (WM_TIMECHANGE, _) => Some(Change::Clock),
        (WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND) => Some(Change::Clock),
        (WM_POWERBROADCAST, PBT_APMPOWERSTATUSCHANGE) => Some(Change::Power),
        _ => None,
    }
}

/// True once after the clock jumped or the machine woke.
pub fn take_clock() -> bool {
    CLOCK.swap(false, Ordering::AcqRel)
}

/// How the host repaints the top bars when Windows tells it something they show changed.
pub fn set_wake_tops(wake: impl Fn() + Send + Sync + 'static) {
    let _ = TOPS.set(Box::new(wake));
}

fn wake_tops() {
    if let Some(wake) = TOPS.get() {
        wake();
    }
}

/// True once after the power status changed.
pub fn take_power() -> bool {
    POWER.swap(false, Ordering::AcqRel)
}

/// Make `hwnd`, eframe's main window, the host: never shown, never focused, and listening.
pub fn start(hwnd: isize, ctx: egui::Context) {
    let _ = CTX.set(ctx);
    crate::shape::never_activate(hwnd);
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        let on: u32 = 1;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_CLOAK, (&raw const on).cast(), 4);
        let _ = SetWindowSubclass(hwnd, Some(host_proc), 2, 0);
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Release);
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

/// Run the host's next frame without laying anything out (it opens or closes the preview panel).
/// Safe from any thread.
pub fn request_frame() {
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

/// True once after the work-area timer decided the work areas are to be put back.
pub fn take_reassert() -> bool {
    REASSERT.swap(false, Ordering::AcqRel)
}

/// (Re)start the work-area timer: it fires `after` the last call.
fn watch_work_area(hwnd: HWND, after: std::time::Duration) {
    unsafe { SetTimer(Some(hwnd), WORK_AREA_TIMER, after.as_millis() as u32, None) };
}

/// The work-area timer fired: decide, and either ask for a layout that puts the work areas back,
/// or look again while an Explorer taskbar runs.
fn work_area_timer(hwnd: HWND) {
    use crate::workarea::{self, Verdict};
    unsafe {
        let _ = KillTimer(Some(hwnd), WORK_AREA_TIMER);
    }
    let explorer = workarea::explorer_taskbar();
    let verdict = workarea::KEEPER.lock().map(|mut k| k.decide(explorer, std::time::Instant::now()));
    workarea::trace(&format!("quiet; Explorer taskbar: {explorer}; {verdict:?}"));
    match verdict {
        Ok(Verdict::Reassert) => {
            REASSERT.store(true, Ordering::Release);
            request_layout();
        }
        Ok(Verdict::Wait) => watch_work_area(hwnd, workarea::WHILE_EXPLORER),
        _ => {}
    }
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
        WM_SETTINGCHANGE if wparam.0 == SPI_SETWORKAREA.0 as usize => {
            crate::workarea::trace("work area changed");
            watch_work_area(hwnd, crate::workarea::QUIET);
            request_layout();
        }
        m if change_of(m, wparam.0).is_some() => {
            match change_of(m, wparam.0) {
                Some(Change::Power) => POWER.store(true, Ordering::Release),
                _ => {
                    // The battery may have changed while asleep, too.
                    POWER.store(true, Ordering::Release);
                    CLOCK.store(true, Ordering::Release);
                    request_frame();
                }
            }
            wake_tops();
        }
        WM_TIMER if wparam.0 == WORK_AREA_TIMER => {
            work_area_timer(hwnd);
            return LRESULT(0);
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Acquire) => {
            crate::workarea::trace("TaskbarCreated");
            watch_work_area(hwnd, crate::workarea::QUIET);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_changes_and_wakes_are_told_apart_from_power() {
        assert_eq!(change_of(WM_TIMECHANGE, 0), Some(Change::Clock));
        assert_eq!(change_of(WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC), Some(Change::Clock));
        assert_eq!(change_of(WM_POWERBROADCAST, PBT_APMRESUMESUSPEND), Some(Change::Clock));
        assert_eq!(change_of(WM_POWERBROADCAST, PBT_APMPOWERSTATUSCHANGE), Some(Change::Power));
        // Going to sleep (PBT_APMSUSPEND) and other messages change nothing shown.
        assert_eq!(change_of(WM_POWERBROADCAST, 0x0004), None);
        assert_eq!(change_of(WM_DISPLAYCHANGE, 0), None);
    }
}