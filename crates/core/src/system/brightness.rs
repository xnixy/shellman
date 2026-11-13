//! Screen brightness, per monitor: DDC/CI for external monitors (the monitor's own brightness
//! setting, over the video cable), the video driver for a laptop's built-in panel, and, for a
//! monitor running HDR, Windows' "SDR content brightness" (`display::sdr`): in HDR the monitor
//! ignores its own brightness setting, and the level everyday content is drawn at is what counts.
//!
//! DDC/CI is slow (tens to hundreds of milliseconds a call) and some monitors never answer, so
//! everything happens on a worker: monitors are opened and read when the watch starts, a
//! monitor that does not answer is left out, and while a slider is dragged only the newest level
//! is written, at most once per `WRITE_GAP` per monitor. HDR screens are read first (cheap) and
//! looked at again every `POLL`, so HDR switched on or off, or the level changed by another tool,
//! shows up while the watch runs. Dropping the watch closes the monitors.

use super::backlight::Backlight;
use super::display::sdr;
use super::display_names;
use super::live::{self, Mailbox, Wake};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, GetVCPFeatureAndVCPFeatureReply, SetVCPFeature,
    PHYSICAL_MONITOR,
};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};

/// The MCCS code for luminance.
const VCP_BRIGHTNESS: u8 = 0x10;
/// Monitors' controllers fall over when written to faster than this.
pub const WRITE_GAP: Duration = Duration::from_millis(120);
/// How often HDR state and levels are read again for changes made elsewhere.
const POLL: Duration = Duration::from_millis(1500);
/// After the last value of a drag, this long passes before it is sent as the final one.
const SETTLE: Duration = Duration::from_millis(400);

/// What a screen's slider changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The monitor's own brightness, over DDC/CI.
    Monitor,
    /// A laptop panel's backlight.
    Builtin,
    /// HDR is on: how bright everyday (SDR) content is drawn, as Windows' own slider sets it.
    Hdr,
}

/// A screen whose brightness can be set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    /// Stable while the watch runs: the GDI device name, or "builtin".
    pub id: String,
    /// The monitor's own name, such as "DELL U2720Q".
    pub name: String,
    /// 0..=100; for `Kind::Hdr` the position Windows' own slider shows.
    pub percent: u8,
    pub kind: Kind,
}

impl Screen {
    /// The slider's heading.
    pub fn label(&self) -> &'static str {
        match self.kind {
            Kind::Hdr => "Brightness (HDR)",
            _ => "Brightness",
        }
    }

    /// A line saying what the slider does, when it is not the obvious.
    pub fn hint(&self) -> Option<&'static str> {
        (self.kind == Kind::Hdr)
            .then_some("HDR is on, so this sets how bright apps and the desktop look. HDR games and video keep their own brightness.")
    }
}

enum Cmd {
    Set(String, u8),
    Stop,
}

pub struct BrightnessWatch {
    tx: Sender<Cmd>,
    mailbox: Arc<Mailbox<Vec<Screen>>>,
}

impl BrightnessWatch {
    /// Start reading every screen; the list follows (screen by screen, as each answers).
    pub fn start(wake: Wake) -> Self {
        let (tx, rx) = channel();
        let mailbox = Arc::new(Mailbox::default());
        let out = mailbox.clone();
        live::spawn("brightness-watch", move || run(rx, out, wake));
        Self { tx, mailbox }
    }

    pub fn take(&self) -> Option<Vec<Screen>> {
        self.mailbox.take()
    }

    /// Ask for a level; written shortly, coalesced with any that follow.
    pub fn set(&self, id: &str, percent: u8) {
        let _ = self.tx.send(Cmd::Set(id.to_string(), percent.min(100)));
    }
}

impl Drop for BrightnessWatch {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

enum Control {
    /// Boxed: the monitor record carries its 256-byte description.
    Ddc { monitor: Box<PHYSICAL_MONITOR>, max: u32 },
    Builtin(Backlight),
    /// Every monitor showing this picture (more than one when duplicated); `unsaved` is when the
    /// last live value went out, until it has been sent again as the final one.
    Sdr { targets: Vec<sdr::Target>, unsaved: Option<Instant> },
}

struct Panel {
    screen: Screen,
    control: Control,
    debounce: Debounce,
}

impl Panel {
    fn write(&mut self, percent: u8, now: Instant) {
        match &mut self.control {
            Control::Ddc { monitor, max } => {
                // SAFETY: an open physical monitor handle.
                unsafe {
                    SetVCPFeature(monitor.hPhysicalMonitor, VCP_BRIGHTNESS, to_raw(percent, *max));
                }
            }
            Control::Builtin(b) => {
                b.set_percent(percent);
            }
            Control::Sdr { targets, unsaved } => {
                for t in targets.iter() {
                    sdr::set_white_level(*t, sdr::level_for(percent), false);
                }
                *unsaved = Some(now);
            }
        }
    }

