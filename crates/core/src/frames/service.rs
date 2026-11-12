//! The frames service: WinEvent hooks of its own and the list of windows it has restyled.
//!
//! Everything lives on the thread that called `start`, which must pump messages (the hooks are
//! out-of-context, so Windows delivers them through that queue). Nothing runs between events:
//! a new window costs one look at its facts and five DWM calls; a foreground change costs two
//! border swaps and one restyle of the new front window.

use super::apply::{self, Outcome};
use super::colors::{border_for, Palette};
use super::eligible::{eligible, is_own_exe, WindowFacts};
use super::settings::{is_excluded, FramesSettings};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetAncestor, GetClassNameW, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowThreadProcessId, IsWindow, IsZoomed, EVENT_OBJECT_CREATE, EVENT_OBJECT_SHOW,
    EVENT_SYSTEM_FOREGROUND, GA_ROOT, GWL_EXSTYLE, GWL_STYLE, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    WS_CAPTION,
};

struct State {
    settings: FramesSettings,
    palette: Palette,
    /// The window in front, as last heard.
    front: isize,
    /// Windows this service has styled, so they can be handed back.
    applied: HashSet<isize>,
    /// Windows that refused (elevated): not tried again.
    denied: HashSet<isize>,
    /// Folders whose programs are shellman's own.
    own_dirs: Vec<PathBuf>,
    hooks: Vec<HWINEVENTHOOK>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Hook up and style every window there is. Call on a thread that pumps messages.
pub fn start(settings: FramesSettings, palette: Palette) {
    let mut own_dirs: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        own_dirs.push(dir);
    }
    if let Some(local) = dirs::data_local_dir() {
        own_dirs.push(local.join("Programs").join("shellman"));
    }
    let hooks = [
        (EVENT_OBJECT_CREATE, EVENT_OBJECT_CREATE),
        (EVENT_OBJECT_SHOW, EVENT_OBJECT_SHOW),
        (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
    ]
    .map(|(min, max)| unsafe {
        SetWinEventHook(min, max, None, Some(on_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS)
    })
    .to_vec();
    let front = root(unsafe { GetForegroundWindow() }).0 as isize;
    let state = State {
        settings,
        palette,
        front,
        applied: HashSet::new(),
        denied: HashSet::new(),
        own_dirs,
        hooks,
    };
    STATE.with(|s| *s.borrow_mut() = Some(state));
    update_all();
}

/// New settings or a new palette (a changed appearance, accent or light/dark): restyle everything,
/// or hand the windows back when frames were switched off.
pub fn update(settings: FramesSettings, palette: Palette) {
    let changed = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let Some(st) = s.as_mut() else { return false };
        let changed = st.settings != settings || st.palette != palette;
        st.settings = settings;
        st.palette = palette;
        changed
    });
    if changed {
        update_all();
    }
}

/// Unhook and give every window we styled back its colours.
pub fn stop() {
    let Some(mut st) = STATE.with(|s| s.borrow_mut().take()) else { return };
    for h in st.hooks.drain(..) {
        unsafe {
            let _ = UnhookWinEvent(h);
        }
    }
    for hwnd in st.applied.drain() {
        reset_one(hwnd);
    }
}

/// Without a running service: hand back the colours of every window that looks like one we would
/// have styled. For cleaning up after a service that was killed before it could stop.
pub fn reset_all_windows() {
    for hwnd in top_level_windows() {
        if unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            apply::reset(hwnd);
        }
    }
}

fn reset_one(hwnd: isize) {
    let hwnd = HWND(hwnd as *mut _);
    if unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        apply::reset(hwnd);
    }
}

/// Bring every window in line with the current settings.
fn update_all() {
    let Some((enabled, front)) = STATE.with(|s| s.borrow().as_ref().map(|st| (st.settings.enabled, st.front))) else {
        return;
    };
    let mut seen = HashSet::new();
    if enabled {
        for hwnd in top_level_windows() {
            if consider(hwnd, front) {
                seen.insert(hwnd.0 as isize);
            }
        }
    }
    // Windows styled earlier that no longer qualify (closed, excluded, switched off).
    let stale: Vec<isize> = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let Some(st) = s.as_mut() else { return Vec::new() };
        let stale: Vec<isize> = st.applied.iter().copied().filter(|h| !seen.contains(h)).collect();
        for h in &stale {
            st.applied.remove(h);
        }
        stale
    });
    stale.into_iter().for_each(reset_one);
}

fn top_level_windows() -> Vec<HWND> {
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        (*(lparam.0 as *mut Vec<HWND>)).push(hwnd);
        true.into()
    }
    let mut all: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM((&raw mut all) as isize));
    }
    all
}

