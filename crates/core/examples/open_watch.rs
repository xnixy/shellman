//! Opening speed seen from outside (see docs/PLAN.md, "Opening speed"): how long from the request
//! until the window is on screen, without trusting the app's own log.
//!
//! - `open_watch summon <event> <window title>`: set the named event (as the top bar or the Win
//!   key does) and wait until the window with that title is shown and not cloaked, then for the
//!   compositor to compose once more. Prints `<ms to uncloaked> <ms to composed>`.
//! - `open_watch spawn <title> <exe> [args]`: start the exe (as `sibling::start` does) and wait
//!   the same way for the new process's window with that title. Prints the same two numbers and the start time.
//!
//! - `open_watch run <title> <exe>`: start the exe and wait for the window with that title of
//!   any process: how the top bar summoned the launcher before `core::summon` (a second process
//!   that sets the resident's event and exits).
//! - `open_watch trim <pid>...`: empty those processes' working sets, as Windows does to an idle
//!   process when memory runs short (to tell what paging back in costs).
//!
//! - `open_watch display off|on`: turn the monitors off as the display timeout does, or wake
//!   them as a mouse move does; with nothing, print the seconds since the last input.
//!
//! "On screen" is an upper bound of one refresh: the composition after the window was uncloaked.

use std::time::{Duration, Instant};
use windows::core::{BOOL, HSTRING};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DwmFlush, DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::System::Threading::{OpenEventW, SetEvent, EVENT_MODIFY_STATE};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
};

const GIVE_UP: Duration = Duration::from_secs(10);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("summon") => summon(&args[1], &args[2]),
        Some("spawn") => spawn(&args[1], &args[2], &args[3..]),
        Some("run") => run(&args[1], &args[2]),
        Some("display") => display(args.get(1).map(String::as_str)),
        Some("trim") => args[1..].iter().filter_map(|p| p.parse().ok()).for_each(trim),
        _ => eprintln!("usage: open_watch summon <event> <title> | spawn <title> <exe> [args] | run <title> <exe> | trim <pid>... | display [off|on]"),
    }
}

fn on_screen(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    unsafe {
        IsWindowVisible(hwnd).as_bool()
            && DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&raw mut cloaked).cast(), 4).is_ok()
            && cloaked == 0
    }
}

/// Wait (polling finely) until `find` gives a window on screen; then one more composition.
fn wait(t0: Instant, mut find: impl FnMut() -> Option<HWND>) {
    loop {
        if let Some(h) = find().filter(|&h| on_screen(h)) {
            let shown = t0.elapsed();
            unsafe {
                let _ = DwmFlush();
            }
            let composed = t0.elapsed();
            let _ = h;
            println!("{:.1} {:.1}", ms(shown), ms(composed));
            return;
        }
        if t0.elapsed() > GIVE_UP {
            println!("timeout");
            return;
        }
        std::thread::sleep(Duration::from_micros(250));
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn summon(event: &str, title: &str) {
    let hwnd = unsafe { FindWindowW(None, &HSTRING::from(title)) }.unwrap_or_default();
    if hwnd.is_invalid() {
        println!("no window");
        return;
    }
    if on_screen(hwnd) {
        println!("already showing");
        return;
    }
    let Ok(ev) = (unsafe { OpenEventW(EVENT_MODIFY_STATE, false, &HSTRING::from(event)) }) else {
        println!("no event");
        return;
    };
    let t0 = Instant::now();
    unsafe {
        let _ = SetEvent(ev);
        let _ = CloseHandle(ev);
    }
    wait(t0, || Some(hwnd));
}

fn spawn(title: &str, exe: &str, args: &[String]) {
    no_inherit();
    let started = shellman_core::timing::now();
    let t0 = Instant::now();
    let child = std::process::Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(child) = child else {
        println!("not started");
        return;
    };
    let pid = child.id();
    print!("{started} ");
    wait(t0, || window_of(pid, title));
}

fn run(title: &str, exe: &str) {
    no_inherit();
    let hwnd = unsafe { FindWindowW(None, &HSTRING::from(title)) }.unwrap_or_default();
    if hwnd.is_invalid() || on_screen(hwnd) {
        println!("no window, or already showing");
        return;
    }
    let t0 = Instant::now();
    if std::process::Command::new(exe).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).spawn().is_err() {
        println!("not started");
        return;
    }
    wait(t0, || Some(hwnd));
}

fn window_of(pid: u32, title: &str) -> Option<HWND> {
    struct Find {
        pid: u32,
        title: Vec<u16>,
        found: Option<HWND>,
    }
    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
        let f = unsafe { &mut *(l.0 as *mut Find) };
        let mut p = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut p)) };
        let mut buf = [0u16; 256];
        let n = unsafe { GetWindowTextW(h, &mut buf) } as usize;
        if p == f.pid && buf[..n] == f.title[..] && on_screen(h) {
            f.found = Some(h);
            return false.into();
        }
        true.into()
    }
    let mut f = Find { pid, title: title.encode_utf16().collect(), found: None };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&raw mut f as isize));
    }
    f.found
}

fn trim(pid: u32) {
    use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_SET_QUOTA};
    unsafe {
        match OpenProcess(PROCESS_SET_QUOTA | PROCESS_QUERY_INFORMATION, false, pid) {
            Ok(h) => {
                let ok = EmptyWorkingSet(h).is_ok();
                let _ = CloseHandle(h);
                println!("{pid} trimmed: {ok}");
            }
            Err(e) => println!("{pid} not opened: {e}"),
        }
    }
}

/// A started app must not inherit this output (a caller reading it would wait for the app).
fn no_inherit() {
    unsafe {
        use windows::Win32::Foundation::{SetHandleInformation, HANDLE_FLAGS, HANDLE_FLAG_INHERIT};
        use windows::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
        for which in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            if let Ok(h) = GetStdHandle(which) {
                let _ = SetHandleInformation(h, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0));
            }
        }
    }
}

fn display(what: Option<&str>) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetLastInputInfo, SendInput, INPUT, INPUT_0, INPUT_MOUSE, LASTINPUTINFO, MOUSEEVENTF_MOVE, MOUSEINPUT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, HWND_BROADCAST, SC_MONITORPOWER, WM_SYSCOMMAND};
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    match what {
        Some("off") => unsafe {
            let _ = PostMessageW(Some(HWND_BROADCAST), WM_SYSCOMMAND, WPARAM(SC_MONITORPOWER as usize), LPARAM(2));
        },
        Some("on") => {
            let nudge = |dx| INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy: 0, dwFlags: MOUSEEVENTF_MOVE, ..Default::default() } },
            };
            unsafe { SendInput(&[nudge(1), nudge(-1)], size_of::<INPUT>() as i32) };
        }
        _ => {
            let mut li = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
            unsafe {
                let _ = GetLastInputInfo(&mut li);
                let now = windows::Win32::System::SystemInformation::GetTickCount();
                println!("{:.1}", now.wrapping_sub(li.dwTime) as f64 / 1000.0);
            }
        }
    }
}
