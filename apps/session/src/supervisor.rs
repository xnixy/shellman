//! Starts the apps in order and keeps them running. One thread, no polling: it sleeps in
//! `MsgWaitForMultipleObjectsEx` on every app's process handle plus the window's messages, with
//! the nearest restart time as the timeout.
//!
//! Everything it does to the system goes through [`Os`], so the rules are tested with fakes.

use crate::apps::App;
use crate::policy::{Crashes, Next};
use crate::proc::Proc;
use crate::safety::Bar;
use crate::{find, log, shutdown};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MsgWaitForMultipleObjectsEx, PeekMessageW, TranslateMessage, MSG, MWMO_INPUTAVAILABLE,
    PM_REMOVE, QS_ALLINPUT, WM_QUIT,
};

/// Between one app's start and the next, so the desktop is up before the bar and so on.
const GAP: Duration = Duration::from_millis(250);

/// How long an app gets to close after a restart was requested (`restart.rs`) before it is ended.
const RESTART_GRACE: Duration = Duration::from_secs(6);

/// A process the fleet holds: started or adopted. Real ones are `Proc`; tests use fakes.
pub trait Child {
    fn pid(&self) -> u32;
    /// The exit code, or `None` while it still runs.
    fn exit_code(&self) -> Option<u32>;
    fn terminate(&self);
    /// What `wait` sleeps on (fakes have none).
    fn handle(&self) -> Option<HANDLE> {
        None
    }
}

impl Child for Proc {
    fn pid(&self) -> u32 {
        self.pid
    }
    fn exit_code(&self) -> Option<u32> {
        Proc::exit_code(self)
    }
    fn terminate(&self) {
        Proc::terminate(self)
    }
    fn handle(&self) -> Option<HANDLE> {
        Some(self.handle)
    }
}

/// Everything the fleet does to the system and to the rest of the session.
pub trait Os {
    fn exists(&self, app: &App) -> bool;
    fn spawn(&self, app: &App) -> Result<Box<dyn Child>, String>;
    /// A running copy of this app's exe that is not one of `held`.
    fn adopt(&self, app: &App, held: &[u32]) -> Option<Box<dyn Child>>;
    /// Ask a process's windows to close; how many were asked.
    fn close_windows(&self, pid: u32) -> usize;
    /// Queued "restart app X" requests (`restart.rs`).
    fn requests(&self) -> Vec<String>;
    /// The session is ending, or Windows asked whether it may end it: nothing starts.
    fn quiet(&self) -> bool;
    /// Which processes to close at logoff (`shutdown`).
    fn publish(&self, pids: Vec<u32>);
    fn publish_names(&self, names: Vec<String>);
    /// Whether the fleet is parked: the window refuses restart requests then.
    fn parked(&self, parked: bool);
}

/// The real system.
pub struct Real;

impl Os for Real {
    fn exists(&self, app: &App) -> bool {
        app.exe.exists()
    }
    fn spawn(&self, app: &App) -> Result<Box<dyn Child>, String> {
        Proc::spawn(app).map(|p| Box::new(p) as Box<dyn Child>)
    }
    fn adopt(&self, app: &App, held: &[u32]) -> Option<Box<dyn Child>> {
        find::by_exe(&app.exe)
            .into_iter()
            .filter(|pid| !held.contains(pid))
            .find_map(Proc::open)
            .filter(|p| p.exit_code().is_none())
            .map(|p| Box::new(p) as Box<dyn Child>)
    }
    fn close_windows(&self, pid: u32) -> usize {
        shutdown::close_windows(&[pid])
    }
    fn requests(&self) -> Vec<String> {
        crate::restart::take()
    }
    fn quiet(&self) -> bool {
        shutdown::quiet()
    }
    fn publish(&self, pids: Vec<u32>) {
        shutdown::publish(pids)
    }
    fn publish_names(&self, names: Vec<String>) {
        crate::restart::publish_names(names)
    }
    fn parked(&self, parked: bool) {
        crate::restart::set_parked(parked)
    }
}

struct Slot {
    app: App,
    proc: Option<Box<dyn Child>>,
    crashes: Crashes,
    /// When to (re)start it; `None` while it runs or after it was given up on.
    due: Option<Instant>,
    started: Instant,
    /// A restart was requested: the deadline by which it must have closed on its own.
    restart: Option<Instant>,
}

pub struct Fleet {
    slots: Vec<Slot>,
    t0: Instant,
    adopted: usize,
    parked: bool,
    os: Box<dyn Os>,
}

