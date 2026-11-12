//! Ending processes: asking their windows to close, or ending them at once. The real [`Control`].
//!
//! Nothing here elevates. A process this window cannot open is reported, not retried another way
//! (see `policy` for where an elevated route would go).

use super::{info, Control, Details, Key};
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

    fn terminate(&self, key: Key) -> Result<(), String> {
        let Some(h) = info::open(key, PROCESS_TERMINATE) else {
            return Err("Windows did not let this window end it, or it has already ended.".into());
        };
        unsafe { TerminateProcess(h.0, 1) }.map_err(|e| format!("It could not be ended: {}", e.message()))
    }

    fn details(&self, key: Key) -> Details {
        info::details(key)
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
        assert!(Native.terminate(wrong).is_err(), "a key with another start time is refused");
        Native.terminate(Key { pid, created: raw.created }).unwrap();
        let status = child.wait().unwrap();
        assert_eq!(status.code(), Some(1));
    }
}
