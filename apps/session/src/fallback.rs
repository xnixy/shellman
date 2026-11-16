//! The safety net's hand-over to Explorer: make Explorer the shell, with a taskbar, and check
//! that it is. The steps are in [`run`], against the [`Shell`] trait, so they are tested with
//! fakes; [`Real`] is Windows, [`StandIn`] the harmless test-run version.
//!
//! How Explorer decides (PLAN, "Modern apps without Explorer"): a new `explorer.exe` becomes the
//! shell (desktop, `Shell_TrayWnd` taskbar) only when nobody owns the shell window
//! (`GetShellWindow`); otherwise it only opens a folder window. The desktop process keeps
//! Windows' modern-app host (ShellAppRuntime) running, and the host's Progman owns the shell
//! window, so it has to go first. Its clean stop is WM_QUIT to the shell window's thread
//! (`app_host::quit_shell`; gone in 0.1 to 8 s, measured), after which Winlogon does not start
//! Explorer itself and the desktop's host supervisor leaves the host stopped. So "is Explorer the
//! shell" is asked of the windows (`app_host::explorer_is_shell`), never of the process list: an
//! Explorer folder window proves nothing.

use crate::{find, log, proc::MARK, shutdown};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, HWND, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, FindWindowW, GetWindowThreadProcessId};

/// Replaces explorer.exe, for tests only: `exe|arg|arg`, a harmless program standing in for it.
/// The stand-in "has a taskbar" while it runs; nothing real (the shell window, the host,
/// Explorer) is looked at or touched.
pub const ENV: &str = "SHELLMAN_SESSION_FALLBACK";

/// How long the modern-app host gets to quit after WM_QUIT (measured: 0.1 to 8 s).
pub const HOST_QUIT: Duration = Duration::from_secs(12);
/// How long Explorer gets, per attempt, to show its taskbar.
pub const EXPLORER_GRACE: Duration = Duration::from_secs(15);
/// Attempts per fallback (the second one asks the host again, in case it came back).
pub const ATTEMPTS: usize = 2;

pub fn shift_held() -> bool {
    // Reads the key state; it sends no input.
    unsafe { GetAsyncKeyState(VK_SHIFT.0 as i32) < 0 }
}

/// Any taskbar window (the bar's or Explorer's).
pub fn tray_up() -> bool {
    unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }.is_ok_and(|h| !h.is_invalid())
}

/// Who owns the shell window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owner {
    Nobody,
    Explorer(u32),
    /// ShellAppRuntime, the modern-app host the desktop keeps.
    Host(u32),
    Other(u32),
}

/// What a fallback came to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Explorer already was the shell: nothing was started.
    AlreadyShell,
    /// Explorer was started and its taskbar is up.
    Taskbar,
    /// No Explorer taskbar after every attempt.
    NoTaskbar,
    /// The session began to end: stopped half way.
    Ending,
}

/// The world as the fallback sees it.
pub trait Shell {
    /// Explorer owns the taskbar or the shell window.
    fn explorer_is_shell(&self) -> bool;
    fn owner(&self) -> Owner;
    /// Ask the host to quit cleanly and wait up to `within`; true when it is gone.
    fn quit_host(&self, pid: u32, within: Duration) -> bool;
    /// Start Explorer; its pid.
    fn start_explorer(&self) -> Result<u32, String>;
    /// Wait up to `within` for an Explorer-owned `Shell_TrayWnd`; true when there is one.
    fn explorer_taskbar(&self, within: Duration) -> bool;
    /// Any taskbar (the bar's or Explorer's): what the parked watchdog looks for.
    fn any_taskbar(&self) -> bool;
    /// Tell shellman's wallpaper to hide: Explorer's desktop and taskbar lie under it. How many
    /// desktop processes were told.
    fn wallpaper_aside(&self) -> usize;
    fn ending(&self) -> bool;
}

/// Explorer is the shell: shellman's wallpaper windows must not lie over its desktop and
/// taskbar (`core::desktop_aside`).
fn clear_the_desktop(sh: &dyn Shell, say: &dyn Fn(&str)) {
    match sh.wallpaper_aside() {
        0 => say("fallback: no shellman desktop runs, nothing to hide"),
        n => say(&format!("fallback: told {n} shellman desktop process(es) to hide the wallpaper windows")),
    }
}

