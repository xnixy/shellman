//! The Sound page's state and the handle the UI holds. A worker thread (`sound_worker`) does all
//! the COM work and publishes a `Page` into a mailbox; the UI takes the newest on its next frame
//! and sends commands back. Nothing here polls.

use super::live::{self, Mailbox, Wake};
use super::sound_dev::{Device, Flow};
use super::sound_format::FormatSpec;
use super::sound_sessions::AppSession;
use super::sound_spatial::Spatial;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;

/// What is shown for the chosen device of a section.
#[derive(Debug, Clone, PartialEq)]
pub struct Detail {
    /// -1 (left) ..= 1 (right); None for a device with one channel.
    pub balance: Option<f32>,
    /// What the device is set to; None when Windows would not say.
    pub format: Option<FormatSpec>,
    /// What it takes, in exclusive mode, lowest first.
    pub supported: Vec<FormatSpec>,
    /// Whether the last format change can be taken back.
    pub can_undo: bool,
    /// Spatial sound (outputs); None when Windows would not say.
    pub spatial: Option<Spatial>,
    /// Why the last spatial change or test tone did not happen, for this device.
    pub spatial_error: Option<String>,
    pub tone_error: Option<String>,
}

/// Outputs or inputs, with the chosen device.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Section {
    pub devices: Vec<Device>,
    /// The device the controls below the list are for: the user's choice, else the default.
    pub selected: Option<String>,
    pub detail: Option<Detail>,
}

impl Section {
    pub fn selected_device(&self) -> Option<&Device> {
        let id = self.selected.as_deref()?;
        self.devices.iter().find(|d| d.id == id)
    }
}

/// Everything the page shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub output: Section,
    pub input: Section,
    /// The mixer: sessions of the default output.
    pub sessions: Vec<AppSession>,
}

impl Page {
    pub fn section(&self, flow: Flow) -> &Section {
        match flow {
            Flow::Output => &self.output,
            Flow::Input => &self.input,
        }
    }
}

/// Which roles a device is made the default for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Apps and system sounds: the console and multimedia roles.
    Everything,
    /// Only the communications role (calls).
    Calls,
}

/// The chosen device: the one asked for if it is still listed, else the default, else the first.
pub fn resolve_selected(devices: &[Device], asked: Option<&str>) -> Option<String> {
    asked
        .filter(|id| devices.iter().any(|d| d.id == *id))
        .or_else(|| devices.iter().find(|d| d.default).map(|d| d.id.as_str()))
        .or_else(|| devices.first().map(|d| d.id.as_str()))
        .map(str::to_owned)
}

pub(crate) enum Cmd {
    Refresh,
    /// A device came or went, or a default changed: attach again, then read.
    Reattach,
    Select(Flow, String),
    Volume(String, f32),
    Mute(String, bool),
    Default(String, Flow, Role),
    Balance(String, f32),
    Format(String, FormatSpec),
    UndoFormat(String),
    Tone(String),
    /// The test tone could not start (false) or played (true) on this device.
    ToneDone(String, bool),
    /// Select this spatial format (empty for off).
    Spatial(String, String),
    SpatialDone(String, Result<(), String>),
    /// Start or stop the input level meter.
    Meter(Option<String>),
    SessionVolume(String, f32),
    SessionMute(String, bool),
    Stop,
}

pub struct SoundPage {
    tx: Sender<Cmd>,
    mailbox: Arc<Mailbox<Page>>,
    peak: Arc<AtomicU32>,
}

impl SoundPage {
    /// Start following; `wake` is called whenever there is something new. The first reading
    /// follows at once.
    pub fn start(wake: Wake) -> Self {
        let (tx, rx) = channel();
        let mailbox = Arc::new(Mailbox::default());
        let peak = Arc::new(AtomicU32::new(0));
        let (to_self, out, level) = (tx.clone(), mailbox.clone(), peak.clone());
        live::spawn("sound-page", move || super::sound_worker::run(rx, to_self, out, level, wake));
        let _ = tx.send(Cmd::Refresh);
        Self { tx, mailbox, peak }
    }

    /// The newest reading since the last call.
    pub fn take(&self) -> Option<Page> {
        self.mailbox.take()
    }

    /// The input meter's level, 0..=1, while the meter runs.
    pub fn peak(&self) -> f32 {
        f32::from_bits(self.peak.load(Ordering::Relaxed))
    }

    fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn select(&self, flow: Flow, id: &str) {
        self.send(Cmd::Select(flow, id.to_owned()));
    }
    pub fn set_volume(&self, id: &str, level: f32) {
        self.send(Cmd::Volume(id.to_owned(), level));
    }
    pub fn set_muted(&self, id: &str, muted: bool) {
        self.send(Cmd::Mute(id.to_owned(), muted));
    }
    pub fn set_default(&self, id: &str, flow: Flow, role: Role) {
        self.send(Cmd::Default(id.to_owned(), flow, role));
    }
    pub fn set_balance(&self, id: &str, balance: f32) {
        self.send(Cmd::Balance(id.to_owned(), balance));
    }
    pub fn set_format(&self, id: &str, format: FormatSpec) {
        self.send(Cmd::Format(id.to_owned(), format));
    }
    /// Put the format back to what it was before the last change on `id`.
    pub fn undo_format(&self, id: &str) {
        self.send(Cmd::UndoFormat(id.to_owned()));
    }
    pub fn set_spatial(&self, id: &str, subtype: &str) {
        self.send(Cmd::Spatial(id.to_owned(), subtype.to_owned()));
    }
    pub fn play_tone(&self, id: &str) {
        self.send(Cmd::Tone(id.to_owned()));
    }
    /// Run the meter on microphone `id`, or stop it with None.
    pub fn meter(&self, id: Option<&str>) {
        self.send(Cmd::Meter(id.map(str::to_owned)));
    }
    pub fn set_session_volume(&self, key: &str, level: f32) {
        self.send(Cmd::SessionVolume(key.to_owned(), level));
    }
    pub fn set_session_muted(&self, key: &str, muted: bool) {
        self.send(Cmd::SessionMute(key.to_owned(), muted));
    }
}