fn root(hwnd: HWND) -> HWND {
    if hwnd.is_invalid() {
        hwnd
    } else {
        let r = unsafe { GetAncestor(hwnd, GA_ROOT) };
        if r.is_invalid() {
            hwnd
        } else {
            r
        }
    }
}

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    // Only whole windows: controls inside them fire the same events.
    const OBJID_WINDOW: i32 = 0;
    const CHILDID_SELF: i32 = 0;
    if hwnd.is_invalid() || id_object != OBJID_WINDOW || id_child != CHILDID_SELF {
        return;
    }
    let top = root(hwnd);
    if event == EVENT_SYSTEM_FOREGROUND {
        foreground_changed(top);
    } else if top == hwnd {
        let front = STATE.with(|s| s.borrow().as_ref().map_or(0, |st| st.front));
        consider(hwnd, front);
    }
}

/// The front window changed: the old one goes dim, the new one is styled in full (which also puts
/// right an app that reset its colours since).
fn foreground_changed(new: HWND) {
    let swap = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let st = s.as_mut()?;
        let old = std::mem::replace(&mut st.front, new.0 as isize);
        let dim = (old != new.0 as isize && st.applied.contains(&old)).then_some(old);
        Some((dim, st.settings.enabled, st.palette, st.settings.clone()))
    });
    let Some((old, enabled, palette, settings)) = swap else { return };
    if !enabled {
        return;
    }
    if let Some(old) = old {
        apply::border(HWND(old as *mut _), &palette, border_for(false, &settings));
    }
    if !new.is_invalid() {
        consider(new, new.0 as isize);
    }
}

/// Style `hwnd` if it qualifies; true when it is now styled by us.
fn consider(hwnd: HWND, front: isize) -> bool {
    let Some(facts) = Facts::of(hwnd) else { return false };
    let decision = STATE.with(|s| {
        let s = s.borrow();
        let st = s.as_ref()?;
        if !st.settings.enabled || st.denied.contains(&(hwnd.0 as isize)) {
            return None;
        }
        // Cheap facts first; the process and monitor are looked up only for likely windows.
        let basic = WindowFacts {
            style: facts.style,
            ex_style: facts.ex_style,
            cloaked: false,
            own_process: false,
            fullscreen: false,
            class: &facts.class,
        };
        if !eligible(&basic) {
            return None;
        }
        let exe = exe_of(hwnd);
        let own = exe.as_ref().is_some_and(|e| is_own_exe(e, &st.own_dirs));
        let full = WindowFacts { cloaked: is_cloaked(hwnd), own_process: own, fullscreen: is_fullscreen(hwnd, facts.style), ..basic };
        if !eligible(&full) {
            return None;
        }
        if exe.is_some_and(|e| is_excluded(&st.settings.exclude, &e.to_string_lossy())) {
            return None;
        }
        Some((st.palette, border_for(hwnd.0 as isize == front, &st.settings)))
    });
    let Some((palette, border)) = decision else { return false };
    match apply::style(hwnd, &palette, border) {
        Outcome::Done => {
            STATE.with(|s| {
                if let Some(st) = s.borrow_mut().as_mut() {
                    st.applied.insert(hwnd.0 as isize);
                    // Closed windows leave their handles behind; sweep now and then.
                    if st.applied.len() > 256 {
                        st.applied.retain(|h| unsafe { IsWindow(Some(HWND(*h as *mut _))) }.as_bool());
                    }
                }
            });
            true
        }
        Outcome::Denied => {
            STATE.with(|s| {
                if let Some(st) = s.borrow_mut().as_mut() {
                    st.denied.insert(hwnd.0 as isize);
                }
            });
            false
        }
    }
}

struct Facts {
    style: u32,
    ex_style: u32,
    class: String,
}

impl Facts {
    fn of(hwnd: HWND) -> Option<Self> {
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return None;
        }
        let mut buf = [0u16; 64];
        let n = unsafe { GetClassNameW(hwnd, &mut buf) }.max(0) as usize;
        Some(Self {
            style: unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32,
            ex_style: unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32,
            class: String::from_utf16_lossy(&buf[..n]),
        })
    }
}

fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    unsafe {
        DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&raw mut cloaked).cast(), size_of::<u32>() as u32).is_ok() && cloaked != 0
    }
}

/// Covers its whole monitor, and is not a maximized window that still has its title bar (those
/// overhang the monitor too).
fn is_fullscreen(hwnd: HWND, style: u32) -> bool {
    unsafe {
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return false;
        }
        if IsZoomed(hwnd).as_bool() && style & WS_CAPTION.0 == WS_CAPTION.0 {
            return false;
        }
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
            return false;
        }
        let m = info.rcMonitor;
        rect.left <= m.left && rect.top <= m.top && rect.right >= m.right && rect.bottom >= m.bottom
    }
}

/// The program that owns the window, if this process may ask.
fn exe_of(hwnd: HWND) -> Option<PathBuf> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == GetCurrentProcessId() {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(process);
        ok.ok()?;
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
    }
}