/// Hand the session to Explorer. Blocks for up to ~`ATTEMPTS * (HOST_QUIT + EXPLORER_GRACE)`,
/// so it runs on a thread of its own.
pub fn run(sh: &dyn Shell, say: &dyn Fn(&str)) -> Outcome {
    for attempt in 1..=ATTEMPTS {
        if sh.ending() {
            say("fallback: the session is ending, stopped");
            return Outcome::Ending;
        }
        if sh.explorer_is_shell() {
            say("fallback: Explorer is already the shell, nothing to start");
            clear_the_desktop(sh, say);
            return if attempt == 1 { Outcome::AlreadyShell } else { Outcome::Taskbar };
        }
        match sh.owner() {
            Owner::Host(pid) => {
                let gone = sh.quit_host(pid, HOST_QUIT);
                if gone {
                    say(&format!("fallback: the modern-app host {pid} owned the shell window; asked it to quit, it is gone"));
                } else {
                    say(&format!("fallback: WARNING the modern-app host {pid} owns the shell window and did not quit in {} s; Explorer will probably only open a folder window", HOST_QUIT.as_secs()));
                }
            }
            Owner::Other(pid) => say(&format!("fallback: WARNING pid {pid} (neither Explorer nor the modern-app host) owns the shell window; starting Explorer anyway")),
            Owner::Explorer(pid) => say(&format!("fallback: Explorer {pid} owns the shell window but shows no taskbar yet")),
            Owner::Nobody => {}
        }
        if sh.ending() {
            say("fallback: the session is ending, stopped");
            return Outcome::Ending;
        }
        match sh.start_explorer() {
            Ok(pid) => say(&format!("fallback: started Explorer as pid {pid} (attempt {attempt} of {ATTEMPTS})")),
            Err(e) => say(&format!("fallback: WARNING could not start Explorer (attempt {attempt} of {ATTEMPTS}): {e}")),
        }
        if sh.explorer_taskbar(EXPLORER_GRACE) {
            say("fallback: Explorer's taskbar is up");
            clear_the_desktop(sh, say);
            return Outcome::Taskbar;
        }
        say(&format!("fallback: WARNING no Explorer taskbar {} s after attempt {attempt} of {ATTEMPTS}", EXPLORER_GRACE.as_secs()));
    }
    say("fallback: ERROR Explorer did not become the shell; no taskbar from Explorer");
    Outcome::NoTaskbar
}

/// The right `Shell` for this run: the real one, or in a test run the stand-in (which only logs
/// when `SHELLMAN_SESSION_FALLBACK` is not set).
pub fn for_run(test: bool) -> Arc<dyn Shell + Send + Sync> {
    if test {
        let spec = std::env::var(ENV).ok().filter(|v| !v.trim().is_empty());
        Arc::new(StandIn { app: spec.and_then(|s| crate::apps::parse(&s).into_iter().next()) })
    } else {
        Arc::new(Real)
    }
}

fn explorer_exe() -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows"));
    root.join("explorer.exe")
}

fn owner_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

fn image_ends(pid: u32, name: &str) -> bool {
    pid != 0 && find::image_of(pid).is_some_and(|i| i.to_lowercase().ends_with(name))
}

/// An `Shell_TrayWnd` owned by explorer.exe (the bar makes one too, so every one is looked at).
fn explorer_tray() -> bool {
    let mut after: Option<HWND> = None;
    for _ in 0..16 {
        let Ok(h) = (unsafe { FindWindowExW(None, after, w!("Shell_TrayWnd"), None) }) else { return false };
        if h.is_invalid() {
            return false;
        }
        if image_ends(owner_pid(h), "\\explorer.exe") {
            return true;
        }
        after = Some(h);
    }
    false
}

/// Wait for a process to end; true when it did (or is not there to open).
fn wait_gone(pid: u32, within: Duration) -> bool {
    let Ok(h) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }) else { return true };
    let ms = within.as_millis().min(u32::MAX as u128 - 1) as u32;
    let gone = unsafe { WaitForSingleObject(h, ms) } == WAIT_OBJECT_0;
    unsafe {
        let _ = CloseHandle(h);
    }
    gone
}

/// Windows.
pub struct Real;

