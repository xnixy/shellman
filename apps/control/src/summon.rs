//! Appearing on request, and only one control centre at a time.
//!
//! The same pattern as the launcher's (copied rather than shared while the launcher is being
//! reworked; the two could move to `shellman_core` together). The process stays resident with
//! its window hidden. Running `shellman-control.exe` again finds the first one through a named
//! mutex, signals it through a named event, and exits at once; the resident process then shows
//! its window, or hides it if it is showing. `SHELLMAN_INSTANCE` is appended to both names, so a
//! test copy can run beside the real one.

use crate::place;
use shellman_core::outside;
use shellman_ui::glass;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::HSTRING;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, RedrawWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST, RDW_INTERNALPAINT,
};
use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject, INFINITE};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, GetCursorPos, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, ASFW_ANY, GWL_EXSTYLE, GWL_STYLE,
    HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE,
    SW_SHOWNOACTIVATE, WS_CAPTION, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
};

/// The environment variable that names a separate instance, for testing.
pub const INSTANCE_VAR: &str = "SHELLMAN_INSTANCE";
/// A summon this soon after a click outside closed the panel is that same click reaching the
/// top bar's button: it closes, it does not reopen.
const CLICK_CLOSE_GRACE: Duration = Duration::from_millis(500);

/// Whether this process is the resident one; if not, the resident one has been signalled and
/// this process has nothing more to do.
pub enum Claim {
    Resident(Instance),
    Signalled,
}

/// The resident process's end of the signal.
pub struct Instance {
    event: HANDLE,
    /// Keeps the mutex alive for as long as the process runs.
    _mutex: HANDLE,
}

/// What the window and the signal thread tell each other.
#[derive(Default)]
pub struct Shared {
    /// Whether the panel is showing (it is never truly hidden: see `park`).
    showing: AtomicBool,
    hide_requested: AtomicBool,
    /// Counts showings, so the window notices a new one.
    shown: AtomicU32,
    /// The monitor the panel shows on, `(left, top, right, bottom)` in pixels, and its scale.
    monitor: Mutex<Option<Monitor>>,
    /// The content's height in points, as last laid out (f32 bits).
    height: AtomicU32,
    /// Frames still to lay out before the panel is let out from behind the cloak.
    reveal: AtomicU32,
    /// When a click outside last closed the panel.
    closed_by_click: Mutex<Option<Instant>>,
}

impl Shared {
    pub fn take_hide_request(&self) -> bool {
        self.hide_requested.swap(false, Ordering::AcqRel)
    }

    pub fn shown_count(&self) -> u32 {
        self.shown.load(Ordering::Acquire)
    }

    pub fn closed_by_click(&self) {
        if let Ok(mut at) = self.closed_by_click.lock() {
            *at = Some(Instant::now());
        }
    }

    fn height(&self) -> f32 {
        f32::from_bits(self.height.load(Ordering::Relaxed))
    }
}

/// How many frames a showing waits behind the cloak: the first ones are laid out for a size and
/// style the window has not settled on, and Windows would show them (a caption bar, a panel at
/// the wrong height) for an instant.
const REVEAL_AFTER: u32 = 3;

/// A monitor's rectangle `(left, top, right, bottom)` in pixels, and its scale.
type Monitor = ((i32, i32, i32, i32), f32);

/// The mutex and event names, with the instance's suffix.
fn names(instance: Option<&str>) -> (String, String) {
    let suffix = instance.filter(|s| !s.is_empty()).map(|s| format!("-{s}")).unwrap_or_default();
    (
        format!("Local\\shellman-control-summon{suffix}"),
        format!("Local\\shellman-control-resident{suffix}"),
    )
}

/// Become the resident process, or signal the one that is. Both sides create the event before
/// the mutex, so whichever order two starts interleave in, the one that loses the mutex finds
/// the event.
pub fn claim(background: bool) -> Claim {
    let instance = std::env::var(INSTANCE_VAR).ok();
    let (event_name, mutex_name) = names(instance.as_deref());
    unsafe {
        let Ok(event) = CreateEventW(None, false, false, &HSTRING::from(event_name)) else {
            // Cannot coordinate; run on our own rather than not at all.
            return Claim::Resident(Instance { event: HANDLE::default(), _mutex: HANDLE::default() });
        };
        let mutex = CreateMutexW(None, false, &HSTRING::from(mutex_name));
        if mutex.is_ok() && GetLastError() == ERROR_ALREADY_EXISTS {
            // The resident process may take the foreground for the window it is about to show.
            // A `--background` start only asks for a resident to exist: it is there, so no
            // signal (which would pop the panel open).
            if !background {
                let _ = AllowSetForegroundWindow(ASFW_ANY);
                let _ = SetEvent(event);
            }
            return Claim::Signalled;
        }
        Claim::Resident(Instance { event, _mutex: mutex.unwrap_or_default() })
    }
}