impl Fleet {
    /// Adopt apps that already run (a session that crashed left them), schedule the rest in order.
    pub fn new(apps: Vec<App>) -> Fleet {
        Fleet::with(apps, Box::new(Real))
    }

    pub fn with(apps: Vec<App>, os: Box<dyn Os>) -> Fleet {
        let t0 = Instant::now();
        let mut fleet = Fleet { slots: Vec::new(), t0, adopted: 0, parked: false, os };
        for (i, app) in apps.into_iter().enumerate() {
            let mut slot =
                Slot { app, proc: None, crashes: Crashes::default(), due: None, started: t0, restart: None };
            match fleet.adopt(&slot.app) {
                Some(p) => {
                    log::line(&format!("{}: already running as pid {}, adopted", slot.app.name, p.pid()));
                    slot.proc = Some(p);
                    fleet.adopted += 1;
                }
                None => slot.due = Some(t0 + GAP * i as u32),
            }
            fleet.slots.push(slot);
        }
        fleet.os.publish_names(fleet.slots.iter().map(|s| s.app.name.clone()).collect());
        fleet.publish();
        fleet
    }

    /// Handle a queued "restart app X" (`restart.rs`): ask it to close; its exit is then a
    /// restart, not a crash. An app that is not running is simply started now.
    fn request_restart(&mut self, name: &str) {
        let now = Instant::now();
        let Some(slot) = self.slots.iter_mut().find(|s| s.app.name == name) else { return };
        match (&slot.proc, slot.restart) {
            (_, Some(_)) => log::line(&format!("{name}: restart already under way")),
            (Some(p), None) => {
                let asked = self.os.close_windows(p.pid());
                log::line(&format!("{name}: restart requested, pid {} asked to close ({asked} windows)", p.pid()));
                slot.restart = Some(now + RESTART_GRACE);
            }
            (None, None) => {
                log::line(&format!("{name}: restart requested, not running: starting it"));
                slot.due = Some(now);
            }
        }
    }

    /// A running copy of this app's exe that no other slot holds.
    fn adopt(&self, app: &App) -> Option<Box<dyn Child>> {
        let held: Vec<u32> = self.slots.iter().filter_map(|s| s.proc.as_ref().map(|p| p.pid())).collect();
        self.os.adopt(app, &held)
    }

    /// Stop supervising and leave everything as it is: Explorer took over (see `safety`).
    ///
    /// Only the fleet is parked. The session is not ending, so this never touches `shutdown`'s
    /// stopping state: the startup items (which sleep on its stop event) carry on. Stopping them
    /// here cost a logon that fell back early all of its startup apps, and the Explorer started
    /// as the fallback skipped them too, because the session had already claimed the run.
    pub fn park(&mut self) {
        self.parked = true;
        for s in &mut self.slots {
            s.due = None;
        }
        self.os.parked(true);
    }

    #[cfg(test)]
    pub fn parked(&self) -> bool {
        self.parked
    }

    pub fn since_start(&self) -> Duration {
        self.t0.elapsed()
    }

    fn publish(&self) {
        self.os.publish(self.slots.iter().filter_map(|s| s.proc.as_ref().map(|p| p.pid())).collect());
    }

    /// How many apps were adopted rather than started (and so: the session was restarted).
    pub fn adopted(&self) -> usize {
        self.adopted
    }

    /// Where the bar stands, and how long since its latest start (since the session's start when
    /// the list has no bar).
    pub fn bar(&self) -> (Bar, Duration) {
        match self.slots.iter().find(|s| s.app.is_bar()) {
            Some(s) => (Bar::of(s.proc.is_some(), s.due.is_some()), s.started.elapsed()),
            None => (Bar::Absent, self.since_start()),
        }
    }

    /// Supervise again after a park: Explorer did not take over (see `safety::after_fallback`).
    /// Every app that is not running is scheduled again in order, with a fresh crash budget.
    pub fn resume(&mut self) {
        self.parked = false;
        self.os.parked(false);
        let now = Instant::now();
        for (i, s) in self.slots.iter_mut().enumerate() {
            if s.proc.is_none() {
                s.crashes = Crashes::default();
                s.due = Some(now + GAP * i as u32);
            }
        }
    }

