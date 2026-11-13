//! Windows Update through the Update Orchestrator's WinRT API (`Windows.Management.Update`,
//! bindings in `wu_api`), the component that owns update state on Windows 11: what is pending,
//! what it is doing, "Check for updates", approving an optional update. Nothing is downloaded or
//! installed through the older WUA API (it needs administrator rights and its state differs
//! from the orchestrator's); WUA is only read for the history (`update_history`).
//!
//! A `Watch` owns a worker thread (MTA) with the manager. Windows' events (progress, actions
//! done, scanning, working, attention) arrive on pool threads and are passed to the worker, which
//! reads the list again (well under a millisecond) and hands a `Snapshot` to the page. The page
//! keeps the watch while it shows; dropping it removes the handlers and ends the thread.
//!
//! Measured unelevated on 26300: the manager starts in 0.2 s; `StartScan(true)` (the same as
//! Settings' "Check for updates") checks in about 6 s and Windows then downloads and installs as
//! it normally would; `PerformScan(WhatIf)` changes nothing and makes Windows take in a pause.

use super::update_history;
use super::update_logic::{self as logic, ticks_to_unix, valid_update_id, Action, Facts, Flags, Kind, Overview};
use super::update_pause::{self, PauseState};
use super::wu_api::*;
use crate::registry::Real;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;
use windows::Foundation::TypedEventHandler;
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime};
use windows_core::{IInspectable, HSTRING};

/// How many history entries the page shows at most.
const HISTORY: i32 = 60;
/// The worker reads again this often even when Windows says nothing (a pause running out).
const IDLE: Duration = Duration::from_secs(30);

/// One pending update.
#[derive(Clone, PartialEq, Debug)]
pub struct Update {
    /// "<guid>:<revision>".
    pub id: String,
    pub title: String,
    pub kb: Option<String>,
    pub kind: Kind,
    /// Download size in bytes, when Windows says (second contract only, and not for every one).
    pub size: Option<u64>,
    pub facts: Facts,
}

/// Everything the page shows, read in one go.
#[derive(Clone, PartialEq, Debug)]
pub struct Snapshot {
    /// Why Windows Update cannot be used, when it cannot.
    pub unavailable: Option<String>,
    pub scanning: bool,
    /// Last successful check, seconds since 1970.
    pub last_check: Option<i64>,
    pub updates: Vec<Update>,
    pub pause: PauseState,
    /// The second contract is there: sizes, categories, approving optional updates.
    pub v2: bool,
    /// A line about the last thing asked for that did not work.
    pub notice: Option<String>,
}

impl Snapshot {
    pub fn overview(&self, now: i64) -> Overview {
        let active = |a: Action| self.updates.iter().any(|u| u.facts.progress.is_some() && u.facts.action == a);
        Overview {
            available: self.unavailable.is_none(),
            scanning: self.scanning,
            paused_until: self.pause.until.filter(|t| *t > now),
            downloading: active(Action::Download),
            installing: active(Action::Install),
            restart: self.updates.iter().any(|u| u.facts.restart_needed()),
            pending: self.updates.iter().filter(|u| !u.facts.optional()).count(),
        }
    }
}

/// What the worker tells the page.
pub enum Msg {
    Snap(Snapshot),
    History(Result<Vec<update_history::Entry>, String>),
}

enum Cmd {
    Check,
    Approve(String),
    Pause(u32),
    Resume,
    History,
    /// Something changed in Windows; read again.
    Changed,
    Progress { id: String, action: Action, value: f32 },
    Done { id: String, ok: bool, error: i32 },
    ScanFailed(i32),
    Stop,
}

/// The page's handle on the worker.
pub struct Watch {
    tx: Sender<Cmd>,
}

impl Watch {
    /// Start the worker; `sink` gets every message (from the worker thread).
    pub fn start(sink: impl Fn(Msg) + Send + 'static) -> Watch {
        let (tx, rx) = channel();
        let me = tx.clone();
        let _ = std::thread::Builder::new().name("windows update".into()).spawn(move || run(rx, me, sink));
        Watch { tx }
    }

    /// "Check for updates".
    pub fn check(&self) {
        let _ = self.tx.send(Cmd::Check);
    }

    /// Download and install an optional update.
    pub fn approve(&self, id: &str) {
        let _ = self.tx.send(Cmd::Approve(id.to_owned()));
    }

    pub fn pause(&self, weeks: u32) {
        let _ = self.tx.send(Cmd::Pause(weeks));
    }

    pub fn resume(&self) {
        let _ = self.tx.send(Cmd::Resume);
    }