impl Drop for SoundPage {
    fn drop(&mut self) {
        self.send(Cmd::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::sound_dev::Kind;

    fn dev(id: &str, default: bool) -> Device {
        Device { id: id.into(), name: id.into(), kind: Kind::Other, default, default_comm: false, volume: 0.5, muted: false }
    }

    #[test]
    fn the_choice_survives_until_the_device_goes() {
        let list = [dev("a", true), dev("b", false)];
        assert_eq!(resolve_selected(&list, Some("b")).as_deref(), Some("b"));
        assert_eq!(resolve_selected(&list, Some("gone")).as_deref(), Some("a"), "falls back to the default");
        assert_eq!(resolve_selected(&list, None).as_deref(), Some("a"));
        let none_default = [dev("x", false), dev("y", false)];
        assert_eq!(resolve_selected(&none_default, None).as_deref(), Some("x"));
        assert_eq!(resolve_selected(&[], Some("a")), None);
    }

    #[test]
    fn a_section_finds_its_selected_device() {
        let s = Section { devices: vec![dev("a", true), dev("b", false)], selected: Some("b".into()), detail: None };
        assert_eq!(s.selected_device().map(|d| d.id.as_str()), Some("b"));
        assert!(Section::default().selected_device().is_none());
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// What this machine has, read through the worker; by hand only. Reads, writes nothing.
    #[test]
    #[ignore]
    fn probe() {
        let page = SoundPage::start(Arc::new(|| {}));
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            if let Some(p) = page.take() {
                println!("{p:#?}");
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("no reading");
    }
}

#[cfg(test)]
mod hands_on {
    use super::*;
    use crate::system::sound_dev::Flow;
    use crate::system::sound_format;
    use std::time::{Duration, Instant};

    fn wait(page: &SoundPage, ok: impl Fn(&Page) -> bool) -> Option<Page> {
        let until = Instant::now() + Duration::from_secs(4);
        while Instant::now() < until {
            if let Some(p) = page.take().filter(|p| ok(p)) {
                return Some(p);
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        None
    }

    /// Moves real things a little and puts each back: the microphone's volume by two points, one
    /// app session's volume by two points, then (only if nothing is playing) the default output's
    /// format to a PCM one and back. By hand only: `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn nudge_and_restore() {
        let page = SoundPage::start(Arc::new(|| {}));
        let first = wait(&page, |_| true).expect("a reading");
        // Microphone volume.
        let mic = first.input.selected_device().unwrap().clone();
        let to = if mic.volume > 0.5 { mic.volume - 0.02 } else { mic.volume + 0.02 };
        page.set_volume(&mic.id, to);
        let moved = wait(&page, |p| p.input.selected_device().is_some_and(|d| (d.volume - to).abs() < 0.005));
        page.set_volume(&mic.id, mic.volume);
        let back = wait(&page, |p| p.input.selected_device().is_some_and(|d| (d.volume - mic.volume).abs() < 0.005));
        println!("mic volume {} -> {to}: moved {}, back {}", mic.volume, moved.is_some(), back.is_some());
        // One session.
        if let Some(s) = first.sessions.iter().find(|s| !s.system) {
            let to = if s.volume > 0.5 { s.volume - 0.02 } else { s.volume + 0.02 };
            page.set_session_volume(&s.key, to);
            let moved = wait(&page, |p| p.sessions.iter().any(|x| x.key == s.key && (x.volume - to).abs() < 0.005));
            page.set_session_volume(&s.key, s.volume);
            let back = wait(&page, |p| p.sessions.iter().any(|x| x.key == s.key && (x.volume - s.volume).abs() < 0.005));
            println!("session {} {} -> {to}: moved {}, back {}", s.name, s.volume, moved.is_some(), back.is_some());
        }
        // Meter: runs, reads, stops.
        page.meter(Some(&mic.id));
        std::thread::sleep(Duration::from_millis(400));
        println!("meter peak {}", page.peak());
        page.meter(None);
        // Format.
        let out = first.output.selected_device().unwrap().clone();
        if first.sessions.iter().any(|s| s.active) {
            println!("format: skipped, a session is playing");
            return;
        }
        let was = first.output.detail.as_ref().unwrap().clone();
        let raw = sound_format::device_format(&out.id).unwrap();
        let alt = was.supported.iter().find(|f| Some(**f) != was.format && f.channels == 2 && f.bits == 16 && f.rate == 48_000).copied();
        let Some(alt) = alt else {
            println!("format: no alternative to try");
            return;
        };
        page.set_format(&out.id, alt);
        let changed = wait(&page, |p| p.output.detail.as_ref().is_some_and(|d| d.format == Some(alt)));
        println!("format -> {}: read back {}, can undo {}", alt.full_label(), changed.is_some(), changed.as_ref().is_some_and(|p| p.output.detail.as_ref().unwrap().can_undo));
        page.undo_format(&out.id);
        let restored = wait(&page, |p| p.output.detail.as_ref().is_some_and(|d| d.format == was.format));
        let same = sound_format::device_format(&out.id).map(|r| r.spec()) == Some(raw.spec());
        println!("format restored: {} (same as before: {same})", restored.is_some());
        assert!(restored.is_some() && same);
    }

    /// Makes another output the default for calls only, then puts it back; by hand only. The
    /// device apps play to is not touched.
    #[test]
    #[ignore]
    fn calls_role_and_back() {
        let page = SoundPage::start(Arc::new(|| {}));
        let first = wait(&page, |_| true).expect("a reading");
        let was = first.output.devices.iter().find(|d| d.default_comm).unwrap().clone();
        let other = first.output.devices.iter().find(|d| !d.default_comm).unwrap().clone();
        page.set_default(&other.id, Flow::Output, Role::Calls);
        let moved = wait(&page, |p| p.output.devices.iter().any(|d| d.id == other.id && d.default_comm));
        page.set_default(&was.id, Flow::Output, Role::Calls);
        let back = wait(&page, |p| p.output.devices.iter().any(|d| d.id == was.id && d.default_comm));
        let still = back.as_ref().is_some_and(|p| p.output.devices.iter().any(|d| d.id == was.id && d.default));
        println!("calls role to {}: {}, back: {}, console default kept: {still}", other.name, moved.is_some(), back.is_some());
        assert!(moved.is_some() && back.is_some() && still);
    }

    /// One short tone on the default output; by hand only.
    #[test]
    #[ignore]
    fn one_tone() {
        let page = SoundPage::start(Arc::new(|| {}));
        let first = wait(&page, |_| true).expect("a reading");
        // The default output here is a monitor that may not take a stream; prefer one that does.
        let out = first.output.devices.iter().find(|d| d.name.contains("Realtek")).or(first.output.selected_device()).unwrap().clone();
        page.meter(Some(&out.id));
        page.play_tone(&out.id);
        let mut top = 0.0f32;
        for _ in 0..24 {
            std::thread::sleep(Duration::from_millis(50));
            top = top.max(page.peak());
        }
        page.meter(None);
        println!("tone sent to {}; the output's peak while it played: {top}", out.name);
    }
}

#[cfg(test)]
mod idle {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    /// How often the worker wakes the UI while nothing happens; by hand only.
    #[test]
    #[ignore]
    fn wakes_while_idle() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = wakes.clone();
        let page = SoundPage::start(Arc::new(move || {
            w.fetch_add(1, Ordering::Relaxed);
        }));
        std::thread::sleep(Duration::from_secs(2));
        let settled = wakes.load(Ordering::Relaxed);
        std::thread::sleep(Duration::from_secs(3));
        println!("wakes: {settled} in the first 2 s, {} in the next 3 s", wakes.load(Ordering::Relaxed) - settled);
        drop(page);
    }
}