impl Shell for Real {
    fn explorer_is_shell(&self) -> bool {
        shellman_core::app_host::explorer_is_shell()
    }
    fn owner(&self) -> Owner {
        match shellman_core::app_host::shell_window_pid() {
            None => Owner::Nobody,
            Some(pid) if image_ends(pid, "\\shellappruntime.exe") => Owner::Host(pid),
            Some(pid) if image_ends(pid, "\\explorer.exe") => Owner::Explorer(pid),
            Some(pid) => Owner::Other(pid),
        }
    }
    fn quit_host(&self, pid: u32, within: Duration) -> bool {
        // Only ever the clean stop: any other end of the host makes Winlogon start Explorer by
        // itself, and the desktop's host supervisor would then stop that Explorer again.
        if !shellman_core::app_host::quit_shell(pid) {
            return wait_gone(pid, Duration::ZERO);
        }
        wait_gone(pid, within)
    }
    fn start_explorer(&self) -> Result<u32, String> {
        // Without the mark, so Explorer and what it starts do not think a session owns them.
        std::process::Command::new(explorer_exe()).env_remove(MARK).spawn().map(|c| c.id()).map_err(|e| e.to_string())
    }
    fn explorer_taskbar(&self, within: Duration) -> bool {
        let until = Instant::now() + within;
        loop {
            if explorer_tray() {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    fn any_taskbar(&self) -> bool {
        tray_up()
    }
    fn wallpaper_aside(&self) -> usize {
        shellman_core::desktop_aside::tell(true)
    }
    fn ending(&self) -> bool {
        shutdown::quiet()
    }
}

/// A test run's Explorer: the program in `SHELLMAN_SESSION_FALLBACK` (or nothing, logged). It "is
/// the shell, with a taskbar" while it runs. Never looks at the real shell window or host.
pub struct StandIn {
    app: Option<crate::apps::App>,
}

impl StandIn {
    fn runs(&self) -> bool {
        self.app.as_ref().is_some_and(|a| !find::by_exe(&a.exe).is_empty())
    }
}

impl Shell for StandIn {
    fn explorer_is_shell(&self) -> bool {
        self.runs()
    }
    fn owner(&self) -> Owner {
        Owner::Nobody
    }
    fn quit_host(&self, _: u32, _: Duration) -> bool {
        true
    }
    fn start_explorer(&self) -> Result<u32, String> {
        let Some(app) = &self.app else { return Err("test run, would start explorer.exe here".into()) };
        let child = std::process::Command::new(&app.exe).args(&app.args).env_remove(MARK).spawn();
        child.map(|c| c.id()).map_err(|e| format!("{}: {e}", app.exe.display()))
    }
    fn explorer_taskbar(&self, within: Duration) -> bool {
        // Still running a moment after its start (a stand-in that exits plays a failed Explorer).
        std::thread::sleep(within.min(Duration::from_secs(1)));
        self.runs()
    }
    fn any_taskbar(&self) -> bool {
        self.runs()
    }
    fn wallpaper_aside(&self) -> usize {
        // A test run never touches the real desktop.
        0
    }
    fn ending(&self) -> bool {
        shutdown::quiet()
    }
}

/// Run the fallback on a thread of its own (it waits for seconds; the session's window must keep
/// answering, a logoff above all). `done` gets the outcome; then the window is woken.
pub fn spawn(shell: Arc<dyn Shell + Send + Sync>, window: isize, done: std::sync::mpsc::Sender<Outcome>) {
    let (shell2, done2) = (shell.clone(), done.clone());
    let started = std::thread::Builder::new().name("fallback".into()).spawn(move || {
        let outcome = run(&*shell, &|s| log::line(s));
        let _ = done.send(outcome);
        wake(window);
    });
    if let Err(e) = started {
        log::line(&format!("fallback: ERROR could not start its thread ({e}); running it here"));
        let _ = done2.send(run(&*shell2, &|s| log::line(s)));
    }
}

/// Wake the message loop (a harmless WM_NULL).
pub fn wake(window: isize) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_NULL};
    unsafe {
        let _ = PostMessageW(Some(HWND(window as *mut _)), WM_NULL, WPARAM(0), LPARAM(0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// A scripted world. `taskbar_after` is the start (1-based) after which Explorer shows its
    /// taskbar; 0: never.
    #[derive(Default)]
    struct Fake {
        explorer_shell: Cell<bool>,
        /// An Explorer folder window runs (it never makes Explorer the shell).
        folder_window: bool,
        owner: Cell<Option<Owner>>,
        host_quits: bool,
        /// The host owns the shell window again after it quit (once).
        host_comes_back: Cell<bool>,
        taskbar_after: usize,
        ending: bool,
        calls: RefCell<Vec<String>>,
        starts: Cell<usize>,
        /// How often the wallpaper was told to stand aside.
        asides: Cell<usize>,
    }

    impl Shell for Fake {
        fn explorer_is_shell(&self) -> bool {
            let _ = self.folder_window;
            self.explorer_shell.get()
        }
        fn owner(&self) -> Owner {
            self.owner.get().unwrap_or(Owner::Nobody)
        }
        fn quit_host(&self, pid: u32, _: Duration) -> bool {
            self.calls.borrow_mut().push(format!("quit {pid}"));
            if self.host_quits {
                self.owner.set(if self.host_comes_back.replace(false) { Some(Owner::Host(pid + 1)) } else { None });
            }
            self.host_quits
        }
        fn start_explorer(&self) -> Result<u32, String> {
            self.starts.set(self.starts.get() + 1);
            self.calls.borrow_mut().push("start".into());
            // Explorer takes the shell window only when nobody holds it.
            if self.owner() == Owner::Nobody && self.taskbar_after != 0 && self.starts.get() >= self.taskbar_after {
                self.owner.set(Some(Owner::Explorer(9)));
                self.explorer_shell.set(true);
            }
            Ok(9)
        }
        fn explorer_taskbar(&self, _: Duration) -> bool {
            self.calls.borrow_mut().push("look".into());
            self.explorer_shell.get()
        }
        fn any_taskbar(&self) -> bool {
            self.explorer_shell.get()
        }
        fn wallpaper_aside(&self) -> usize {
            self.asides.set(self.asides.get() + 1);
            1
        }
        fn ending(&self) -> bool {
            self.ending
        }
    }

    fn go(f: &Fake) -> Outcome {
        run(f, &|_| {})
    }

    #[test]
    fn a_folder_window_explorer_does_not_stop_the_fallback() {
        // The old check (any explorer.exe running) started nothing here and left no shell.
        let f = Fake { folder_window: true, taskbar_after: 1, ..Fake::default() };
        assert_eq!(go(&f), Outcome::Taskbar);
        assert_eq!(*f.calls.borrow(), ["start", "look"]);
    }

    #[test]
    fn the_host_is_asked_to_quit_before_explorer_starts() {
        let f = Fake { owner: Cell::new(Some(Owner::Host(40))), host_quits: true, taskbar_after: 1, ..Fake::default() };
        assert_eq!(go(&f), Outcome::Taskbar);
        assert_eq!(*f.calls.borrow(), ["quit 40", "start", "look"]);
        assert_eq!(f.asides.get(), 1, "Explorer's taskbar is up: the wallpaper windows hide");
    }

    #[test]
    fn a_host_that_comes_back_is_asked_again_on_the_retry() {
        let f = Fake {
            owner: Cell::new(Some(Owner::Host(40))),
            host_quits: true,
            host_comes_back: Cell::new(true),
            taskbar_after: 1,
            ..Fake::default()
        };
        assert_eq!(go(&f), Outcome::Taskbar);
        assert_eq!(*f.calls.borrow(), ["quit 40", "start", "look", "quit 41", "start", "look"]);
    }

    #[test]
    fn a_host_that_will_not_quit_ends_without_a_taskbar() {
        let f = Fake { owner: Cell::new(Some(Owner::Host(40))), host_quits: false, taskbar_after: 1, ..Fake::default() };
        assert_eq!(go(&f), Outcome::NoTaskbar);
        assert_eq!(f.starts.get(), ATTEMPTS, "retried once, then given up");
        assert_eq!(f.asides.get(), 0, "no Explorer desktop to make room for");
    }

    #[test]
    fn retries_once_when_the_first_start_shows_no_taskbar() {
        let f = Fake { taskbar_after: 2, ..Fake::default() };
        assert_eq!(go(&f), Outcome::Taskbar);
        assert_eq!(f.starts.get(), 2);
        let never = Fake::default();
        assert_eq!(go(&never), Outcome::NoTaskbar);
        assert_eq!(never.starts.get(), ATTEMPTS);
    }

    #[test]
    fn explorer_already_the_shell_is_left_alone() {
        let f = Fake { explorer_shell: Cell::new(true), owner: Cell::new(Some(Owner::Explorer(5))), ..Fake::default() };
        assert_eq!(go(&f), Outcome::AlreadyShell);
        assert!(f.calls.borrow().is_empty());
        assert_eq!(f.asides.get(), 1, "its desktop and taskbar are there: the wallpaper windows hide");
    }

    #[test]
    fn nothing_starts_while_the_session_ends() {
        let f = Fake { ending: true, taskbar_after: 1, ..Fake::default() };
        assert_eq!(go(&f), Outcome::Ending);
        assert_eq!(f.starts.get(), 0);
        assert_eq!(f.asides.get(), 0);
    }

    #[test]
    fn the_stand_in_without_a_program_starts_nothing() {
        let s = StandIn { app: None };
        assert!(s.start_explorer().is_err());
        assert!(!s.any_taskbar() && !s.explorer_is_shell());
        assert_eq!(s.owner(), Owner::Nobody, "a test run never looks at the real shell window");
    }
}
