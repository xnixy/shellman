//! What file work a shellman window has under way, told to the top bar.
//!
//! A copy, move or delete goes through `SHFileOperationW` (see `shell`), so its progress shows in
//! Windows' own dialog (`OperationStatusWindow`, a top-level window of the same process), which
//! reports it to the taskbar as any app does. That says how far, not what: the dialog's title is
//! only "64% complete". So while an operation runs, the window that started it carries a property,
//! [`PROP`], counting the operations under way by kind; the bar reads it from the windows of the
//! dialog's process and says "Copying" or "Moving" (see the bar's `topbar::now`).
//!
//! The property holds a number, not a handle, so another process can read it with `GetPropW`.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::UI::WindowsAndMessaging::{GetPropW, RemovePropW, SetPropW};

/// The property's name.
pub const PROP: windows::core::PCWSTR = w!("ShellmanFileWork");

/// What kind of file work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Work {
    Copy,
    Move,
    Delete,
}

/// How many operations of each kind are under way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub copy: u8,
    pub mv: u8,
    pub delete: u8,
}

impl Counts {
    fn slot(&mut self, w: Work) -> &mut u8 {
        match w {
            Work::Copy => &mut self.copy,
            Work::Move => &mut self.mv,
            Work::Delete => &mut self.delete,
        }
    }

    /// Packed into the property's value: a byte per kind, plus one so a value is never 0
    /// (`GetPropW`'s "none").
    pub fn pack(self) -> usize {
        1 + ((self.copy as usize) << 8 | (self.mv as usize) << 16 | (self.delete as usize) << 24)
    }

    pub fn unpack(v: usize) -> Self {
        if v == 0 {
            return Self::default();
        }
        let v = v - 1;
        Self { copy: (v >> 8) as u8, mv: (v >> 16) as u8, delete: (v >> 24) as u8 }
    }

    pub fn is_empty(self) -> bool {
        self == Self::default()
    }

    /// The sum of two windows' counts (one process, several windows).
    pub fn plus(self, o: Self) -> Self {
        Self {
            copy: self.copy.saturating_add(o.copy),
            mv: self.mv.saturating_add(o.mv),
            delete: self.delete.saturating_add(o.delete),
        }
    }

    /// What the work is called, when it is all of one kind: "Copying", "Moving", "Deleting".
    /// Mixed work, or none: None (the bar names the app instead).
    pub fn verb(self) -> Option<&'static str> {
        match (self.copy > 0, self.mv > 0, self.delete > 0) {
            (true, false, false) => Some("Copying"),
            (false, true, false) => Some("Moving"),
            (false, false, true) => Some("Deleting"),
            _ => None,
        }
    }
}

/// This process's counts, by window.
static UNDER_WAY: LazyLock<Mutex<HashMap<isize, Counts>>> = LazyLock::new(Mutex::default);

fn publish(hwnd: isize, c: Counts) {
    let h = HWND(hwnd as *mut _);
    // SAFETY: plain property calls on a window handle; a stale handle only makes them fail.
    unsafe {
        if c.is_empty() {
            let _ = RemovePropW(h, PROP);
        } else {
            let _ = SetPropW(h, PROP, Some(HANDLE(c.pack() as *mut _)));
        }
    }
}

/// Marks `hwnd` busy with `work` until dropped.
pub struct Busy {
    hwnd: isize,
    work: Work,
}

/// Mark `hwnd` as running `work` while the guard lives. No window (0): nothing is marked.
pub fn start(hwnd: isize, work: Work) -> Option<Busy> {
    if hwnd == 0 {
        return None;
    }
    let mut map = UNDER_WAY.lock().unwrap();
    let c = map.entry(hwnd).or_default();
    *c.slot(work) = c.slot(work).saturating_add(1);
    publish(hwnd, *c);
    Some(Busy { hwnd, work })
}

impl Drop for Busy {
    fn drop(&mut self) {
        let mut map = UNDER_WAY.lock().unwrap();
        let c = map.entry(self.hwnd).or_default();
        *c.slot(self.work) = c.slot(self.work).saturating_sub(1);
        let now = *c;
        if now.is_empty() {
            map.remove(&self.hwnd);
        }
        publish(self.hwnd, now);
    }
}

/// What a window (of any process) says it has under way.
pub fn read(hwnd: isize) -> Counts {
    // SAFETY: reading a property; a stale handle reads as none.
    let v = unsafe { GetPropW(HWND(hwnd as *mut _), PROP) };
    Counts::unpack(v.0 as usize)
}

/// What all the top-level windows of process `pid` have under way together (a process may start
/// work from any of its windows, and one may be minimized, hidden or on another desktop).
pub fn read_process(pid: u32) -> Counts {
    use windows::core::BOOL;
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId};
    struct Acc {
        pid: u32,
        sum: Counts,
    }
    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
        let acc = &mut *(l.0 as *mut Acc);
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        if pid == acc.pid {
            acc.sum = acc.sum.plus(read(h.0 as isize));
        }
        BOOL(1)
    }
    let mut acc = Acc { pid, sum: Counts::default() };
    // SAFETY: the callback only runs during the call, with a pointer to `acc`.
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut acc as *mut Acc as isize));
    }
    acc.sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_round_trip_and_never_pack_to_zero() {
        let c = Counts { copy: 2, mv: 0, delete: 1 };
        assert_eq!(Counts::unpack(c.pack()), c);
        assert_ne!(Counts::default().pack(), 0);
        assert_eq!(Counts::unpack(0), Counts::default());
    }

    #[test]
    fn one_kind_has_a_verb_mixed_work_has_none() {
        assert_eq!(Counts { copy: 3, ..Default::default() }.verb(), Some("Copying"));
        assert_eq!(Counts { mv: 1, ..Default::default() }.verb(), Some("Moving"));
        assert_eq!(Counts { delete: 1, ..Default::default() }.verb(), Some("Deleting"));
        assert_eq!(Counts { copy: 1, mv: 1, delete: 0 }.verb(), None);
        assert_eq!(Counts::default().verb(), None);
        let a = Counts { copy: 1, ..Default::default() };
        assert_eq!(a.plus(Counts { mv: 1, ..Default::default() }).verb(), None);
    }
    /// As Files does it: the window lives on one thread, the operation runs on a worker.
    #[test]
    fn a_worker_marks_its_window_and_the_mark_goes_with_it() {
        use windows::core::w;
        use windows::Win32::System::Threading::GetCurrentProcessId;
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP};
        // SAFETY: a plain hidden STATIC window, destroyed at the end on this thread.
        let hwnd = unsafe {
            CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!("shellman busy test"), WS_POPUP, 0, 0, 10, 10, None, None, None, None)
        }
        .expect("test window");
        let h = hwnd.0 as isize;
        let pid = unsafe { GetCurrentProcessId() };
        let before = read_process(pid);
        let (started, done) = (std::sync::mpsc::channel(), std::sync::mpsc::channel::<()>());
        let worker = std::thread::spawn(move || {
            let _busy = start(h, Work::Copy);
            started.0.send(()).unwrap();
            done.1.recv().unwrap();
        });
        started.1.recv().unwrap();
        assert_eq!(read(h), Counts { copy: 1, ..Default::default() });
        assert_eq!(read_process(pid).copy, before.copy + 1);
        assert_eq!(read_process(pid).verb(), Some("Copying"));
        done.0.send(()).unwrap();
        worker.join().unwrap();
        assert_eq!(read(h), Counts::default(), "the mark goes when the work ends");
        assert!(start(0, Work::Move).is_none(), "no window, no mark");
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}