    fn start(&mut self, i: usize) {
        let now = self.t0.elapsed();
        let slot = &mut self.slots[i];
        slot.due = None;
        if !self.os.exists(&slot.app) {
            log::line(&format!("{}: {} not found, not started", slot.app.name, slot.app.exe.display()));
            return;
        }
        match self.os.spawn(&slot.app) {
            Ok(p) => {
                log::line(&format!("{}: started, pid {}", slot.app.name, p.pid()));
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
        let requested = self.slots[i].restart.take().is_some();
        if self.os.quiet() {
            log::line(&format!("{name}: exited with {code:#x} while ending, not restarted"));
            return;
        }
        let next = if requested {
            log::line(&format!("{name}: exited with {code:#x} after {} ms, as asked: restarting", ran.as_millis()));
            self.slots[i].crashes.on_requested_exit()
        } else {
            log::line(&format!("{name}: exited with {code:#x} after {} ms", ran.as_millis()));
            self.slots[i].crashes.on_exit(code, now)
        };
        if next == Next::Deliberate {
            // A resident started with `--background` while its copy runs exits 0 at once: hold
            // that copy instead, so it is still watched.
            if let Some(p) = self.adopt(&self.slots[i].app) {
                log::line(&format!("{name}: another copy runs as pid {}, adopted", p.pid()));
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
        if self.parked {
            return;
        }
        for name in self.os.requests() {
            self.request_restart(&name);
        }
        for s in &mut self.slots {
            // Asked to close and did not: end it (still a requested restart).
            if let (Some(p), Some(deadline)) = (&s.proc, s.restart) {
                if deadline <= Instant::now() && p.exit_code().is_none() {
                    log::line(&format!("{}: did not close in {} s, ended", s.app.name, RESTART_GRACE.as_secs()));
                    p.terminate();
                    s.restart = Some(Instant::now() + RESTART_GRACE);
                }
            }
        }
        for i in 0..self.slots.len() {
            if let Some(code) = self.slots[i].proc.as_ref().and_then(|p| p.exit_code()) {
                self.exited(i, code);
            }
        }
        if !self.os.quiet() {
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
        if self.parked {
            return (Vec::new(), None);
        }
        let handles = self.slots.iter().filter_map(|s| s.proc.as_ref().and_then(|p| p.handle())).take(63).collect();
        let now = Instant::now();
        let due = self.slots.iter().filter_map(|s| s.due.or(s.restart)).min().map(|d| d.saturating_duration_since(now));
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

/// A fake system for the fleet's tests: processes are numbers whose exit code the test sets.
#[cfg(test)]
pub mod fake {
    use super::{App, Child, Os};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    #[derive(Default)]
    pub struct World {
        pub next_pid: Cell<u32>,
        /// pid -> exit code once it ended.
        pub exits: RefCell<Vec<(u32, Option<u32>)>>,
        /// Names of the apps started, in order.
        pub spawned: RefCell<Vec<String>>,
        pub closed: RefCell<Vec<u32>>,
        pub requests: RefCell<Vec<String>>,
        pub quiet: Cell<bool>,
        pub parked: Cell<Option<bool>>,
        pub missing: RefCell<Vec<String>>,
    }

    impl World {
        pub fn new() -> Rc<World> {
            let w = World::default();
            w.next_pid.set(100);
            Rc::new(w)
        }

        /// The pid of the latest copy of `name` that was started.
        pub fn pid_of(&self, name: &str) -> u32 {
            // Pids are handed out from 100 in start order.
            100 + self.spawned.borrow().iter().rposition(|s| s == name).expect("started") as u32
        }

        pub fn end(&self, pid: u32, code: u32) {
            for e in self.exits.borrow_mut().iter_mut() {
                if e.0 == pid {
                    e.1 = Some(code);
                }
            }
        }

        pub fn starts(&self, name: &str) -> usize {
            self.spawned.borrow().iter().filter(|s| *s == name).count()
        }
    }

    struct FakeChild(u32, Rc<World>);

    impl Child for FakeChild {
        fn pid(&self) -> u32 {
            self.0
        }
        fn exit_code(&self) -> Option<u32> {
            self.1.exits.borrow().iter().find(|e| e.0 == self.0).and_then(|e| e.1)
        }
        fn terminate(&self) {
            self.1.end(self.0, 1);
        }
    }

    pub struct Fake(pub Rc<World>);

    impl Os for Fake {
        fn exists(&self, app: &App) -> bool {
            !self.0.missing.borrow().contains(&app.name)
        }
        fn spawn(&self, app: &App) -> Result<Box<dyn Child>, String> {
            let pid = self.0.next_pid.get();
            self.0.next_pid.set(pid + 1);
            self.0.exits.borrow_mut().push((pid, None));
            self.0.spawned.borrow_mut().push(app.name.clone());
            Ok(Box::new(FakeChild(pid, self.0.clone())))
        }
        fn adopt(&self, _: &App, _: &[u32]) -> Option<Box<dyn Child>> {
            None
        }
        fn close_windows(&self, pid: u32) -> usize {
            self.0.closed.borrow_mut().push(pid);
            1
        }
        fn requests(&self) -> Vec<String> {
            std::mem::take(&mut *self.0.requests.borrow_mut())
        }
        fn quiet(&self) -> bool {
            self.0.quiet.get()
        }
        fn publish(&self, _: Vec<u32>) {}
        fn publish_names(&self, _: Vec<String>) {}
        fn parked(&self, parked: bool) {
            self.0.parked.set(Some(parked));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{Fake, World};
    use super::*;
    use std::rc::Rc;

    fn apps(names: &[&str]) -> Vec<App> {
        crate::apps::parse(&names.iter().map(|n| format!("C:\\t\\shellman-{n}.exe")).collect::<Vec<_>>().join(";"))
    }

    /// Step until nothing is due any more (the gaps between starts are real, but short).
    fn settle(fleet: &mut Fleet) {
        for _ in 0..40 {
            fleet.step();
            if fleet.waits().1.is_none() {
                return;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        panic!("still something due");
    }

    fn fleet(names: &[&str]) -> (Fleet, Rc<World>) {
        let world = World::new();
        let mut fleet = Fleet::with(apps(names), Box::new(Fake(world.clone())));
        settle(&mut fleet);
        (fleet, world)
    }

    #[test]
    fn starts_every_app_in_order() {
        let (_, world) = fleet(&["desktop", "bar", "shelf"]);
        assert_eq!(*world.spawned.borrow(), ["desktop", "bar", "shelf"]);
    }

    #[test]
    fn park_does_not_stop_the_startup_items() {
        let (mut fleet, world) = fleet(&["desktop", "bar"]);
        fleet.park();
        assert!(fleet.parked());
        assert_eq!(world.parked.get(), Some(true), "the window is told, to refuse restart requests");
        // The startup thread sleeps through `shutdown::sleep`; a park must not wake it to stop.
        assert!(!shutdown::STOPPING.load(std::sync::atomic::Ordering::SeqCst));
        assert!(shutdown::sleep(Duration::from_millis(1)), "a startup item wait still runs its course");
        fleet.resume();
        assert_eq!(world.parked.get(), Some(false));
        assert!(shutdown::sleep(Duration::from_millis(1)));
    }

    #[test]
    fn a_parked_fleet_starts_nothing() {
        let (mut fleet, world) = fleet(&["desktop", "bar"]);
        fleet.park();
        world.end(world.pid_of("bar"), 1);
        fleet.step();
        assert_eq!(world.starts("bar"), 1);
        assert_eq!(fleet.bar().0, Bar::Running, "not even looked at while parked");
    }

    #[test]
    fn requested_restarts_work_while_not_parked() {
        let (mut fleet, world) = fleet(&["desktop", "bar"]);
        let pid = world.pid_of("bar");
        world.requests.borrow_mut().push("bar".into());
        fleet.step();
        assert_eq!(*world.closed.borrow(), [pid], "asked to close");
        assert_eq!(fleet.bar().0, Bar::Running);
        world.end(pid, 1);
        fleet.step();
        assert_eq!(world.starts("bar"), 2, "started again at once, no pause");
        assert_eq!(fleet.bar().0, Bar::Running);
        // Not counted as a crash: the next real crash is the first, with the shortest pause.
        world.end(world.pid_of("bar"), 1);
        fleet.step();
        assert_eq!(fleet.bar().0, Bar::Pending);
        assert!(fleet.waits().1.is_some_and(|d| d <= crate::policy::DELAYS[0]));
    }

    #[test]
    fn a_restart_request_after_resume_works_again() {
        let (mut fleet, world) = fleet(&["bar"]);
        fleet.park();
        fleet.resume();
        let pid = world.pid_of("bar");
        world.requests.borrow_mut().push("bar".into());
        fleet.step();
        world.end(pid, 0);
        fleet.step();
        assert_eq!(world.starts("bar"), 2);
    }
}
