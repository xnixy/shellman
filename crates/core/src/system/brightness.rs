//! Screen brightness, per monitor: DDC/CI for external monitors (the monitor's own brightness
//! setting, over the video cable), and the video driver for a laptop's built-in panel.
//!
//! DDC/CI is slow (tens to hundreds of milliseconds a call) and some monitors never answer, so
//! everything happens on a worker: monitors are opened and read when the watch starts, a
//! monitor that does not answer is left out, and while a slider is dragged only the newest level
//! is written, at most once per `WRITE_GAP` per monitor. Dropping the watch closes the monitors.

use super::backlight::Backlight;
use super::display_names;
use super::live::{self, Mailbox, Wake};
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

/// A screen whose brightness can be set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    /// Stable while the watch runs: the GDI device name, or "builtin".
    pub id: String,
    /// The monitor's own name, such as "DELL U2720Q".
    pub name: String,
    /// 0..=100
    pub percent: u8,
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
    Ddc { monitor: PHYSICAL_MONITOR, max: u32 },
    Builtin(Backlight),
}

struct Panel {
    screen: Screen,
    control: Control,
    debounce: Debounce,
}

impl Panel {
    fn write(&self, percent: u8) {
        match &self.control {
            Control::Ddc { monitor, max } => {
                // SAFETY: an open physical monitor handle.
                unsafe {
                    SetVCPFeature(monitor.hPhysicalMonitor, VCP_BRIGHTNESS, to_raw(percent, *max));
                }
            }
            Control::Builtin(b) => {
                b.set_percent(percent);
            }
        }
    }

    fn close(self) {
        if let Control::Ddc { monitor, .. } = self.control {
            // SAFETY: opened by GetPhysicalMonitorsFromHMONITOR, closed once.
            unsafe {
                let _ = DestroyPhysicalMonitors(&[monitor]);
            }
        }
    }
}

fn run(rx: Receiver<Cmd>, out: Arc<Mailbox<Vec<Screen>>>, wake: Wake) {
    let publish = |panels: &[Panel]| {
        out.put(panels.iter().map(|p| p.screen.clone()).collect());
        wake();
    };
    let mut panels: Vec<Panel> = Vec::new();
    if let Some(b) = Backlight::open() {
        if let Some(percent) = b.percent() {
            let screen = Screen { id: "builtin".into(), name: "Built-in display".into(), percent };
            panels.push(Panel { screen, control: Control::Builtin(b), debounce: Debounce::default() });
        }
    }
    let names = display_names::friendly_names();
    for (device, monitor) in physical_monitors() {
        let mut max = 0u32;
        let mut now = 0u32;
        // SAFETY: an open physical monitor handle and valid out-pointers.
        let answered = unsafe {
            GetVCPFeatureAndVCPFeatureReply(monitor.hPhysicalMonitor, VCP_BRIGHTNESS, None, &mut now, Some(&mut max))
        } != 0;
        if !answered || max == 0 {
            // SAFETY: opened by GetPhysicalMonitorsFromHMONITOR, closed once.
            unsafe {
                let _ = DestroyPhysicalMonitors(&[monitor]);
            }
            continue;
        }
        let name = names.get(&device).map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
        let name = name.unwrap_or_else(|| "Display".into());
        let screen = Screen { id: device, name, percent: to_percent(now, max) };
        panels.push(Panel { screen, control: Control::Ddc { monitor, max }, debounce: Debounce::default() });
        publish(&panels);
    }
    publish(&panels);

    loop {
        let now = Instant::now();
        let wait = panels.iter().filter_map(|p| p.debounce.wait(now)).min();
        let cmd = match wait {
            Some(wait) => match rx.recv_timeout(wait) {
                Ok(cmd) => Some(cmd),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match rx.recv() {
                Ok(cmd) => Some(cmd),
                Err(_) => break,
            },
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
                p.write(percent);
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
}
