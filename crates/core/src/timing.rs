//! Opening-speed milestones, for measuring how fast things open. Off unless `SHELLMAN_TIMING=1`;
//! off, every call is one relaxed atomic load.
//!
//! Each line goes to `timing-<app>.log` in the data folder (`%LOCALAPPDATA%\shellman`, or the
//! `SHELLMAN_CONFIG_DIR` folder for a test copy):
//!
//! `<filetime> <pid> <app> +<ms since the process was created> <what> [detail]`
//!
//! The filetime is the precise system time in 100 ns units, the same clock as the process's
//! creation time, so a driver can line up its own request times with the app's milestones.
//! "Since the process was created" uses `GetProcessTimes`, so the loader, static
//! initialisation and anything an antivirus does before `main` count.

use std::io::Write;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

/// 0 not decided yet, 1 off, 2 on.
static STATE: AtomicU8 = AtomicU8::new(0);
static SINK: OnceLock<Sink> = OnceLock::new();

struct Sink {
    app: String,
    created: u64,
    file: Mutex<Option<std::fs::File>>,
}

/// Whether timing is on (`SHELLMAN_TIMING=1`).
#[inline]
pub fn enabled() -> bool {
    match STATE.load(Ordering::Relaxed) {
        0 => decide(),
        s => s == 2,
    }
}

#[cold]
fn decide() -> bool {
    let on = std::env::var_os("SHELLMAN_TIMING").is_some_and(|v| v == "1");
    STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
    on
}

/// Start the log for `app`: writes the process's creation and `main entered`. Call first thing
/// in `main`. Without it, marks are dropped.
pub fn init(app: &str) {
    if !enabled() {
        return;
    }
    let created = creation_time().unwrap_or_else(now);
    let path = crate::config::DIR_OVERRIDE;
    let dir = match std::env::var_os(path).filter(|d| !d.is_empty()) {
        Some(d) => Some(std::path::PathBuf::from(d)),
        None => dirs::data_local_dir().map(|d| d.join("shellman")),
    };
    let file = dir.and_then(|d| {
        let _ = std::fs::create_dir_all(&d);
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(d.join(format!("timing-{app}.log")))
            .ok()
    });
    let _ = SINK.set(Sink { app: app.to_owned(), created, file: Mutex::new(file) });
    write(created, "process created", "");
    mark("main entered");
}

/// The precise system time, in 100 ns units since 1601.
pub fn now() -> u64 {
    let ft = unsafe { GetSystemTimePreciseAsFileTime() };
    ft_u64(ft)
}

fn ft_u64(ft: FILETIME) -> u64 {
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}

fn creation_time() -> Option<u64> {
    let (mut c, mut e, mut k, mut u) = Default::default();
    unsafe { GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) }.ok()?;
    Some(ft_u64(c))
}

/// A milestone, now.
#[inline]
pub fn mark(what: &str) {
    if enabled() {
        write(now(), what, "");
    }
}

/// A milestone with a detail, which is only formatted when timing is on.
#[inline]
pub fn mark_with(what: &str, detail: impl FnOnce() -> String) {
    if enabled() {
        write(now(), what, &detail());
    }
}

/// A milestone at a time taken earlier (from `now`).
pub fn mark_at(at: u64, what: &str) {
    if enabled() {
        write(at, what, "");
    }
}

/// A milestone with this process's page faults and working set, for telling a process paged
/// back in after idle from one that stayed resident.
pub fn mark_mem(what: &str) {
    if enabled() {
        let t = now();
        write(t, what, &memory());
    }
}

fn memory() -> String {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    let mut c = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) }.is_ok();
    if !ok {
        return String::new();
    }
    format!(
        "faults={} ws={}KB",
        c.PageFaultCount,
        c.WorkingSetSize / 1024
    )
}

fn write(at: u64, what: &str, detail: &str) {
    let Some(sink) = SINK.get() else { return };
    let since = (at as i64 - sink.created as i64) as f64 / 10_000.0;
    let line = format!(
        "{at} {} {} +{since:.1} {what}{}{detail}\n",
        std::process::id(),
        sink.app,
        if detail.is_empty() { "" } else { " | " }
    );
    if let Ok(mut f) = sink.file.lock() {
        if let Some(f) = f.as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

/// Time a stage: logs `<what> <ms>` when dropped. Free when timing is off.
pub struct Span {
    what: &'static str,
    start: Option<Instant>,
}

#[inline]
pub fn span(what: &'static str) -> Span {
    Span { what, start: enabled().then(Instant::now) }
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            write(now(), self.what, &format!("took {ms:.1} ms"));
        }
    }
}

/// Log `<what> presented` once the desktop compositor has composed a frame after this call
/// (waited for on a thread of its own): the first time what was just painted can be on screen.
/// Two compositions are waited for, so it is an upper bound by at most one refresh.
pub fn mark_presented(what: &str) {
    if !enabled() {
        return;
    }
    let what = format!("{what} presented");
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::Graphics::Dwm::DwmFlush();
            let _ = windows::Win32::Graphics::Dwm::DwmFlush();
        }
        write(now(), &what, "");
    });
}

/// A window message's time (`GetMessageTime`, milliseconds of `GetTickCount`) as a filetime, for
/// logging when a click or key that summoned something happened.
pub fn from_tick(tick_ms: u32) -> u64 {
    use windows::Win32::System::SystemInformation::GetTickCount;
    let ago = unsafe { GetTickCount() }.wrapping_sub(tick_ms) as u64;
    now().saturating_sub(ago * 10_000)
}
