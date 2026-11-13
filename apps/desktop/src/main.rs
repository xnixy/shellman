#![windows_subsystem = "windows"]
//! Shellman's desktop: the wallpaper. One plain window per monitor, always at the bottom of the
//! z-order, painted once and then left alone. What sits on the wallpaper (the shelf) and its
//! right-click menu are the shelf process's (pps/shelf); a right-click here is handed to it.

mod app;
mod decode;
mod fit;
mod frames;
mod monitors;
mod source;
mod surface;

use windows::core::HSTRING;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};

fn main() {
    unsafe {
        // Real pixels on every monitor, so the picture is scaled to what the screen really has.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let has = |flag: &str| std::env::args().any(|a| a == flag);
        // `--frames-reset`: hand every window's frame colours back to Windows, and exit. For
        // cleaning up after a frames service that was killed before it could.
        if has("--frames-reset") {
            shellman_core::frames::reset_all_windows();
            return;
        }
        // `--frames-only`: just the frames service, no wallpaper (so a test copy can run next to
        // the real desktop). It has a mutex of its own.
        let frames_only = has("--frames-only");
        // One wallpaper per session: a second copy just exits. The handle lives until exit. A test
        // copy (`SHELLMAN_INSTANCE`) has a name of its own, so it can run beside the real one.
        let base = if frames_only { "Local\\shellman-desktop-frames" } else { "Local\\shellman-desktop" };
        let name = match std::env::var("SHELLMAN_INSTANCE").ok().filter(|i| !i.is_empty()) {
            Some(instance) => HSTRING::from(format!("{base}-{instance}")),
            None => HSTRING::from(base),
        };
        let _single = CreateMutexW(None, false, &name);
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        // Windows' host for modern apps (Settings, Calculator, the Store), kept running while
        // shellman is the shell. The desktop carries it because it is the plainest and longest
        // lived of the apps, and the first to start. `--no-shell` (or a SHELLMAN_INSTANCE test
        // copy) leaves it out.
        if !frames_only {
            shellman_core::app_host::start(has("--no-shell"));
            // The shelf (what sits on the wallpaper, and its right-click menu) is a process of its
            // own. The session starts and restarts it; run without one, the desktop starts it.
            // A shelf already running makes the new one exit at once.
            if std::env::var_os("SHELLMAN_SESSION").is_none() {
                std::thread::spawn(|| shellman_core::sibling::start("shelf"));
            }
        }
        app::run(frames_only);
    }
}
