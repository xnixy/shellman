//! The Display page's worker. One thread owns a hidden window (never shown, so it still gets
//! the broadcast WM_DISPLAYCHANGE and WM_DEVICECHANGE) and sleeps in GetMessage: commands, a
//! monitor change and the 15 s "keep these settings?" timer all arrive as messages, so nothing
//! polls and an idle page costs nothing. It reads the configuration, publishes a `Page`, and
//! does what the page asks.

use super::os_apply as apply;
use super::os_read::{self, Snapshot};
use super::page::{self, Change, Page, Topology};
use super::revert::{self, Do, Revert};
use crate::system::live::{self, Mailbox, Wake};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;

const WM_CMD: u32 = WM_APP;
const WM_CHANGED: u32 = WM_APP + 1;
const TIMER_REFRESH: usize = 1;
const TIMER_REVERT: usize = 2;
/// A plug or a mode change sends several messages in a burst; read once after they settle.
const SETTLE_MS: u32 = 300;

pub(crate) enum Cmd {
    Refresh,
    Apply { staged: Vec<Change>, primary: String },
    Topology(Topology),
    Keep,
    Revert,
    Scale(String, u32),
    Hdr(String, bool),
    Stop,
}

pub struct DisplayPage {
    tx: Sender<Cmd>,
    hwnd: Arc<AtomicIsize>,
    mailbox: Arc<Mailbox<Page>>,
}

impl DisplayPage {
    /// Start following; `wake` is called whenever there is something new. The first reading
    /// follows at once.
    pub fn start(wake: Wake) -> Self {
        let (tx, rx) = channel();
        let mailbox = Arc::new(Mailbox::default());
        let hwnd = Arc::new(AtomicIsize::new(0));
        let (out, cell) = (mailbox.clone(), hwnd.clone());
        live::spawn("display-page", move || run(rx, out, cell, wake));
        Self { tx, hwnd, mailbox }
    }

    pub fn take(&self) -> Option<Page> {
        self.mailbox.take()
    }

    fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
        let h = self.hwnd.load(Ordering::Acquire);
        if h != 0 {
            // SAFETY: posting to a window of the worker thread; a dead window just fails.
            unsafe {
                let _ = PostMessageW(Some(HWND(h as *mut _)), WM_CMD, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub fn refresh(&self) {
        self.send(Cmd::Refresh);
    }
    pub fn apply(&self, staged: Vec<Change>, primary: &str) {
        self.send(Cmd::Apply { staged, primary: primary.to_owned() });
    }
    pub fn set_topology(&self, t: Topology) {
        self.send(Cmd::Topology(t));
    }
    pub fn keep(&self) {
        self.send(Cmd::Keep);
    }
    pub fn revert(&self) {
        self.send(Cmd::Revert);
    }
    pub fn set_scale(&self, id: &str, percent: u32) {
        self.send(Cmd::Scale(id.to_owned(), percent));
    }
    pub fn set_hdr(&self, id: &str, on: bool) {
        self.send(Cmd::Hdr(id.to_owned(), on));
    }
}

impl Drop for DisplayPage {
    fn drop(&mut self) {
        self.send(Cmd::Stop);
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // Sent, not queued: turn them into queued messages the loop below sees.
        WM_DISPLAYCHANGE | WM_DEVICECHANGE => {
            // SAFETY: posting to our own window.
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CHANGED, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        // SAFETY: the default handling for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

fn make_window() -> Option<HWND> {
    // SAFETY: a window class and a window of this thread; never shown.
    unsafe {
        let class = w!("ShellmanDisplayWatch");
        let wc = WNDCLASSW { lpfnWndProc: Some(proc), lpszClassName: class, ..Default::default() };
        RegisterClassW(&wc);
        CreateWindowExW(WS_EX_TOOLWINDOW, class, w!(""), WS_POPUP, 0, 0, 0, 0, None, None, None, None).ok()
    }
}

struct Worker {
    hwnd: HWND,
    snap: Option<Snapshot>,
    monitors: Vec<page::Monitor>,
    topology: Option<Topology>,
    /// The configuration to go back to, while a change awaits an answer.
    before: Option<Snapshot>,
    revert: Revert,
    notice: Option<String>,
}

fn run(rx: Receiver<Cmd>, out: Arc<Mailbox<Page>>, cell: Arc<AtomicIsize>, wake: Wake) {
    live::init_mta();
    let Some(hwnd) = make_window() else { return };
    cell.store(hwnd.0 as isize, Ordering::Release);
    let mut w = Worker { hwnd, snap: None, monitors: Vec::new(), topology: None, before: None, revert: Revert::default(), notice: None };
    w.read();
    w.publish(&out, &wake);
    let mut msg = MSG::default();
    // SAFETY: the standard message loop of the thread that owns the window.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if msg.hwnd != hwnd {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
                continue;
            }
            let mut dirty = false;
            match msg.message {
                WM_CMD => {
                    for cmd in rx.try_iter().collect::<Vec<_>>() {
                        if matches!(cmd, Cmd::Stop) {
                            let _ = DestroyWindow(hwnd);
                            return;
                        }
                        w.handle(cmd);
                        dirty = true;
                    }
                }
                WM_CHANGED => {
                    SetTimer(Some(hwnd), TIMER_REFRESH, SETTLE_MS, None);
                }
                WM_TIMER if msg.wParam.0 == TIMER_REFRESH => {
                    let _ = KillTimer(Some(hwnd), TIMER_REFRESH);
                    dirty = true;
                }
                WM_TIMER if msg.wParam.0 == TIMER_REVERT => {
                    let action = w.revert.tick(Instant::now());
                    w.act(action);
                    dirty = true;
                }
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if dirty {
                w.read();
                w.publish(&out, &wake);
            }
        }
    }
}

impl Worker {
    fn read(&mut self) {
        if let Some((m, t, snap)) = os_read::monitors() {
            self.monitors = m;
            self.topology = Some(t);
            self.snap = Some(snap);
        }
    }

    fn publish(&self, out: &Mailbox<Page>, wake: &Wake) {
        out.put(Page {
            monitors: self.monitors.clone(),
            topology: self.topology,
            pending: self.revert.pending(),
            notice: self.notice.clone(),
        });
        wake();
    }

    fn handle(&mut self, cmd: Cmd) {
        self.notice = None;
        match cmd {
            Cmd::Refresh | Cmd::Stop => {}
            Cmd::Apply { staged, primary } => self.apply_plan(&staged, &primary),
            Cmd::Topology(t) => self.apply_topology(t),
            Cmd::Keep => {
                let action = self.revert.keep();
                self.act(action);
            }
            Cmd::Revert => {
                let action = self.revert.revert();
                self.act(action);
            }
            Cmd::Scale(id, percent) => {
                if let Some(snap) = &self.snap {
                    if let Err(e) = apply::set_scale(snap, &id, percent) {
                        self.notice = Some(e);
                    }
                }
            }
            Cmd::Hdr(id, on) => {
                if let Some(snap) = &self.snap {
                    if let Err(e) = apply::set_hdr(snap, &id, on) {
                        self.notice = Some(e);
                    }
                }
            }
        }
    }

    fn act(&mut self, action: Do) {
        // SAFETY: our own window's timer.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_REVERT);
        }
        match action {
            Do::Nothing => {}
            Do::Forget => self.before = None,
            Do::Restore => {
                if let Some(before) = self.before.take() {
                    if !apply::restore(&before) {
                        self.notice = Some("The previous settings could not be restored.".into());
                    } else {
                        self.notice = Some("Went back to the previous display settings.".into());
                    }
                }
            }
        }
        if self.revert.pending().is_some() {
            self.arm();
        }
    }

    fn arm(&self) {
        // SAFETY: our own window's timer.
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_REVERT, revert::WINDOW.as_millis() as u32, None);
        }
    }

    /// The configuration to go back to: the first one of a run of changes, so "keep" accepts
    /// what the user last saw and "revert" returns to it.
    fn remember(&mut self) {
        if self.before.is_none() {
            self.before = self.snap.clone();
        }
    }

    fn apply_plan(&mut self, staged: &[Change], primary: &str) {
        let Some(snap) = self.snap.clone() else { return };
        let plan = page::plan(&self.monitors, staged, primary);
        if !page::plan_valid(&plan) {
            self.notice = Some("The screens would overlap or be cut off from each other.".into());
            return;
        }
        if let Err(e) = apply::validate(&self.monitors, &plan, &snap) {
            self.notice = Some(e);
            return;
        }
        self.remember();
        if let Err(e) = apply::apply(&self.monitors, &plan, &snap) {
            self.notice = Some(e);
            self.before = None;
            return;
        }
        self.revert.applied(Instant::now());
        self.arm();
    }

    fn apply_topology(&mut self, t: Topology) {
        if let Err(e) = apply::set_topology(t, false) {
            self.notice = Some(e);
            return;
        }
        self.remember();
        if let Err(e) = apply::set_topology(t, true) {
            self.notice = Some(e);
            if let Some(b) = self.before.take() {
                apply::restore(&b);
            }
            return;
        }
        self.revert.applied(Instant::now());
        self.arm();
    }
}
