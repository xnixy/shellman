//! Screen space for the bars when Explorer is not running.
//!
//! Without Explorer nobody keeps the list of app bars, so the work area (where windows maximize)
//! is set directly, monitor by monitor: Windows applies a new work area to the monitor that holds
//! it. Each monitor's is put back to its whole screen when the bar exits or stops using it, but
//! only while that monitor is still there with the same size; a work area for a monitor that has
//! gone would otherwise land on whichever monitor is nearest.
//!
//! Explorer also hides minimized windows (they live on the taskbar); without a shell, Windows
//! draws them as small title bars at the bottom-left of the desktop. The same setting Explorer
//! uses hides them while the bar runs; it takes effect once the tray window is registered as the
//! shell's task window, and for windows minimized after that (at sign-in, that is all of them).

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONULL};
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, MINIMIZEDMETRICS, SPI_GETMINIMIZEDMETRICS, SPI_SETMINIMIZEDMETRICS,
    SPI_SETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

/// ARW_HIDE: minimized windows are not drawn on the desktop.
const ARW_HIDE: i32 = 0x0008;

/// SPIF_SENDCHANGE: tell running apps, so maximized windows resize at once.
const SEND_CHANGE: SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS = SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(2);

/// One monitor's work area, with space taken off its top and bottom.
pub struct WorkArea {
    monitor: RECT,
    wanted: RECT,
}

impl WorkArea {
    /// Keep `top` and `bottom` physical pixels of `monitor` free of maximized windows.
    pub fn reserve(monitor: RECT, top: i32, bottom: i32) -> Self {
        let wanted = RECT {
            top: monitor.top + top,
            bottom: monitor.bottom - bottom,
            ..monitor
        };
        set(wanted);
        Self { monitor, wanted }
    }

    /// Set it again if something else changed it. True if it had to. An Explorer that Winlogon
    /// starts for a moment (when the modern-app host dies, see `shellman_core::app_host`) sets
    /// every monitor's work area for its own taskbar, and nothing else would put shellman's back.
    /// Only called once no Explorer taskbar is left (see `Keeper`).
    pub fn reassert(&self) -> bool {
        let now = work_area(self.monitor);
        trace(&format!("monitor {:?}: work area {:?}, wanted {:?}", rect(self.monitor), now.map(rect), rect(self.wanted)));
        if now.is_some_and(|now| now != self.wanted) {
            set(self.wanted);
            return true;
        }
        false
    }
}

fn rect(r: RECT) -> [i32; 4] {
    [r.left, r.top, r.right, r.bottom]
}

/// The work area of the monitor with exactly this rectangle.
fn work_area(monitor: RECT) -> Option<RECT> {
    let handle = unsafe { MonitorFromRect(&monitor, MONITOR_DEFAULTTONULL) };
    if handle.is_invalid() {
        return None;
    }
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let ok = unsafe { GetMonitorInfoW(handle, &mut info) }.as_bool() && info.rcMonitor == monitor;
    ok.then_some(info.rcWork)
}

impl Drop for WorkArea {
    fn drop(&mut self) {
        if still_there(self.monitor) {
            set(self.monitor);
        }
    }
}

/// Is there still a monitor with exactly this rectangle?
fn still_there(monitor: RECT) -> bool {
    work_area(monitor).is_some()
}

fn set(mut rect: RECT) {
    unsafe {
        let _ = SystemParametersInfoW(SPI_SETWORKAREA, 0, Some((&raw mut rect).cast()), SEND_CHANGE);
    }
}

/// Minimized windows hidden while this lives; the previous setting comes back when dropped.
pub struct HiddenMinimized {
    before: MINIMIZEDMETRICS,
}

impl HiddenMinimized {
    pub fn start() -> Self {
        let mut m = MINIMIZEDMETRICS {
            cbSize: size_of::<MINIMIZEDMETRICS>() as u32,
            ..Default::default()
        };
        unsafe {
            let _ = SystemParametersInfoW(SPI_GETMINIMIZEDMETRICS, m.cbSize, Some((&raw mut m).cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
            let before = m;
            m.iArrange.0 |= ARW_HIDE;
            let _ = SystemParametersInfoW(SPI_SETMINIMIZEDMETRICS, m.cbSize, Some((&raw mut m).cast()), SEND_CHANGE);
            Self { before }
        }
    }
}

impl Drop for HiddenMinimized {
    fn drop(&mut self) {
        let mut m = self.before;
        unsafe {
            let _ = SystemParametersInfoW(SPI_SETMINIMIZEDMETRICS, m.cbSize, Some((&raw mut m).cast()), SEND_CHANGE);
        }
    }
}

/// The one keeper: the host's timer decides with it, the bars record their rounds in it.
pub static KEEPER: std::sync::Mutex<Keeper> = std::sync::Mutex::new(Keeper::new());

/// When to put shellman's work areas back after someone else changed them.
///
/// The usual someone is an Explorer that Winlogon starts when a shell host dies: it sets every
/// monitor's work area for its taskbar, possibly several times, and again whenever the work area
/// changes under it. Fighting it while it runs would ping-pong, so nothing is put back while an
/// `explorer.exe` owns a `Shell_TrayWnd` (the bar owns one too, so it goes by process); once none
/// is left and things have been quiet for a moment, the work areas are put back. And whatever the
/// other party is, at most `MAX_ROUNDS` rounds of putting back in `WINDOW`, then a pause of
/// `PAUSE`.
pub struct Keeper {
    rounds: std::collections::VecDeque<std::time::Instant>,
    paused_until: Option<std::time::Instant>,
}

/// After the last work-area change (or Explorer taskbar coming or going), wait this long.
pub const QUIET: std::time::Duration = std::time::Duration::from_millis(400);
/// While an Explorer taskbar is there, look again this often.
pub const WHILE_EXPLORER: std::time::Duration = std::time::Duration::from_millis(1000);
const MAX_ROUNDS: usize = 5;
const WINDOW: std::time::Duration = std::time::Duration::from_secs(30);
const PAUSE: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// Put the work areas back now.
    Reassert,
    /// An Explorer taskbar runs: look again in `WHILE_EXPLORER`.
    Wait,
    /// Too many rounds lately: leave the work areas alone until the pause is over.
    Paused,
}

impl Keeper {
    pub const fn new() -> Self {
        Self { rounds: std::collections::VecDeque::new(), paused_until: None }
    }