impl Instance {
    /// Show the window now, as if the exe had been run again.
    pub fn summon(&self) {
        if !self.event.is_invalid() {
            unsafe {
                let _ = SetEvent(self.event);
            }
        }
    }

    /// Serve summons for the window `hwnd` from a thread of its own, until the process ends.
    pub fn serve(&self, hwnd: isize, ctx: egui::Context, shared: Arc<Shared>) {
        if self.event.is_invalid() {
            return;
        }
        let event = self.event.0 as isize;
        std::thread::spawn(move || {
            let event = HANDLE(event as *mut _);
            let hwnd = HWND(hwnd as *mut _);
            // The event wakes this thread, so nothing polls while the panel is hidden.
            while unsafe { WaitForSingleObject(event, INFINITE) }.0 == 0 {
                let just_closed = shared
                    .closed_by_click
                    .lock()
                    .ok()
                    .and_then(|mut at| at.take())
                    .is_some_and(|at| at.elapsed() < CLICK_CLOSE_GRACE);
                if shared.showing.load(Ordering::Acquire) {
                    shared.hide_requested.store(true, Ordering::Release);
                } else if !just_closed {
                    show(hwnd, &shared);
                    shared.shown.fetch_add(1, Ordering::AcqRel);
                }
                ctx.request_repaint();
            }
        });
    }
}

/// Put the panel out of sight. Called by the window itself.
pub fn hide(hwnd: isize, shared: &Shared) {
    shared.showing.store(false, Ordering::Release);
    shared.reveal.store(0, Ordering::Release);
    park(hwnd);
}

/// Set the window up as resident and out of sight: cloaked, and a tool window, so it is never
/// listed as a running app. Called once, before the window library first shows it.
pub fn prepare(hwnd: isize) {
    glass::cloak(hwnd, true);
    tool_window(HWND(hwnd as *mut _));
    shellman_ui::park::shrink(hwnd);
}

/// Out of sight, yet never truly hidden. Windows sends a hidden window no paint messages, and
/// the window library then retries a frame it was asked for (by the last click, say) without
/// pause, a core spinning for as long as the panel is hidden. So the window is cloaked (with its
/// glass), and hidden and shown again without activation, which hands the focus back to the
/// window below: it still paints, so pending frames finish and the UI goes idle.
pub fn park(hwnd: isize) {
    glass::cloak(hwnd, true);
    // Out of sight, its buffers go (`shellman_ui::park`); `show` sets the size again.
    shellman_ui::park::shrink(hwnd);
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        // A frame asked for while hidden was dropped; ask again.
        let _ = RedrawWindow(Some(hwnd), None, None, RDW_INTERNALPAINT);
    }
}

/// A tool window, never an app window: the dock and the window list leave it out. (The window
/// library's "no taskbar button" only removes the button.)
fn tool_window(hwnd: HWND) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let tool = (ex & !WS_EX_APPWINDOW.0) | WS_EX_TOOLWINDOW.0;
        if tool != ex {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, tool as isize);
        }
    }
}

