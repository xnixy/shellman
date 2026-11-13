//! The Windows side of packaged startup tasks: finding them for the current user, and starting
//! one the way the shell does.
//!
//! Listing reads each package's manifest (`packaged::parse_manifest`) and the user's `State`.
//!
//! Starting: a task cannot be started with the public API (`StartupTask` only works from inside
//! its own package), and starting the app's executable or activating its AUMID would start the
//! wrong program, or the right one without the startup-task activation its code checks for. The
//! shell (Explorer, and ShellAppRuntime: `shell\lib\runonce\runonce.cpp`, read from
//! ShellAppRuntime.exe 10.0.26100.9818) does it with two classes of `Windows.ApplicationModel.dll`
//! that are registered for any caller (in-process, partial trust) but have no metadata:
//!
//! - `Windows.ApplicationModel.Internal.StartupTaskInternal`: its statics interface
//!   {9A3392CE-BB67-4D5D-A130-59B6CA9EE5A3}, second method, returns
//!   `IAsyncOperation<IVectorView<StartupTaskInternal>>`, the current user's tasks;
//! - `Windows.ApplicationModel.Internal.StartupTaskActivator`: activated per task; its interface
//!   {104BA70F-4EED-4A8E-A5B5-CF4A5B86C218}, first method, takes a task and returns the new
//!   process id.
//!
//! Windows' list holds only the tasks that are on; an entry is matched to the task read from the
//! manifest by its program (see `matches`). Everything runs on the caller's thread, which must
//! have COM initialised (the session's startup thread does).

use super::packaged::{self, Task};
use crate::registry::Registry;
use std::ffi::c_void;
use std::time::{Duration, Instant};
use windows::core::{interface, IInspectable, IUnknown, IUnknown_Vtbl, Interface, HRESULT, HSTRING};
use windows::ApplicationModel::{PackageSignatureKind, StartupTask, StartupTaskState};
use windows_future::{AsyncStatus, IAsyncInfo};
use windows::Management::Deployment::PackageManager;
use windows::Win32::System::WinRT::{RoActivateInstance, RoGetActivationFactory};

/// Every startup task of the current user's packages, with its state. Needs COM.
pub fn list(reg: &dyn Registry) -> Vec<Task> {
    let Ok(manager) = PackageManager::new() else { return Vec::new() };
    let Ok(packages) = manager.FindPackagesByUserSecurityId(&HSTRING::new()) else { return Vec::new() };
    let mut out = Vec::new();
    for p in packages {
        let skip = p.IsFramework().unwrap_or(true)
            || p.IsResourcePackage().unwrap_or(true)
            || p.SignatureKind().is_ok_and(|k| k == PackageSignatureKind::System);
        if skip {
            continue;
        }
        let (Ok(id), Ok(dir)) = (p.Id(), p.InstalledPath()) else { continue };
        let Ok(family) = id.FamilyName() else { continue };
        let dir = dir.to_string();
        let Ok(manifest) = std::fs::read_to_string(std::path::Path::new(&dir).join("AppxManifest.xml")) else { continue };
        if !manifest.contains("windows.startupTask") {
            continue;
        }
        let name = p.DisplayName().map(|s| s.to_string()).unwrap_or_default();
        out.extend(packaged::tasks_of(reg, &family.to_string(), &name, &dir, &manifest));
    }
    out
}

// The two undocumented interfaces, declared as far as they are called. Slots before the one used
// are placeholders so it lands in the right place; they are never called. The macro takes only
// IUnknown as a base, so IInspectable's three methods are spelled out first.
#[allow(non_snake_case)]
mod internal {
    use super::*;

    #[interface("9A3392CE-BB67-4D5D-A130-59B6CA9EE5A3")]
    pub unsafe trait IStartupTaskInternalStatics: IUnknown {
        fn GetIids(&self) -> HRESULT;
        fn GetRuntimeClassName(&self) -> HRESULT;
        fn GetTrustLevel(&self) -> HRESULT;
        fn Unused0(&self) -> HRESULT;
        fn GetCurrentUserTasksAsync(&self, operation: *mut *mut c_void) -> HRESULT;
    }

    #[interface("104BA70F-4EED-4A8E-A5B5-CF4A5B86C218")]
    pub unsafe trait IStartupTaskActivator: IUnknown {
        fn GetIids(&self) -> HRESULT;
        fn GetRuntimeClassName(&self) -> HRESULT;
        fn GetTrustLevel(&self) -> HRESULT;
        fn Activate(&self, task: *mut c_void, pid: *mut u32) -> HRESULT;
    }

