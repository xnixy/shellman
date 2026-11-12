//! Starts the apps in order and keeps them running. One thread, no polling: it sleeps in
//! `MsgWaitForMultipleObjectsEx` on every app's process handle plus the window's messages, with
//! the nearest restart time as the timeout.

use crate::apps::App;
use crate::policy::{Crashes, Next};
use crate::proc::Proc;
use crate::{find, log, shutdown};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MsgWaitForMultipleObjectsEx, PeekMessageW, TranslateMessage, MSG, MWMO_INPUTAVAILABLE,
    PM_REMOVE, QS_ALLINPUT, WM_QUIT,
};

/// Between one app's start and the next, so the desktop is up before the bar and so on.
const GAP: Duration = Duration::from_millis(250);

struct Slot {
    app: App,
    proc: Option<Proc>,
    crashes: Crashes,
    /// When to (re)start it; `None` while it runs or after it was given up on.
    due: Option<Instant>,
    started: Instant,
}

pub struct Fleet {
    slots: Vec<Slot>,
    t0: Instant,
    adopted: usize,
}

impl Fleet {
    /// Adopt apps that already run (a session that crashed left them), schedule the rest in order.
    pub fn new(apps: Vec<App>) -> Fleet {
        let t0 = Instant::now();
        let mut fleet = Fleet { slots: Vec::new(), t0, adopted: 0 };
        for (i, app) in apps.into_iter().enumerate() {
            let mut slot =
                Slot { app, proc: None, crashes: Crashes::default(), due: None, started: t0 };
            match fleet.adopt(&slot.app) {
                Some(p) => {
                    log::line(&format!("{}: already running as pid {}, adopted", slot.app.name, p.pid));
                    slot.proc = Some(p);
                    fleet.adopted += 1;
                }
                None => slot.due = Some(t0 + GAP * i as u32),
            }
            fleet.slots.push(slot);
        }
        fleet.publish();
        fleet
    }

    /// A running copy of this app's exe that no other slot holds.
    fn adopt(&self, app: &App) -> Option<Proc> {
        let held: Vec<u32> = self.slots.iter().filter_map(|s| s.proc.as_ref().map(|p| p.pid)).collect();
        find::by_exe(&app.exe)
            .into_iter()
            .filter(|pid| !held.contains(pid))
            .find_map(Proc::open)
            .filter(|p| p.exit_code().is_none())
    }

    fn publish(&self) {
        shutdown::publish(self.slots.iter().filter_map(|s| s.proc.as_ref().map(|p| p.pid)).collect());
    }

    /// How many apps were adopted rather than started (and so: the session was restarted).
    pub fn adopted(&self) -> usize {
        self.adopted
    }

    pub fn bar_alive(&self) -> bool {
        self.slots.iter().any(|s| s.app.is_bar() && s.proc.is_some())
    }

    /// The bar is gone for good: missing, or given up on, or ended on purpose.
    pub fn bar_lost(&self) -> bool {
        self.slots.iter().any(|s| s.app.is_bar() && s.proc.is_none() && s.due.is_none())
    }

    fn start(&mut self, i: usize) {
        let now = self.t0.elapsed();
        let slot = &mut self.slots[i];
        slot.due = None;
        if !slot.app.exe.exists() {
            log::line(&format!("{}: {} not found, not started", slot.app.name, slot.app.exe.display()));
            return;
        }
        match Proc::spawn(&slot.app) {
            Ok(p) => {
                log::line(&format!("{}: started, pid {}", slot.app.name, p.pid));
                slot.started = Instant::now();
                slot.proc = Some(p);
            }
            Err(e) => {
                log::line(&format!("{}: could not start: {e}", slot.app.name));
                let next = slot.crashes.on_exit(1, now);
                Self::apply(slot, next);
            }
        }
    }

    fn apply(slot: &mut Slot, next: Next) {
        match next {
            Next::Restart(after) => {
                log::line(&format!("{}: restarting in {} ms", slot.app.name, after.as_millis()));
                slot.due = Some(Instant::now() + after);
            }
            Next::GiveUp => log::line(&format!("{}: crashing again and again, given up", slot.app.name)),
            Next::Deliberate => {}
        }
    }

    fn exited(&mut self, i: usize, code: u32) {
        let now = self.t0.elapsed();
        let ran = self.slots[i].started.elapsed();
        self.slots[i].proc = None;
        let name = self.slots[i].app.name.clone();
        if shutdown::quiet() {
            log::line(&format!("{name}: exited with {code:#x} while ending, not restarted"));
            return;
        }
        let next = self.slots[i].crashes.on_exit(code, now);
        log::line(&format!("{name}: exited with {code:#x} after {} ms", ran.as_millis()));
        if next == Next::Deliberate {
            // A resident started with `--background` while its copy runs exits 0 at once: hold
            // that copy instead, so it is still watched.
            if let Some(p) = self.adopt(&self.slots[i].app) {
                log::line(&format!("{name}: another copy runs as pid {}, adopted", p.pid));
                self.slots[i].proc = Some(p);
                self.slots[i].started = Instant::now();
            } else {
                log::line(&format!("{name}: ended on purpose, left stopped"));
            }
            return;
        }
        Self::apply(&mut self.slots[i], next);
    }

    /// One pass over what happened: exits first, then anything due to start.
    pub fn step(&mut self) {
        for i in 0..self.slots.len() {
            if let Some(code) = self.slots[i].proc.as_ref().and_then(Proc::exit_code) {
                self.exited(i, code);
            }
        }
        if !shutdown::quiet() {
            let now = Instant::now();
            for i in 0..self.slots.len() {
                if self.slots[i].due.is_some_and(|d| d <= now) {
                    self.start(i);
                }
            }
        }
        self.publish();
    }

    /// Handles to wait on, and the time until the nearest start (None: no limit).
    fn waits(&self) -> (Vec<HANDLE>, Option<Duration>) {
        let handles = self.slots.iter().filter_map(|s| s.proc.as_ref().map(|p| p.handle)).take(63).collect();
        let now = Instant::now();
        let due = self.slots.iter().filter_map(|s| s.due).min().map(|d| d.saturating_duration_since(now));
        (handles, due)
    }

    /// Sleep until something happens. False when the window asked to quit.
    pub fn wait(&self, extra: Option<Duration>) -> bool {
        let (handles, due) = self.waits();
        let limit = [due, extra].into_iter().flatten().min();
        let ms = limit.map_or(u32::MAX, |d| d.as_millis().min(u32::MAX as u128 - 1) as u32 + 1);
        unsafe {
            MsgWaitForMultipleObjectsEx(Some(&handles), ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    return false;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        true
    }
}