    pub fn history(&self) {
        let _ = self.tx.send(Cmd::History);
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

/// Seconds since 1970 now.
pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// A UTC time (seconds since 1970) shifted to local time, with the daylight-saving rule of that
/// date. Unchanged if Windows cannot say.
pub fn local(utc: i64) -> i64 {
    let ticks = ((utc + 11_644_473_600) * 10_000_000) as u64;
    let ft = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    let (mut u, mut l, mut lft) = (SYSTEMTIME::default(), SYSTEMTIME::default(), FILETIME::default());
    // SAFETY: out-parameters on the stack.
    let ok = unsafe { FileTimeToSystemTime(&ft, &mut u).is_ok() && SystemTimeToTzSpecificLocalTime(None, &u, &mut l).is_ok() && SystemTimeToFileTime(&l, &mut lft).is_ok() };
    if !ok {
        return utc;
    }
    ticks_to_unix(((lft.dwHighDateTime as u64) << 32 | lft.dwLowDateTime as u64) as i64)
}

/// A null WinRT reference comes back as an error with code 0: read it as `None`.
fn opt<T>(r: windows_core::Result<T>) -> Option<T> {
    r.ok()
}

struct Worker {
    mgr: Option<WindowsUpdateManager>,
    unavailable: Option<String>,
    /// Progress from the events, by update id.
    live: HashMap<String, (Action, f32)>,
    /// Why the last action on an update failed, by id.
    failed: HashMap<String, String>,
    notice: Option<String>,
    sink: Box<dyn Fn(Msg) + Send>,
}

fn run(rx: Receiver<Cmd>, tx: Sender<Cmd>, sink: impl Fn(Msg) + Send + 'static) {
    // SAFETY: the thread's own COM start-up, matched below.
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let made = WindowsUpdateManager::CreateInstance(&HSTRING::from("shellman"));
    let mut w = Worker {
        unavailable: made.as_ref().err().map(|e| logic::unavailable_text(e.code().0)),
        mgr: made.ok(),
        live: HashMap::new(),
        failed: HashMap::new(),
        notice: None,
        sink: Box::new(sink),
    };
    let tokens = w.mgr.as_ref().and_then(|m| subscribe(m, &tx).ok());
    w.publish();
    loop {
        let cmd = match rx.recv_timeout(IDLE) {
            Ok(c) => c,
            Err(RecvTimeoutError::Timeout) => Cmd::Changed,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match cmd {
            Cmd::Stop => break,
            Cmd::Changed => {}
            Cmd::Progress { id, action, value } => {
                w.live.insert(id, (action, value));
            }
            Cmd::Done { id, ok, error } => {
                w.live.remove(&id);
                if ok {
                    w.failed.remove(&id);
                } else {
                    w.failed.insert(id, logic::failure_text(error));
                }
            }
            Cmd::ScanFailed(code) => w.notice = Some(format!("The check did not finish: {}.", logic::failure_text(code))),
            Cmd::Check => w.check(),
            Cmd::Approve(id) => w.approve(&id),
            Cmd::Pause(weeks) => w.pause(weeks),
            Cmd::Resume => w.resume(),
            Cmd::History => (w.sink)(Msg::History(update_history::read(HISTORY))),
        }
        w.publish();
    }
    if let (Some(m), Some(t)) = (w.mgr.as_ref(), tokens) {
        unsubscribe(m, t);
    }
    drop(w);
    if com {
        // SAFETY: matches the successful CoInitializeEx above; every COM object is dropped.
        unsafe { CoUninitialize() };
    }
}

type Tokens = [i64; 6];

fn subscribe(m: &WindowsUpdateManager, tx: &Sender<Cmd>) -> windows_core::Result<Tokens> {
    let id_of = |u: windows_core::Result<WindowsUpdate>| u.and_then(|u| u.UpdateId()).map(|s| s.to_string()).unwrap_or_default();
    let t = tx.clone();
    let progress = m.ProgressChanged(&TypedEventHandler::<WindowsUpdateManager, WindowsUpdateProgressChangedEventArgs>::new(move |_, a| {
        if let Some(a) = a.as_ref() {
            let p = a.ActionProgress()?;
            let _ = t.send(Cmd::Progress { id: id_of(a.Update()), action: Action::parse(&p.Action()?.to_string()), value: p.Progress()? as f32 });
        }
        Ok(())
    }))?;
    let t = tx.clone();
    let done = m.ActionCompleted(&TypedEventHandler::<WindowsUpdateManager, WindowsUpdateActionCompletedEventArgs>::new(move |_, a| {
        if let Some(a) = a.as_ref() {
            let _ = t.send(Cmd::Done { id: id_of(a.Update()), ok: a.Succeeded().unwrap_or(true), error: a.ExtendedError().map(|e| e.0).unwrap_or(0) });
        }
        Ok(())
    }))?;
    let changed = |t: Sender<Cmd>| {
        TypedEventHandler::<WindowsUpdateManager, IInspectable>::new(move |_, _| {
            let _ = t.send(Cmd::Changed);
            Ok(())
        })
    };
    let scanning = m.ScanningStateChanged(&changed(tx.clone()))?;
    let working = m.WorkingStateChanged(&changed(tx.clone()))?;
    let t = tx.clone();
    let attention = m.AttentionRequiredReasonChanged(&TypedEventHandler::<WindowsUpdateManager, WindowsUpdateAttentionRequiredReasonChangedEventArgs>::new(move |_, _| {
        let _ = t.send(Cmd::Changed);
        Ok(())
    }))?;
    let t = tx.clone();
    let scanned = m.ScanCompleted(&TypedEventHandler::<WindowsUpdateManager, WindowsUpdateScanCompletedEventArgs>::new(move |_, a| {
        if let Some(a) = a.as_ref() {
            if a.Succeeded() == Ok(false) {
                let _ = t.send(Cmd::ScanFailed(a.ExtendedError().map(|e| e.0).unwrap_or(0)));
            }
        }
        Ok(())
    }))?;
    Ok([progress, done, scanning, working, attention, scanned])
}

fn unsubscribe(m: &WindowsUpdateManager, t: Tokens) {
    let _ = m.RemoveProgressChanged(t[0]);
    let _ = m.RemoveActionCompleted(t[1]);
    let _ = m.RemoveScanningStateChanged(t[2]);
    let _ = m.RemoveWorkingStateChanged(t[3]);
    let _ = m.RemoveAttentionRequiredReasonChanged(t[4]);
    let _ = m.RemoveScanCompleted(t[5]);
}

impl Worker {
    fn publish(&self) {
        (self.sink)(Msg::Snap(self.read()));
    }

    fn read(&self) -> Snapshot {
        let pause = update_pause::read(&Real, now());
        let mut snap = Snapshot { unavailable: self.unavailable.clone(), scanning: false, last_check: None, updates: Vec::new(), pause, v2: false, notice: self.notice.clone() };
        let Some(m) = &self.mgr else { return snap };
        let list = match m.GetApplicableUpdates() {
            Ok(l) => l,
            Err(e) => {
                snap.unavailable = Some(logic::unavailable_text(e.code().0));
                return snap;
            }
        };
        snap.scanning = m.IsScanning().unwrap_or(false);
        snap.last_check = opt(m.LastSuccessfulScanTimestamp()).and_then(|r| opt(r.Value())).map(|d| ticks_to_unix(d.UniversalTime));
        // The second contract, where this Windows has it: categories and sizes, by id.
        let more: Option<HashMap<String, (i32, u64)>> = m.GetApplicableSoftwareUpdates().ok().map(|v2| {
            v2.into_iter()
                .filter_map(|u| Some((u.UpdateId().ok()?.to_string(), (u.UpdateCategory().map(|c| c.0).unwrap_or(0), u.DownloadSizeInBytes().unwrap_or(0)))))
                .collect()
        });
        snap.v2 = more.is_some();
        for u in list {
            let id = u.UpdateId().map(|s| s.to_string()).unwrap_or_default();
            let title = u.Title().map(|s| s.to_string()).unwrap_or_default();
            let flags = Flags { driver: u.IsDriver().unwrap_or(false), feature: u.IsFeatureUpdate().unwrap_or(false), security: u.IsSecurity().unwrap_or(false) };
            let (category, size) = more.as_ref().and_then(|m| m.get(&id)).copied().unzip();
            let action = Action::parse(&u.CurrentAction().map(|s| s.to_string()).unwrap_or_default());
            let live = self.live.get(&id).filter(|(a, _)| *a == action).map(|(_, p)| *p);
            let last = opt(u.ActionResult()).filter(|r| r.Succeeded() == Ok(false)).map(|r| logic::failure_text(r.ExtendedError().map(|e| e.0).unwrap_or(0)));
            let facts = Facts {
                action,
                reason: opt(u.AttentionRequiredInfo()).and_then(|a| opt(a.Reason())).map_or(0, |r| r.0),
                seeker: u.IsSeeker().unwrap_or(false),
                progress: live,
                failed: self.failed.get(&id).cloned().or(last),
            };
            snap.updates.push(Update { kb: logic::kb(&title), kind: logic::kind_of(category, flags, &title), size: size.filter(|s| *s > 0), title, id, facts });
        }
        snap
    }

    fn check(&mut self) {
        self.notice = None;
        let Some(m) = &self.mgr else { return };
        if let Err(e) = m.StartScan(true) {
            self.notice = Some(format!("Windows did not start the check: {}.", logic::failure_text(e.code().0)));
        }
    }

    fn approve(&mut self, id: &str) {
        self.notice = self.try_approve(id).err();
    }

    fn try_approve(&self, id: &str) -> Result<(), String> {
        if !valid_update_id(id) {
            return Err("That update could not be found.".to_owned());
        }
        let m = self.mgr.as_ref().ok_or("Windows Update is not available.")?;
        let list = m.GetApplicableSoftwareUpdates().map_err(|_| "This version of Windows only installs optional updates from its own Settings.".to_owned())?;
        let u = list.into_iter().find(|u| u.UpdateId().is_ok_and(|s| s.to_string() == id)).ok_or("That update is no longer offered.")?;
        let denied = |e: &windows_core::Error| e.code().0 as u32 == 0x8007_0005;
        let info = WindowsSoftwareUpdateApprovalInfo::CreateInstance(true, false, false, true).map_err(|e| format!("Windows did not take the request: {}.", logic::failure_text(e.code().0)))?;
        let r = u.Approve(&info).map_err(|e| if denied(&e) { "Windows refused: installing this one needs administrator rights.".to_owned() } else { format!("Windows did not take the request: {}.", logic::failure_text(e.code().0)) })?;
        if r.Succeeded().unwrap_or(false) {
            Ok(())
        } else {
            Err(format!("Windows did not take the request: {}.", logic::failure_text(r.ExtendedError().unwrap_or(0) as u32 as i32)))
        }
    }

    fn pause(&mut self, weeks: u32) {
        self.notice = None;
        if let Err(e) = update_pause::pause(&Real, now(), weeks) {
            self.notice = Some(e.line());
            return;
        }
        // Show the pause at once; Windows takes it in at its next scan, which a what-if scan
        // (second contract) brings about without changing anything else.
        self.publish();
        if let Some(m) = &self.mgr {
            let _ = m.PerformScan(WindowsUpdateManagerScanMode::WhatIf);
        }
    }

    fn resume(&mut self) {
        self.notice = None;
        if let Err(e) = update_pause::resume(&Real) {
            self.notice = Some(e.line());
            return;
        }
        self.publish();
        // As Windows' Settings does on resuming: check, which also brings the updates back.
        self.check();
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_time_is_a_whole_quarter_hour_away() {
        let t = 1_794_535_000;
        let off = local(t) - t;
        assert_eq!(off % 900, 0);
        assert!(off.abs() <= 14 * 3600);
    }

    #[test]
    fn the_overview_counts_only_what_windows_installs_by_itself() {
        let f = |action: &str, reason: i32, seeker: bool, progress: Option<f32>| Update {
            id: String::new(),
            title: String::new(),
            kb: None,
            kind: Kind::Driver,
            size: None,
            facts: Facts { action: Action::parse(action), reason, seeker, progress, failed: None },
        };
        let mut s = Snapshot { unavailable: None, scanning: false, last_check: None, updates: vec![f("Download", 1, true, None)], pause: PauseState { until: None, windows_agrees: false, max_days: 35 }, v2: true, notice: None };
        assert_eq!(logic::headline(&s.overview(0)), logic::Headline::UpToDate, "an optional update alone");
        s.updates.push(f("Commit", 2, false, None));
        assert_eq!(logic::headline(&s.overview(0)), logic::Headline::RestartNeeded);
        s.updates.push(f("Download", 0, false, Some(0.3)));
        assert_eq!(logic::headline(&s.overview(0)), logic::Headline::Downloading);
        s.pause.until = Some(100);
        assert_eq!(s.overview(200).paused_until, None, "a pause that ran out");
    }

    /// Reads this machine's updates (read only): `cargo test -p shellman-core updates::tests::read_this_machine -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn read_this_machine() {
        let (tx, rx) = channel();
        let w = Watch::start(move |m| {
            let _ = tx.send(m);
        });
        if let Ok(Msg::Snap(s)) = rx.recv_timeout(Duration::from_secs(10)) {
            println!("v2 {} unavailable {:?} scanning {} last {:?} pause {:?}", s.v2, s.unavailable, s.scanning, s.last_check, s.pause);
            for u in &s.updates {
                println!("{:?} {:?} {:?} {} | {} | {}", u.kind, u.kb, u.size, u.id, u.title, u.facts.status());
            }
            println!("{:?}", logic::headline(&s.overview(now())));
        }
        drop(w);
    }
}
