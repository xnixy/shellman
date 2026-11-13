//! Ending processes: asking their windows to close, or ending them at once. The real [`Control`].
//!
//! [`Native`] never elevates: a process it cannot open is reported as [`EndError::Denied`], and
//! the caller may then ask for [`end_elevated`], which goes through `shellman-elevate.exe` (a UAC
//! prompt). The helper ends processes with [`terminate_as_helper`].

use super::policy;
use super::{info, Control, Details, EndError, Key};
use crate::elevate::ops::Op;
use crate::elevate::{self, Outcome};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{TerminateProcess, PROCESS_TERMINATE};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};

/// Acts on processes through Windows.
#[derive(Default, Clone, Copy)]
pub struct Native;

impl Control for Native {
    fn close_windows(&self, windows: &[isize]) -> usize {
        windows
            .iter()
            .filter(|&&h| unsafe { PostMessageW(Some(HWND(h as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0)) }.is_ok())
            .count()
    }

    fn terminate(&self, key: Key) -> Result<(), EndError> {
        let h = info::open_checked(key, PROCESS_TERMINATE).map_err(EndError::from)?;
        unsafe { TerminateProcess(h.0, 1) }.map_err(|e| EndError::Failed(e.message()))
    }

    fn details(&self, key: Key) -> Details {
        info::details(key)
    }
}

impl From<info::OpenError> for EndError {
    fn from(e: info::OpenError) -> Self {
        match e {
            info::OpenError::Denied => EndError::Denied,
            info::OpenError::Gone => EndError::Gone,
            info::OpenError::Other(why) => EndError::Failed(why),
        }
    }
}

/// What the administrator helper does for each process of `end-processes`: end it, unless it is
/// a critical process of Windows or shellman's session. Checked here, again, so the helper never
/// trusts the caller's policy. A process already gone is not a failure.
pub fn terminate_as_helper(key: Key) -> Result<(), EndError> {
    let h = match info::open_checked(key, PROCESS_TERMINATE) {
        Ok(h) => h,
        Err(info::OpenError::Gone) => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    // A process whose name cannot be read is not ended: it cannot be told from a critical one.
    let Some(name) = info::image_path(&h).and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())) else {
        return Err(EndError::Failed("Its name could not be read.".into()));
    };
    if policy::is_untouchable(key.pid, &name) {
        return Err(EndError::Failed("Windows needs this process.".into()));
    }
    unsafe { TerminateProcess(h.0, 1) }.map_err(|e| EndError::Failed(e.message()))
}

/// End `keys` (in this order) as administrator, behind the UAC prompt. Blocks until the prompt is
/// answered and the helper is done: call it from a worker thread. Declining is
/// [`Outcome::Cancelled`].
pub fn end_elevated(keys: &[Key]) -> Outcome {
    match Op::end_processes(&keys.iter().map(|k| (k.pid, k.created)).collect::<Vec<_>>()) {
        Some(op) => elevate::run(&op),
        None => Outcome::Failed("Too many processes to end at once.".into()),
    }
}

/// The order to end a process tree in: the deepest children first, the root last, so nothing
/// left running gets a chance to start a replacement. `children` gives a key's direct children.
pub fn tree_order(root: Key, children: &dyn Fn(Key) -> Vec<Key>) -> Vec<Key> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    fn walk(k: Key, children: &dyn Fn(Key) -> Vec<Key>, seen: &mut std::collections::HashSet<Key>, out: &mut Vec<Key>) {
        if !seen.insert(k) {
            return;
        }
        for c in children(k) {
            walk(c, children, seen, out);
        }
        out.push(k);
    }
    walk(root, children, &mut seen, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(pid: u32) -> Key {
        Key { pid, created: pid as i64 }
    }

    #[test]
    fn children_go_first() {
        let children = |key: Key| match key.pid {
            1 => vec![k(2), k(3)],
            2 => vec![k(4)],
            _ => vec![],
        };
        assert_eq!(tree_order(k(1), &children), [k(4), k(2), k(3), k(1)]);
    }

    #[test]
    fn a_loop_ends() {
        let children = |key: Key| vec![k(if key.pid == 1 { 2 } else { 1 })];
        assert_eq!(tree_order(k(1), &children), [k(2), k(1)]);
    }

    /// A scratch process started here, ended here by its key: the only kind this test touches.
    #[test]
    fn ends_a_scratch_process_it_started() {
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/c", "ping", "-n", "4", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut r = super::super::nt::Reader::default();
        let raw = r.processes().into_iter().find(|p| p.pid == pid).unwrap();
        let wrong = Key { pid, created: raw.created + 1 };
        assert_eq!(Native.terminate(wrong), Err(EndError::Gone), "a key with another start time is refused");
        Native.terminate(Key { pid, created: raw.created }).unwrap();
        let status = child.wait().unwrap();
        assert_eq!(status.code(), Some(1));
    }

    /// What Windows refuses an unelevated program is told apart from a process that is gone:
    /// only the first goes to the administrator helper. Nothing is ended here: `open_checked`
    /// only opens (and closes) a handle, even when this runs elevated.
    #[test]
    fn refused_and_gone_are_told_apart() {
        use super::super::info::{open_checked, OpenError};
        use windows::Win32::System::Threading::PROCESS_TERMINATE;
        let mut r = super::super::nt::Reader::default();
        let all = r.processes();
        let lsass = all.iter().find(|p| p.name.eq_ignore_ascii_case("lsass.exe")).expect("lsass");
        let key = Key { pid: lsass.pid, created: lsass.created };
        assert!(matches!(open_checked(key, PROCESS_TERMINATE), Err(OpenError::Denied) | Ok(_)), "a protected system process is refused, not gone");
        let me = std::process::id();
        let own = all.iter().find(|p| p.pid == me).unwrap();
        assert!(matches!(open_checked(Key { pid: me, created: own.created + 1 }, PROCESS_TERMINATE), Err(OpenError::Gone)), "reused PID");
        assert!(matches!(open_checked(Key { pid: 0x0FFF_FFF0, created: 1 }, PROCESS_TERMINATE), Err(OpenError::Gone)), "no such PID");
        assert_eq!(Native.terminate(Key { pid: 0x0FFF_FFF0, created: 1 }), Err(EndError::Gone));
    }
}
