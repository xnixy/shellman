//! Running the picker for a dialog's `Show`: write the request, start `shellman-files --pick`,
//! and keep the app's window alive (its messages pumped, the window disabled as under a modal
//! dialog) until the picker answers.

use shellman_pick::{exit, Request, Response, Verdict};
use std::os::windows::io::AsRawHandle;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use windows::Win32::Foundation::{HANDLE, HMODULE, HWND, WAIT_OBJECT_0};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, DispatchMessageW, IsWindow, MsgWaitForMultipleObjectsEx,
    PeekMessageW, PostQuitMessage, SetForegroundWindow, TranslateMessage, MSG,
    MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, WM_QUIT,
};

/// Names the picker program instead of the copy next to this DLL (for tests).
pub const EXE_VAR: &str = "SHELLMAN_PICKER_EXE";
/// The picker's file name, next to this DLL.
const EXE: &str = "shellman-files.exe";
/// How long one wait for the picker lasts before the app's messages are looked at again.
const TICK_MS: u32 = 50;

/// How a picker run ended.
#[derive(Debug)]
pub enum Ran {
    Chosen(Response),
    Cancelled,
    /// The picker could not be started, or failed without an answer: show the real dialog.
    Failed(String),
}

/// What the run needs from the dialog while it waits.
pub struct Hooks<'a> {
    /// Check a choice (the app's `OnFileOk`); true to accept it. None: accept everything, and
    /// the picker does not wait for a verdict.
    pub check: Option<&'a mut dyn FnMut(&Response) -> bool>,
    /// True once the app has asked the dialog to close (`IFileDialog::Close`).
    pub closed: &'a dyn Fn() -> bool,
}

/// Run the picker for `req`, over `owner`.
pub fn run(mut req: Request, owner: HWND, hooks: Hooks) -> Ran {
    let Some(exe) = picker_exe() else {
        return Ran::Failed(format!("{EXE} not found next to the hook"));
    };
    static COUNT: AtomicU64 = AtomicU64::new(1);
    let path = shellman_pick::new_request_path(
        &std::env::temp_dir(),
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed),
    );
    req.owner = owner.0 as i64;
    req.host_pid = std::process::id();
    req.confirm = hooks.check.is_some();
    let response_path = shellman_pick::response_path(&path);
    let verdict_path = shellman_pick::verdict_path(&path);
    let _ = std::fs::remove_file(&response_path);
    if let Err(e) = shellman_pick::write_json(&path, &req) {
        return Ran::Failed(format!("couldn't write the request: {e}"));
    }
    let child = Command::new(&exe)
        .arg("--pick")
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let ran = match child {
        Ok(child) => wait(child, owner, hooks, &response_path, &verdict_path),
        Err(e) => Ran::Failed(format!("couldn't start {}: {e}", exe.display())),
    };
    for p in [&path, &response_path, &verdict_path] {
        let _ = std::fs::remove_file(p);
    }
    ran
}

fn wait(
    mut child: Child,
    owner: HWND,
    mut hooks: Hooks,
    response_path: &std::path::Path,
    verdict_path: &std::path::Path,
) -> Ran {
    unsafe {
        // The picker may take the foreground (the app has it now and may hand it on).
        let _ = AllowSetForegroundWindow(child.id());
    }
    let modal = Modal::begin(owner);
    let handle = HANDLE(child.as_raw_handle());
    let mut accepted: Option<Response> = None;
    let mut quit: Option<i32> = None;
    loop {
        let woke =
            unsafe { MsgWaitForMultipleObjectsEx(Some(&[handle]), TICK_MS, QS_ALLINPUT, MWMO_INPUTAVAILABLE) };
        if woke == WAIT_OBJECT_0 {
            break;
        }
        if let Some(code) = pump() {
            quit = Some(code);
            break;
        }
        if (hooks.closed)() {
            break;
        }
        if accepted.is_some() {
            continue;
        }
        let Some(check) = hooks.check.as_mut() else { continue };
        if let Some(resp) = shellman_pick::read_response(response_path).filter(|r| r.ok) {
            let _ = std::fs::remove_file(response_path);
            let ok = check(&resp);
            // A message box the app showed meanwhile enables its window again when it closes.
            modal.disable_again();
            let verdict = if ok { Verdict::Ok } else { Verdict::Veto };
            if shellman_pick::write_verdict(verdict_path, verdict).is_err() {
                break;
            }
            if ok {
                accepted = Some(resp);
            }
        }
    }
    // Still running: the app quit or closed the dialog. This is our own child; stop it.
    let finished = matches!(child.try_wait(), Ok(Some(_)));
    if !finished {
        let _ = child.kill();
    }
    let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(exit::FAILED);
    modal.end();
    if let Some(code) = quit {
        // Hand the quit on to the app's own loop, as a modal dialog does.
        unsafe { PostQuitMessage(code) };
        return Ran::Cancelled;
    }
    if !finished {
        return Ran::Cancelled;
    }
    if let Some(resp) = accepted {
        return Ran::Chosen(resp);
    }
    match code {
        exit::OK => match shellman_pick::read_response(response_path) {
            Some(r) if r.ok => Ran::Chosen(r),
            _ => Ran::Failed("the picker said OK but left no answer".into()),
        },
        exit::CANCELLED => Ran::Cancelled,
        other => Ran::Failed(format!("the picker failed (exit code {other})")),
    }
}

/// Handle every waiting message of this thread; the code of a `WM_QUIT`, if one came.
fn pump() -> Option<i32> {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                return Some(msg.wParam.0 as i32);
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    None
}

/// The app's window, disabled while the picker is up (so it behaves as under a modal dialog),
/// and enabled and brought back to the front at the end.
struct Modal {
    owner: Option<HWND>,
}

impl Modal {
    fn begin(owner: HWND) -> Self {
        unsafe {
            if owner.0.is_null() || !IsWindow(Some(owner)).as_bool() || !IsWindowEnabled(owner).as_bool() {
                return Self { owner: None };
            }
            let _ = EnableWindow(owner, false);
        }
        Self { owner: Some(owner) }
    }

    fn disable_again(&self) {
        if let Some(o) = self.owner {
            unsafe {
                let _ = EnableWindow(o, false);
            }
        }
    }

    fn end(&self) {
        if let Some(o) = self.owner {
            unsafe {
                let _ = EnableWindow(o, true);
                let _ = SetForegroundWindow(o);
            }
        }
    }
}

/// The picker program: `SHELLMAN_PICKER_EXE`, or `shellman-files.exe` next to this DLL.
fn picker_exe() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(EXE_VAR) {
        return Some(PathBuf::from(p));
    }
    let exe = own_dir()?.join(EXE);
    exe.is_file().then_some(exe)
}

/// The folder this DLL (or, linked into a program, that program) was loaded from.
fn own_dir() -> Option<PathBuf> {
    unsafe {
        let mut module = HMODULE::default();
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            windows::core::PCWSTR(own_dir as *const u16),
            &mut module,
        )
        .ok()?;
        let mut buf = [0u16; 1024];
        let n = GetModuleFileNameW(Some(module), &mut buf) as usize;
        if n == 0 || n >= buf.len() {
            return None;
        }
        PathBuf::from(String::from_utf16_lossy(&buf[..n]))
            .parent()
            .map(PathBuf::from)
    }
}