    /// `IAsyncOperation<IVectorView<StartupTaskInternal>>`, through its vtable only (its IID is
    /// parameterised; the object is never asked for it).
    #[interface("00000000-0000-0000-0000-000000000001")]
    pub unsafe trait IAsyncOperationRaw: IUnknown {
        fn GetIids(&self) -> HRESULT;
        fn GetRuntimeClassName(&self) -> HRESULT;
        fn GetTrustLevel(&self) -> HRESULT;
        fn put_Completed(&self, handler: *mut c_void) -> HRESULT;
        fn get_Completed(&self, handler: *mut *mut c_void) -> HRESULT;
        fn GetResults(&self, results: *mut *mut c_void) -> HRESULT;
    }

    /// `IVectorView<StartupTaskInternal>`, through its vtable only, as above.
    #[interface("00000000-0000-0000-0000-000000000002")]
    pub unsafe trait IVectorViewRaw: IUnknown {
        fn GetIids(&self) -> HRESULT;
        fn GetRuntimeClassName(&self) -> HRESULT;
        fn GetTrustLevel(&self) -> HRESULT;
        fn GetAt(&self, index: u32, item: *mut *mut c_void) -> HRESULT;
        fn get_Size(&self, size: *mut u32) -> HRESULT;
    }

    /// The task object's default interface (its IID as `GetIids` reports it on 26300): the shell
    /// reads its seventh method's string, the program the task starts, to log it.
    #[interface("22AA83C0-1615-4F47-AAEE-A63A926DA3AF")]
    pub unsafe trait IStartupTaskInternalRaw: IUnknown {
        fn GetIids(&self) -> HRESULT;
        fn GetRuntimeClassName(&self) -> HRESULT;
        fn GetTrustLevel(&self) -> HRESULT;
        fn Unused0(&self) -> HRESULT;
        fn Unused1(&self) -> HRESULT;
        fn Unused2(&self) -> HRESULT;
        fn Unused3(&self) -> HRESULT;
        fn Unused4(&self) -> HRESULT;
        fn Unused5(&self) -> HRESULT;
        fn get_Program(&self, path: *mut std::mem::MaybeUninit<HSTRING>) -> HRESULT;
    }

    pub unsafe fn program(t: &IStartupTaskInternalRaw) -> Option<String> {
        let mut out = std::mem::MaybeUninit::<HSTRING>::zeroed();
        t.get_Program(&mut out).ok().ok()?;
        Some(out.assume_init().to_string())
    }

    pub unsafe fn tasks_async(s: &IStartupTaskInternalStatics, out: *mut *mut c_void) -> HRESULT {
        s.GetCurrentUserTasksAsync(out)
    }
    pub unsafe fn activate(a: &IStartupTaskActivator, task: *mut c_void, pid: *mut u32) -> HRESULT {
        a.Activate(task, pid)
    }
    pub unsafe fn results(op: &IAsyncOperationRaw, out: *mut *mut c_void) -> HRESULT {
        op.GetResults(out)
    }
    pub unsafe fn size(v: &IVectorViewRaw, out: *mut u32) -> HRESULT {
        v.get_Size(out)
    }
    pub unsafe fn at(v: &IVectorViewRaw, i: u32, out: *mut *mut c_void) -> HRESULT {
        v.GetAt(i, out)
    }
}

use internal::*;

/// One task as Windows' own list has it.
pub struct Live {
    object: IInspectable,
    /// The program as the manifest names it, relative to the package (`1Password.exe`).
    pub program: String,
    /// Empty: the objects do not answer the public `IStartupTask` (seen on 26300).
    pub task_id: String,
    pub state: Option<StartupTaskState>,
}

fn wait(info: &IAsyncInfo, limit: Duration) -> Result<(), String> {
    let start = Instant::now();
    loop {
        match info.Status().map_err(|e| e.message())? {
            AsyncStatus::Started if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(20)),
            AsyncStatus::Started => return Err("timed out".into()),
            AsyncStatus::Completed => return Ok(()),
            _ => return Err(format!("failed: {}", info.ErrorCode().map(|c| c.message()).unwrap_or_default())),
        }
    }
}