    /// Things have been quiet for `QUIET`: what now?
    pub fn decide(&mut self, explorer_taskbar: bool, now: std::time::Instant) -> Verdict {
        if explorer_taskbar {
            return Verdict::Wait;
        }
        if self.paused_until.is_some_and(|until| now < until) {
            return Verdict::Paused;
        }
        self.paused_until = None;
        while self.rounds.front().is_some_and(|&t| now.duration_since(t) > WINDOW) {
            self.rounds.pop_front();
        }
        if self.rounds.len() >= MAX_ROUNDS {
            self.paused_until = Some(now + PAUSE);
            self.rounds.clear();
            return Verdict::Paused;
        }
        Verdict::Reassert
    }

    /// A round put at least one work area back.
    pub fn reasserted(&mut self, now: std::time::Instant) {
        self.rounds.push_back(now);
    }
}

/// Does an `explorer.exe` own a `Shell_TrayWnd`? (Its taskbar runs, or is starting.)
pub fn explorer_taskbar() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, GetWindowThreadProcessId};
    let me = std::process::id();
    let mut after = None;
    for _ in 0..64 {
        let Ok(hwnd) = (unsafe { FindWindowExW(None, after, w!("Shell_TrayWnd"), None) }) else { break };
        if hwnd.is_invalid() {
            break;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid != me && crate::tray::log::process_name(pid).eq_ignore_ascii_case("explorer.exe") {
            trace(&format!("Shell_TrayWnd {:?} belongs to explorer.exe pid {pid}", hwnd.0));
            return true;
        }
        after = Some(HWND(hwnd.0));
    }
    false
}

/// Optional trace of work-area decisions: set `SHELLMAN_WORKAREA_LOG=<file>` before starting the
/// bar.
pub fn trace(text: &str) {
    use std::io::Write;
    use std::sync::OnceLock;
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    let Some(path) = PATH.get_or_init(|| std::env::var("SHELLMAN_WORKAREA_LOG").ok().filter(|p| !p.is_empty())) else {
        return;
    };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        let _ = writeln!(f, "{}.{:03} {text}", t.as_secs(), t.subsec_millis());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn waits_while_explorer_runs() {
        let mut k = Keeper::new();
        let t = Instant::now();
        assert_eq!(k.decide(true, t), Verdict::Wait);
        assert_eq!(k.decide(false, t), Verdict::Reassert);
    }

    #[test]
    fn pauses_after_too_many_rounds() {
        let mut k = Keeper::new();
        let t = Instant::now();
        for i in 0..MAX_ROUNDS as u64 {
            let now = t + Duration::from_secs(i);
            assert_eq!(k.decide(false, now), Verdict::Reassert);
            k.reasserted(now);
        }
        let now = t + Duration::from_secs(MAX_ROUNDS as u64);
        assert_eq!(k.decide(false, now), Verdict::Paused);
        assert_eq!(k.decide(false, now + PAUSE - Duration::from_secs(1)), Verdict::Paused);
        assert_eq!(k.decide(false, now + PAUSE), Verdict::Reassert);
    }

    #[test]
    fn old_rounds_are_forgotten() {
        let mut k = Keeper::new();
        let t = Instant::now();
        for i in 0..MAX_ROUNDS as u64 {
            k.reasserted(t + Duration::from_secs(i));
        }
        assert_eq!(k.decide(false, t + WINDOW + Duration::from_secs(MAX_ROUNDS as u64)), Verdict::Reassert);
    }

    #[test]
    fn explorer_wins_over_pause() {
        let mut k = Keeper::new();
        let t = Instant::now();
        for _ in 0..MAX_ROUNDS {
            k.reasserted(t);
        }
        assert_eq!(k.decide(true, t), Verdict::Wait);
    }
}