    /// When the last value of a drag goes out again as the final one.
    fn settle_at(&self) -> Option<Instant> {
        match &self.control {
            Control::Sdr { unsaved: Some(at), .. } if self.debounce.pending.is_none() => Some(*at + SETTLE),
            _ => None,
        }
    }

    fn settle(&mut self, now: Instant) {
        if !self.settle_at().is_some_and(|at| at <= now) {
            return;
        }
        let level = sdr::level_for(self.screen.percent);
        if let Control::Sdr { targets, unsaved } = &mut self.control {
            for t in targets.iter() {
                sdr::set_white_level(*t, level, true);
            }
            *unsaved = None;
        }
    }

    /// Being changed from here: what is read back is not the truth yet.
    fn busy(&self) -> bool {
        self.debounce.pending.is_some() || matches!(self.control, Control::Sdr { unsaved: Some(_), .. })
    }

    fn close(self) {
        if let Control::Ddc { monitor, .. } = self.control {
            // SAFETY: opened by GetPhysicalMonitorsFromHMONITOR, closed once.
            unsafe {
                let _ = DestroyPhysicalMonitors(&[*monitor]);
            }
        }
    }
}

/// The screens that get the HDR slider: each GDI device with HDR on, its monitors, and the
/// slider position for its level. The others keep DDC or the backlight.
pub fn hdr_screens(states: &[sdr::State]) -> Vec<(String, Vec<sdr::Target>, u8)> {
    let mut out: Vec<(String, Vec<sdr::Target>, u8)> = Vec::new();
    for s in states.iter().filter(|s| s.hdr) {
        let Some(level) = s.level else { continue };
        match out.iter_mut().find(|(d, _, _)| *d == s.device) {
            Some((_, targets, _)) => targets.push(s.target),
            None => out.push((s.device.clone(), vec![s.target], sdr::percent_for(level))),
        }
    }
    out
}

/// Open every screen: the built-in panel, the HDR ones (instant), then DDC monitors one by one.
fn open_panels(publish: &dyn Fn(&[Panel])) -> Vec<Panel> {
    let names: HashMap<String, String> = display_names::friendly_names();
    let name_of = |device: &str| {
        let name = names.get(device).map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
        name.unwrap_or_else(|| "Display".into())
    };
    let mut panels: Vec<Panel> = Vec::new();
    if let Some(b) = Backlight::open() {
        if let Some(percent) = b.percent() {
            let screen = Screen { id: "builtin".into(), name: "Built-in display".into(), percent, kind: Kind::Builtin };
            panels.push(Panel { screen, control: Control::Builtin(b), debounce: Debounce::default() });
        }
    }
    let hdr = hdr_screens(&sdr::states());
    for (device, targets, percent) in &hdr {
        let screen = Screen { id: device.clone(), name: name_of(device), percent: *percent, kind: Kind::Hdr };
        let control = Control::Sdr { targets: targets.clone(), unsaved: None };
        panels.push(Panel { screen, control, debounce: Debounce::default() });
    }
    publish(&panels);
    for (device, monitor) in physical_monitors() {
        let mut max = 0u32;
        let mut now = 0u32;
        let hdr_on = hdr.iter().any(|(d, _, _)| *d == device);
        // SAFETY: an open physical monitor handle and valid out-pointers.
        let answered = !hdr_on
            && unsafe {
                GetVCPFeatureAndVCPFeatureReply(monitor.hPhysicalMonitor, VCP_BRIGHTNESS, None, &mut now, Some(&mut max))
            } != 0;
        if !answered || max == 0 {
            // SAFETY: opened by GetPhysicalMonitorsFromHMONITOR, closed once.
            unsafe {
                let _ = DestroyPhysicalMonitors(&[monitor]);
            }
            continue;
        }
        let screen = Screen { name: name_of(&device), id: device, percent: to_percent(now, max), kind: Kind::Monitor };
        panels.push(Panel { screen, control: Control::Ddc { monitor: Box::new(monitor), max }, debounce: Debounce::default() });
        publish(&panels);
    }
    publish(&panels);
    panels
}

fn run(rx: Receiver<Cmd>, out: Arc<Mailbox<Vec<Screen>>>, wake: Wake) {
    let publish = |panels: &[Panel]| {
        out.put(panels.iter().map(|p| p.screen.clone()).collect());
        wake();
    };
    let mut panels = open_panels(&publish);
    let mut next_poll = Instant::now() + POLL;

    loop {
        let now = Instant::now();
        let wait = panels
            .iter()
            .filter_map(|p| p.debounce.wait(now))
            .chain(panels.iter().filter_map(Panel::settle_at).map(|at| at.saturating_duration_since(now)))
            .fold(next_poll.saturating_duration_since(now), Duration::min);
        let cmd = match rx.recv_timeout(wait) {
            Ok(cmd) => Some(cmd),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match cmd {
            Some(Cmd::Stop) => break,
            Some(Cmd::Set(id, percent)) => {
                if let Some(p) = panels.iter_mut().find(|p| p.screen.id == id) {
                    p.debounce.want(percent);
                    p.screen.percent = percent;
                }
            }
            None => {}
        }
        let now = Instant::now();
        for p in &mut panels {
            if let Some(percent) = p.debounce.due(now) {
                p.write(percent, now);
            }
            p.settle(now);
        }
        if now >= next_poll {
            next_poll = now + POLL;
            let hdr = hdr_screens(&sdr::states());
            let had: Vec<&str> = panels.iter().filter(|p| p.screen.kind == Kind::Hdr).map(|p| p.screen.id.as_str()).collect();
            let has: Vec<&str> = hdr.iter().map(|(d, _, _)| d.as_str()).collect();
            if had != has {
                // HDR went on or off somewhere, or a screen came or went: start again.
                for p in std::mem::take(&mut panels) {
                    p.close();
                }
                panels = open_panels(&publish);
                continue;
            }
            let mut changed = false;
            for (device, _, percent) in &hdr {
                if let Some(p) = panels.iter_mut().find(|p| p.screen.id == *device && !p.busy()) {
                    if p.screen.percent != *percent {
                        p.screen.percent = *percent;
                        changed = true;
                    }
                }
            }
            if changed {
                publish(&panels);
            }
        }
    }
    for p in panels {
        p.close();
    }
}

/// Every physical monitor with the GDI device name of the display it shows.
fn physical_monitors() -> Vec<(String, PHYSICAL_MONITOR)> {
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, list: LPARAM) -> windows::core::BOOL {
        (*(list.0 as *mut Vec<HMONITOR>)).push(m);
        true.into()
    }
    let mut handles: Vec<HMONITOR> = Vec::new();
    let mut out = Vec::new();
    // SAFETY: the callback only pushes into `handles`, which outlives the call; buffers are
    // sized by the count Windows reports.
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&raw mut handles as isize));
        for m in handles {
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
            if !GetMonitorInfoW(m, (&raw mut info).cast::<MONITORINFO>()).as_bool() {
                continue;
            }
            let device = display_names::wide_str(&info.szDevice);
            let mut count = 0u32;
            if GetNumberOfPhysicalMonitorsFromHMONITOR(m, &mut count).is_err() || count == 0 {
                continue;
            }
            let mut found = vec![PHYSICAL_MONITOR::default(); count as usize];
            if GetPhysicalMonitorsFromHMONITOR(m, &mut found).is_err() {
                continue;
            }
            out.extend(found.into_iter().map(|p| (device.clone(), p)));
        }
    }
    out
}

