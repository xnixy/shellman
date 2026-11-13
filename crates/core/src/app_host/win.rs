//! The Win32 side of the app host: who owns the shell window, which processes run in this
//! session, and starting, watching and stopping them.

use std::path::PathBuf;
use std::time::Duration;
use windows::core::{w, HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HWND, LPARAM, MAX_PATH, WAIT_OBJECT_0, WPARAM};
use windows::Win32::Storage::FileSystem::{
    FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::SystemInformation::{GetSystemDirectoryW, GetSystemTimeAsFileTime};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, GetExitCodeProcess, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW,
    TerminateProcess, WaitForMultipleObjects, INFINITE, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetShellWindow, GetWindowThreadProcessId, PostThreadMessageW, RegisterWindowMessageW,
    SendNotifyMessageW, HWND_BROADCAST, WM_QUIT,
};

pub const HOST_EXE: &str = "shellappruntime.exe";

/// One process of this session.
#[derive(Clone, Debug)]
pub struct Proc {
    pub pid: u32,
    pub parent: u32,
    /// The exe's file name, lower case.
    pub name: String,
}

/// `%SystemRoot%\System32\ShellAppRuntime.exe`, from the system's own answer (never PATH), when
/// it is there.
pub fn host_path() -> Option<PathBuf> {
    let mut buf = [0u16; MAX_PATH as usize];
    let n = unsafe { GetSystemDirectoryW(Some(&mut buf)) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    let path = PathBuf::from(String::from_utf16_lossy(&buf[..n])).join("ShellAppRuntime.exe");
    path.is_file().then_some(path)
}

/// The process that owns the shell window, if there is one.
pub fn shell_window_pid() -> Option<u32> {
    unsafe {
        let hwnd = GetShellWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        (pid != 0).then_some(pid)
    }
}

/// Every process in this session (other sessions' Explorers are none of shellman's business).
pub fn session_processes() -> Vec<Proc> {
    let mine = session_of(unsafe { GetCurrentProcessId() });
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut entry).is_ok();
        while ok {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            let pid = entry.th32ProcessID;
            if mine.is_some() && session_of(pid) == mine {
                out.push(Proc {
                    pid,
                    parent: entry.th32ParentProcessID,
                    name: String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase(),
                });
            }
            ok = Process32NextW(snap, &mut entry).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

fn session_of(pid: u32) -> Option<u32> {
    let mut session = 0u32;
    unsafe { ProcessIdToSessionId(pid, &mut session) }.ok().map(|_| session)
}

/// A process's full exe path, lower case.
pub fn image(pid: u32) -> Option<String> {
    let process = Handle::open(pid, PROCESS_QUERY_LIMITED_INFORMATION.0)?;
    let mut buf = [0u16; MAX_PATH as usize];
    let mut len = buf.len() as u32;
    unsafe { QueryFullProcessImageNameW(process.0, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.ok()?;
    Some(String::from_utf16_lossy(&buf[..len as usize]).to_lowercase())
}

/// Now, in 100 ns units since 1601 (the same clock as process creation times).
pub fn now() -> u64 {
    ticks(unsafe { GetSystemTimeAsFileTime() })
}

/// When a process started, on the clock of `now`.
pub fn started(pid: u32) -> Option<u64> {
    let process = Handle::open(pid, PROCESS_QUERY_LIMITED_INFORMATION.0)?;
    let (mut created, mut exited, mut kernel, mut user) = Default::default();
    unsafe { GetProcessTimes(process.0, &mut created, &mut exited, &mut kernel, &mut user) }.ok()?;
    Some(ticks(created))
}

fn ticks(t: FILETIME) -> u64 {
    ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64
}

/// Start the host. It is not a child that matters: shellman ending leaves it running.
pub fn start_host(path: &std::path::Path) -> Option<(u32, Handle)> {
    let dir = path.parent()?;
    let child = std::process::Command::new(path).current_dir(dir).spawn().ok()?;
    // Opened while the child's own handle still holds the process, so the pid cannot be reused.
    Some((child.id(), Handle::watch(child.id())?))
}

/// Ask the process that owns the shell window to quit, the way it ends cleanly: WM_QUIT to the
/// shell window's thread. Ending it any other way makes Winlogon start Explorer as the shell.
/// False when `pid` does not own the shell window.
pub fn quit_shell(pid: u32) -> bool {
    unsafe {
        let hwnd = GetShellWindow();
        let mut owner = 0u32;
        let thread = GetWindowThreadProcessId(hwnd, Some(&mut owner));
        owner == pid && thread != 0 && PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0)).is_ok()
    }
}

/// End a process at once (only ever the Explorer that Winlogon started).
pub fn terminate(pid: u32) -> bool {
    let Some(process) = Handle::open(pid, PROCESS_TERMINATE.0 | PROCESS_SYNCHRONIZE.0) else { return false };
    let ok = unsafe { TerminateProcess(process.0, 1) }.is_ok();
    ok && process.wait(Duration::from_secs(5)) == Wait::Process
}

/// Tell apps the taskbar was created, so they add their tray icons again: an Explorer that ran
/// for a moment took them, and the bar's tray is the only one left.
pub fn announce_taskbar() {
    unsafe {
        if FindWindowW(w!("Shell_TrayWnd"), None).is_ok_and(|h| !h.is_invalid()) {
            let created = RegisterWindowMessageW(w!("TaskbarCreated"));
            let _ = SendNotifyMessageW(HWND_BROADCAST, created, WPARAM(0), LPARAM(0));
        }
    }
}

/// Ask every shellman glass panel to make its backdrop again (`shellman_ui::glass`'s
/// `REFRESH_MESSAGE`). DWM serves the host backdrop flat until the immersive shell is up, and a
/// backdrop made before then stays flat.
pub fn refresh_glass() {
    unsafe {
        let message = RegisterWindowMessageW(w!("shellman-glass-refresh"));
        let _ = SendNotifyMessageW(HWND_BROADCAST, message, WPARAM(0), LPARAM(0));
    }
}

/// Give the shell's task-window role back to the bar's tray window once the host has gone. The
/// host takes the role while it runs (the bar's own claim is then refused), and it is left empty
/// when the host ends; Windows only hides minimized windows (rather than drawing title-bar stubs
/// on the desktop) while someone holds it. Left alone while Explorer's tray is the one there.
/// Both calls are exported by user32 but not declared in the SDK, so they are looked up by name.
pub fn restore_taskman() {
    use windows::core::s;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    unsafe {
        let Ok(user32) = GetModuleHandleW(w!("user32.dll")) else { return };
        let (Some(get), Some(set)) = (GetProcAddress(user32, s!("GetTaskmanWindow")), GetProcAddress(user32, s!("SetTaskmanWindow"))) else {
            return;
        };
        let get: unsafe extern "system" fn() -> HWND = std::mem::transmute(get);
        let set: unsafe extern "system" fn(HWND) -> i32 = std::mem::transmute(set);
        if !get().is_invalid() || explorer_is_shell() {
            return;
        }
        if let Ok(tray) = FindWindowW(w!("Shell_TrayWnd"), None) {
            if !tray.is_invalid() {
                set(tray);
            }
        }
    }
}

/// Whether modern (UWP / CoreWindow) apps can open right now: something hosts the immersive
/// shell, whose window this is. Explorer and ShellAppRuntime both create it; without it,
/// activating such an app times out. One window lookup, cheap enough to ask at every showing.
pub fn immersive_shell() -> bool {
    unsafe { FindWindowW(w!("ApplicationManager_ImmersiveShellWindow"), None) }.is_ok_and(|h| !h.is_invalid())
}

/// Whether Explorer is the shell: it owns the tray window (`Shell_TrayWnd`) or the shell window.
pub fn explorer_is_shell() -> bool {
    let tray = unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }.ok().filter(|h| !h.is_invalid());
    let owner = |hwnd: HWND| {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        pid
    };
    let is_explorer = |pid: u32| pid != 0 && image(pid).is_some_and(|p| p.ends_with("\\explorer.exe"));
    tray.is_some_and(|t| is_explorer(owner(t))) || shell_window_pid().is_some_and(is_explorer)
}

/// A process handle, closed on drop.
pub struct Handle(HANDLE);

#[derive(PartialEq, Eq, Debug)]
pub enum Wait {
    Process,
    Settings,
    Timeout,
}

impl Handle {
    fn open(pid: u32, access: u32) -> Option<Self> {
        let access = windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS(access);
        unsafe { OpenProcess(access, false, pid) }.ok().map(Self)
    }

    /// Open a running process to wait on it.
    pub fn watch(pid: u32) -> Option<Self> {
        Self::open(pid, PROCESS_SYNCHRONIZE.0 | PROCESS_QUERY_LIMITED_INFORMATION.0)
    }

    pub fn wait(&self, timeout: Duration) -> Wait {
        wait_any(Some(self), None, Some(timeout))
    }

    pub fn exit_code(&self) -> Option<u32> {
        let mut code = 0u32;
        unsafe { GetExitCodeProcess(self.0, &mut code) }.ok().map(|_| code)
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Wakes when a file in the settings folder is written.
pub struct SettingsWatch(HANDLE);

impl SettingsWatch {
    pub fn new(dir: &std::path::Path) -> Option<Self> {
        let _ = std::fs::create_dir_all(dir);
        let filter = FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_FILE_NAME;
        unsafe { FindFirstChangeNotificationW(&HSTRING::from(dir.as_os_str()), false, filter) }.ok().map(Self)
    }

    /// Arm it again after it fired.
    pub fn rearm(&self) {
        unsafe {
            let _ = FindNextChangeNotification(self.0);
        }
    }
}

impl Drop for SettingsWatch {
    fn drop(&mut self) {
        unsafe {
            let _ = FindCloseChangeNotification(self.0);
        }
    }
}

/// Sleep until the process ends, the settings change, or the timeout (None: forever). No polling:
/// one kernel wait on both handles.
pub fn wait_any(process: Option<&Handle>, settings: Option<&SettingsWatch>, timeout: Option<Duration>) -> Wait {
    let mut handles = Vec::with_capacity(2);
    if let Some(p) = process {
        handles.push(p.0);
    }
    if let Some(s) = settings {
        handles.push(s.0);
    }
    let ms = timeout.map_or(INFINITE, |t| t.as_millis().min(u32::MAX as u128 - 1) as u32);
    if handles.is_empty() {
        std::thread::sleep(Duration::from_millis(ms as u64));
        return Wait::Timeout;
    }
    let got = unsafe { WaitForMultipleObjects(&handles, false, ms) };
    match got.0.wrapping_sub(WAIT_OBJECT_0.0) {
        0 if process.is_some() => Wait::Process,
        0 | 1 if settings.is_some() => Wait::Settings,
        _ => Wait::Timeout,
    }
}