/// Show the panel on the monitor under the pointer, and take the keyboard focus.
fn show(hwnd: HWND, shared: &Shared) {
    unsafe {
        let mut at = POINT::default();
        let _ = GetCursorPos(&mut at);
        let monitor = MonitorFromPoint(at, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let (mut dpi_x, mut dpi_y) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let scale = dpi_x as f32 / 96.0;
        // The panel hangs just under the top bar, which reserves its space (islands and the gap
        // below them) out of the work area, and stops at the work area's bottom, so it never
        // covers the tray island or runs under the dock. On a monitor without a top bar it keeps
        // a gap from the screen's edge.
        let (m, work) = (info.rcMonitor, info.rcWork);
        let top = if work.top > m.top { work.top } else { m.top + (place::GAP * scale).round() as i32 };
        let rect = (m.left, top, m.right, work.bottom.min(m.bottom));
        if let Ok(mut slot) = shared.monitor.lock() {
            *slot = Some((rect, scale));
        }
        let r = place::panel_rect(rect, scale, shared.height());
        // Placed while still cloaked, so it is never seen where it was or at a passing size.
        glass::cloak(hwnd.0 as isize, true);
        strip_frame(hwnd);
        tool_window(hwnd);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), r.x, r.y, r.w, r.h, SWP_SHOWWINDOW | SWP_FRAMECHANGED);
        // Still cloaked: `fit` lets it out once a few frames have been laid out at its size.
        shared.reveal.store(REVEAL_AFTER, Ordering::Release);
        shared.showing.store(true, Ordering::Release);
        outside::set_rect(r.x, r.y, r.w, r.h);
        take_foreground(hwnd);
    }
}

/// Keep the window where it belongs for content `height` points tall: called on every frame,
/// it corrects a height change, and Windows resizing the window when it lands on a monitor with
/// another scale.
pub fn fit(hwnd: isize, shared: &Shared, height: f32, ctx: &egui::Context) {
    shared.height.store(height.to_bits(), Ordering::Relaxed);
    let Some((rect, scale)) = shared.monitor.lock().ok().and_then(|m| *m) else {
        return;
    };
    let want = place::panel_rect(rect, scale, height);
    let hwnd = HWND(hwnd as *mut _);
    // The islands fill the whole client area, so a caption the window library gave the window
    // back would cover the top island: take it off again, with the frame recalculated.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        if style & (WS_CAPTION.0 | WS_THICKFRAME.0) != 0 {
            strip_frame(hwnd);
            let _ = SetWindowPos(
                hwnd,
                None,
                want.x,
                want.y,
                want.w,
                want.h,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }
    let mut now = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut now);
        let have = place::Px { x: now.left, y: now.top, w: now.right - now.left, h: now.bottom - now.top };
        if have != want {
            let _ = SetWindowPos(hwnd, None, want.x, want.y, want.w, want.h, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
    outside::set_rect(want.x, want.y, want.w, want.h);
    // Let the panel out once it has settled; until then keep the frames coming.
    let waiting = shared.reveal.load(Ordering::Acquire);
    if waiting > 0 {
        shared.reveal.store(waiting - 1, Ordering::Release);
        if waiting == 1 {
            glass::cloak(hwnd.0 as isize, false);
        }
        ctx.request_repaint();
    }
}

/// Windows lets a process take the foreground only in narrow cases, chiefly when it sent the
/// last input; the top bar that summons the panel never takes the focus, so the first request
/// is often refused. Then one press of a key no keyboard has (the switcher's trick) makes this
/// process the last input's sender, and the second request is granted.
unsafe fn take_foreground(hwnd: HWND) {
    let _ = SetForegroundWindow(hwnd);
    if GetForegroundWindow() == hwnd {
        return;
    }
    send_mask();
    let _ = SetForegroundWindow(hwnd);
}

/// One press and release of VK 0xE8, which is unassigned.
fn send_mask() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0xE8), dwFlags: flags, ..Default::default() },
        },
    };
    let inputs = [key(Default::default()), key(KEYEVENTF_KEYUP)];
    unsafe {
        SendInput(&inputs, size_of::<INPUT>() as i32);
    }
}

/// No caption, system menu or sizing frame: over glass, Windows would otherwise draw its
/// caption buttons into the panel. Done on every showing, since the window library re-applies
/// its own styles now and then.
fn strip_frame(hwnd: HWND) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let bare = style & !(WS_CAPTION.0 | WS_SYSMENU.0 | WS_THICKFRAME.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0);
        SetWindowLongPtrW(hwnd, GWL_STYLE, bare as isize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instances_get_their_own_names() {
        let (event, mutex) = names(None);
        assert_eq!(event, "Local\\shellman-control-summon");
        assert_eq!(mutex, "Local\\shellman-control-resident");
        assert_eq!(names(Some("")), names(None));
        let (event, mutex) = names(Some("test"));
        assert!(event.ends_with("-summon-test") && mutex.ends_with("-resident-test"));
    }
}