/// The current user's startup tasks, from Windows' internal list. Needs COM.
pub fn live() -> Result<Vec<Live>, String> {
    unsafe {
        let class = HSTRING::from("Windows.ApplicationModel.Internal.StartupTaskInternal");
        let statics: IStartupTaskInternalStatics = RoGetActivationFactory(&class).map_err(|e| format!("statics: {e}"))?;
        let mut raw = std::ptr::null_mut();
        tasks_async(&statics, &mut raw).ok().map_err(|e| format!("list: {e}"))?;
        if raw.is_null() {
            return Err("list: no operation".into());
        }
        let op = IAsyncOperationRaw::from_raw(raw);
        wait(&op.cast::<IAsyncInfo>().map_err(|e| e.message())?, Duration::from_secs(10))?;
        let mut raw = std::ptr::null_mut();
        results(&op, &mut raw).ok().map_err(|e| format!("results: {e}"))?;
        if raw.is_null() {
            return Ok(Vec::new());
        }
        let view = IVectorViewRaw::from_raw(raw);
        let mut n = 0u32;
        size(&view, &mut n).ok().map_err(|e| format!("size: {e}"))?;
        let mut out = Vec::new();
        for i in 0..n {
            let mut raw = std::ptr::null_mut();
            if at(&view, i, &mut raw).is_err() || raw.is_null() {
                continue;
            }
            let object = IInspectable::from_raw(raw);
            // Asked for by IID, so a build whose task object differs gives no program (and so no
            // match) instead of a call into the wrong slot.
            let program = object.cast::<IStartupTaskInternalRaw>().ok().and_then(|t| program(&t)).unwrap_or_default();
            let public = object.cast::<StartupTask>().ok();
            let task_id = public.as_ref().and_then(|t| t.TaskId().ok()).map(|s| s.to_string()).unwrap_or_default();
            let state = public.as_ref().and_then(|t| t.State().ok());
            out.push(Live { object, program, task_id, state });
        }
        Ok(out)
    }
}

/// Whether an entry of Windows' list is `task`. Seen on 26300: the list holds only the tasks that
/// are on, its objects do not answer `IStartupTask`, and the one string the shell reads from them
/// is the manifest's `Executable`. So that, and the task id when there is one, are what match;
/// two packages with the same program name and both on are refused rather than guessed.
pub fn matches(live_program: &str, live_task_id: &str, executable: &str, task_id: &str) -> bool {
    live_program.eq_ignore_ascii_case(executable) && (live_task_id.is_empty() || live_task_id == task_id)
}

/// Start the task that runs `executable` (as the manifest names it) as the shell does. Returns
/// the new process id.
pub fn start(executable: &str, task_id: &str) -> Result<u32, String> {
    let tasks = live()?;
    let found: Vec<&Live> = tasks.iter().filter(|l| matches(&l.program, &l.task_id, executable, task_id)).collect();
    let live = match found.as_slice() {
        [one] => *one,
        [] => return Err("Windows does not list this task as on for this user".into()),
        _ => return Err("more than one enabled task starts a program of this name; not guessing".into()),
    };
    unsafe {
        let class = HSTRING::from("Windows.ApplicationModel.Internal.StartupTaskActivator");
        let object: IInspectable = RoActivateInstance(&class).map_err(|e| format!("activator: {e}"))?;
        let activator: IStartupTaskActivator = object.cast().map_err(|e| format!("activator: {e}"))?;
        let mut pid = 0u32;
        activate(&activator, live.object.as_raw(), &mut pid).ok().map_err(|e| e.message())?;
        Ok(pid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    #[test]
    fn a_task_is_matched_by_its_program_and_its_id_when_known() {
        assert!(matches("1Password.exe", "", "1Password.exe", "1PasswordStartup"));
        assert!(matches("1password.EXE", "", "1Password.exe", "x"), "case does not matter");
        assert!(!matches("Other.exe", "", "1Password.exe", "1PasswordStartup"));
        assert!(matches("a.exe", "T", "a.exe", "T"));
        assert!(!matches("a.exe", "T", "a.exe", "U"));
    }

    #[test]
    #[ignore = "reads this machine's packages; starts nothing"]
    fn lists_this_machines_tasks() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        for t in list(&crate::registry::Real) {
            println!("manifest: {} {} {} full_trust={} enabled={} ({}) {}", t.family, t.declared.task_id, t.aumid(), t.declared.full_trust, t.enabled, t.why, t.program());
        }
        match live() {
            Ok(tasks) => {
                for l in tasks {
                    println!("windows: {} id={:?} {:?}", l.program, l.task_id, l.state);
                }
            }
            Err(e) => println!("windows' list failed: {e}"),
        }
    }
}