/// A monitor's raw value (0..=max) as a percentage.
pub fn to_percent(raw: u32, max: u32) -> u8 {
    if max == 0 {
        return 0;
    }
    ((raw.min(max) as f32 / max as f32) * 100.0).round() as u8
}

/// A percentage as the monitor's raw value (0..=max).
pub fn to_raw(percent: u8, max: u32) -> u32 {
    ((percent.min(100) as f32 / 100.0) * max as f32).round() as u32
}

/// Holds the newest level asked for and lets it through at most once per `WRITE_GAP`: the first
/// change goes out at once, the rest of a drag in steps, and the last value always lands.
#[derive(Debug, Default)]
pub struct Debounce {
    pending: Option<u8>,
    last_write: Option<Instant>,
}

impl Debounce {
    pub fn want(&mut self, percent: u8) {
        self.pending = Some(percent);
    }

    /// How long until the pending value may be written; None when nothing is pending.
    pub fn wait(&self, now: Instant) -> Option<Duration> {
        self.pending?;
        Some(match self.last_write {
            Some(at) => (at + WRITE_GAP).saturating_duration_since(now),
            None => Duration::ZERO,
        })
    }

    /// The value to write now, if one is pending and the gap has passed.
    pub fn due(&mut self, now: Instant) -> Option<u8> {
        if self.wait(now)? > Duration::ZERO {
            return None;
        }
        self.last_write = Some(now);
        self.pending.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_round_trips() {
        assert_eq!(to_percent(50, 100), 50);
        assert_eq!(to_percent(255, 255), 100);
        assert_eq!(to_percent(300, 255), 100);
        assert_eq!(to_percent(5, 0), 0);
        for max in [100, 255, 75] {
            for p in [0u8, 1, 33, 50, 99, 100] {
                let back = to_percent(to_raw(p, max), max);
                assert!((back as i16 - p as i16).abs() <= 1, "{p} via {max} came back {back}");
            }
        }
    }

    fn state(device: &str, id: u32, hdr: bool, level: u32) -> sdr::State {
        sdr::State { device: device.into(), target: sdr::Target { adapter: (1, 0), id }, hdr, level: hdr.then_some(level) }
    }

    #[test]
    fn hdr_monitors_get_the_hdr_slider_and_the_others_do_not() {
        let states = [state("D1", 1, true, 3000), state("D2", 2, false, 1000), state("D3", 3, true, 6000)];
        let got = hdr_screens(&states);
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].0.as_str(), got[0].2), ("D1", 40));
        assert_eq!((got[1].0.as_str(), got[1].2), ("D3", 100));
    }

    #[test]
    fn duplicated_hdr_monitors_share_one_slider() {
        let got = hdr_screens(&[state("D1", 1, true, 2000), state("D1", 2, true, 2000)]);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1.len(), 2);
        assert_eq!(got[0].2, 20);
    }

    #[test]
    fn hdr_screens_say_so() {
        let s = Screen { id: "D1".into(), name: "XG32".into(), percent: 40, kind: Kind::Hdr };
        assert_eq!(s.label(), "Brightness (HDR)");
        assert!(s.hint().is_some());
        let s = Screen { kind: Kind::Monitor, ..s };
        assert_eq!(s.label(), "Brightness");
        assert!(s.hint().is_none());
    }

    #[test]
    fn debounce_writes_first_at_once_then_the_newest_after_the_gap() {
        let t0 = Instant::now();
        let mut d = Debounce::default();
        assert_eq!(d.wait(t0), None);
        d.want(10);
        assert_eq!(d.due(t0), Some(10));
        d.want(20);
        d.want(30);
        assert_eq!(d.due(t0 + Duration::from_millis(50)), None);
        assert_eq!(d.wait(t0 + Duration::from_millis(50)), Some(WRITE_GAP - Duration::from_millis(50)));
        assert_eq!(d.due(t0 + WRITE_GAP), Some(30));
        assert_eq!(d.due(t0 + WRITE_GAP * 3), None);
    }

    /// What this machine's monitors answer; run with `--ignored --nocapture`. Reads only.
    #[test]
    #[ignore]
    fn probe() {
        println!("{:?}", display_names::friendly_names());
        let wake: Wake = Arc::new(|| {});
        let watch = BrightnessWatch::start(wake);
        std::thread::sleep(Duration::from_secs(3));
        println!("{:?}", watch.take());
    }

    /// Through the watch, as the sliders do: the screen named by SHELLMAN_BRIGHTNESS_SCREEN (a GDI
    /// name; pick a secondary one) two points up and back, read back by a second watch each time.
    /// By hand with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn nudge_a_screen_and_put_it_back() {
        let Ok(id) = std::env::var("SHELLMAN_BRIGHTNESS_SCREEN") else { return };
        let read = || {
            let again = BrightnessWatch::start(Arc::new(|| {}));
            std::thread::sleep(Duration::from_secs(2));
            again.take().and_then(|s| s.into_iter().find(|s| s.id == id))
        };
        let Some(screen) = read() else {
            println!("{id} does not answer");
            return;
        };
        let watch = BrightnessWatch::start(Arc::new(|| {}));
        std::thread::sleep(Duration::from_secs(2));
        let was = screen.percent;
        watch.set(&id, (was + 2).min(100));
        std::thread::sleep(Duration::from_millis(900));
        let raised = read().map(|s| s.percent);
        watch.set(&id, was);
        std::thread::sleep(Duration::from_millis(900));
        let now = read().map(|s| s.percent);
        println!("{} ({:?}) was {was}, raised {raised:?}, now {now:?}", screen.name, screen.kind);
        assert_eq!(now, Some(was));
    }
}
