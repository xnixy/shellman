//! The Sound page's worker: owns every COM object, takes commands, hears Windows' change
//! notifications (volumes, devices, formats, sessions), reads the state once per burst of them
//! and publishes it. While the input meter runs it also wakes every 50 ms to read the level;
//! otherwise it sleeps in a blocking receive and costs nothing.

use super::audio::{self, set_default};
use crate::elevate::{self, ops::Op};
use super::live::{self, Mailbox, Wake};
use super::sound_dev::{self as dev, Flow};
use super::sound_enable::{self, Next};
use super::sound_format::{self as format, FormatSpec};
use super::sound_page::{resolve_selected, Cmd, Detail, Page, Role, Section};
use super::sound_sessions::{Poke, Sessions};
use super::sound_spatial as spatial;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;
use windows::core::{implement, Result, PCWSTR};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
    IAudioMeterInformation,
};
use windows::Win32::Media::Audio::{
    eCommunications, eConsole, eMultimedia, eRender, EDataFlow, ERole, IMMDeviceEnumerator,
    IMMNotificationClient, IMMNotificationClient_Impl, AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE,
};

/// The meter is read this often: 20 times a second is smooth for a level bar.
const METER_EVERY: Duration = Duration::from_millis(50);

/// The device-format property sets: a change of format fires the notification, and the page reads
/// again. (PKEY_AudioEngine_DeviceFormat and the endpoint's format, as seen on a real device.)
const FORMAT_SETS: [u128; 2] = [
    0xf19f064d_082c_4e27_bc73_6882a1bb8e4c,
    0xe4870e26_3cc5_4cd2_ba46_ca0a9a70ed04,
];

/// The roles `role` sets.
pub fn roles_for(role: Role) -> &'static [ERole] {
    match role {
        Role::Everything => &[eConsole, eMultimedia],
        Role::Calls => &[eCommunications],
    }
}

struct Worker {
    en: IMMDeviceEnumerator,
    /// The user's choice per flow.
    chosen: HashMap<Flow, String>,
    /// What was probed for the chosen device, so a reading does not probe again.
    probed: HashMap<Flow, (String, FormatSpec, Vec<FormatSpec>)>,
    /// The format a device had before the page last changed it, exactly as Windows held it: the
    /// device format and the mix format. Lets any change be taken back, even from a format the
    /// page cannot build (an encoded stream).
    undo: HashMap<String, (format::Raw, format::Raw)>,
    /// Why the last spatial change / tone failed, per device.
    spatial_err: HashMap<String, String>,
    tone_err: HashMap<String, String>,
    /// Why the last turning on or off failed.
    enable_err: Option<String>,
    /// Hears each output's spatial state change.
    spatial_watch: Vec<spatial::Watch>,
    to_self: Sender<Cmd>,
    sessions: Sessions,
    meter: Option<IAudioMeterInformation>,
    attached: Vec<IAudioEndpointVolume>,
    callback: IAudioEndpointVolumeCallback,
}

pub(crate) fn run(rx: Receiver<Cmd>, to_self: Sender<Cmd>, out: Arc<Mailbox<Page>>, level: Arc<AtomicU32>, wake: Wake) {
    live::init_mta();
    let Some(en) = audio::enumerator() else { return };
    let devices: IMMNotificationClient = Devices { tx: to_self.clone() }.into();
    // SAFETY: valid interface; unregistered before the thread ends.
    let listening = unsafe { en.RegisterEndpointNotificationCallback(&devices).is_ok() };
    let poke: Poke = {
        let tx = std::sync::Mutex::new(to_self.clone());
        Arc::new(move || {
            if let Ok(tx) = tx.lock() {
                let _ = tx.send(Cmd::Refresh);
            }
        })
    };
    let mut w = Worker {
        en,
        chosen: HashMap::new(),
        probed: HashMap::new(),
        undo: HashMap::new(),
        spatial_err: HashMap::new(),
        tone_err: HashMap::new(),
        enable_err: None,
        spatial_watch: Vec::new(),
        to_self: to_self.clone(),
        sessions: Sessions::new(poke),
        meter: None,
        attached: Vec::new(),
        callback: Levels { tx: to_self }.into(),
    };
    w.reattach();
    let mut last_peak = 0.0f32;
    loop {
        let first = if w.meter.is_some() {
            match rx.recv_timeout(METER_EVERY) {
                Ok(c) => Some(c),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(c) => Some(c),
                Err(_) => break,
            }
        };
        if let Some(first) = first {
            let mut batch = vec![first];
            batch.extend(rx.try_iter());
            if batch.iter().any(|c| matches!(c, Cmd::Stop)) {
                break;
            }
            w.handle(batch);
            out.put(w.page());
            wake();
        }
        if let Some(m) = &w.meter {
            let now = dev::peak(m);
            // Wake only when the bar would move by a visible step (the page draws the square
            // root of the level), so the noise floor of a quiet microphone costs no frames.
            let step = |p: f32| (p.sqrt() * 60.0).round();
            if step(now) != step(last_peak) {
                level.store(now.to_bits(), Ordering::Relaxed);
                last_peak = now;
                wake();
            }
        } else if last_peak != 0.0 {
            level.store(0, Ordering::Relaxed);
            last_peak = 0.0;
        }
    }
    w.detach();
    if listening {
        // SAFETY: the same client that was registered.
        unsafe {
            let _ = w.en.UnregisterEndpointNotificationCallback(&devices);
        }
    }
}

impl Worker {
    /// Listen to every active device's volume and to the default output's sessions.
    fn reattach(&mut self) {
        self.detach();
        for flow in [Flow::Output, Flow::Input] {
            for d in dev::list(&self.en, flow) {
                let Some(ep) = dev::open(&self.en, &d.id).and_then(|x| dev::endpoint(&x)) else { continue };
                // SAFETY: valid interfaces; unregistered in `detach`.
                if unsafe { ep.RegisterControlChangeNotify(&self.callback).is_ok() } {
                    self.attached.push(ep);
                }
            }
        }
        self.spatial_watch.clear();
        for d in dev::list(&self.en, Flow::Output) {
            let tx = self.to_self.clone();
            if let Some(w) = spatial::watch(&d.id, move || {
                let _ = tx.send(Cmd::Refresh);
            }) {
                self.spatial_watch.push(w);
            }
        }
        // SAFETY: COM call with valid arguments.
        let default = unsafe { self.en.GetDefaultAudioEndpoint(eRender, eConsole).ok() };
        self.sessions.attach(default.as_ref());
    }

    fn detach(&mut self) {
        for ep in self.attached.drain(..) {
            // SAFETY: the same callback that was registered.
            unsafe {
                let _ = ep.UnregisterControlChangeNotify(&self.callback);
            }
        }
    }

    fn handle(&mut self, batch: Vec<Cmd>) {
        if batch.iter().any(|c| matches!(c, Cmd::Reattach)) {
            self.reattach();
        }
        // A drag sends many levels; only the last of each counts.
        let mut volumes: HashMap<String, f32> = HashMap::new();
        let mut balances: HashMap<String, f32> = HashMap::new();
        let mut session_volumes: HashMap<String, f32> = HashMap::new();
        for cmd in batch {
            match cmd {
                Cmd::Select(flow, id) => {
                    self.chosen.insert(flow, id);
                }
                Cmd::Volume(id, v) => {
                    volumes.insert(id, v);
                }
                Cmd::Balance(id, b) => {
                    balances.insert(id, b);
                }
                Cmd::SessionVolume(key, v) => {
                    session_volumes.insert(key, v);
                }
                Cmd::Mute(id, m) => {
                    if let Some(d) = dev::open(&self.en, &id) {
                        dev::set_muted(&d, m);
                    }
                }
                Cmd::SessionMute(key, m) => self.sessions.set_muted(&key, m),
                Cmd::Default(id, flow, role) => {
                    let _ = flow;
                    // The default-changed notification attaches again and reads.
                    set_default(&id, roles_for(role));
                }
                Cmd::Format(id, spec) => {
                    if let (Some(d), Some(m)) = (format::device_format(&id), format::mix_format(&id)) {
                        // An encoded format follows spatial sound; a PCM pick would knock the
                        // device out of Dolby/DTS. The page does not offer it; this is the guard.
                        let encoded = d.spec().is_some_and(|s| s.encoded);
                        if !encoded && format::set_device_format(&id, &spec) {
                            self.undo.insert(id, (d, m));
                        }
                    }
                    self.probed.clear();
                }
                Cmd::UndoFormat(id) => {
                    if let Some((d, m)) = self.undo.remove(&id) {
                        format::restore(&id, &d, &m);
                    }
                    self.probed.clear();
                }
                Cmd::Tone(id) => {
                    self.tone_err.remove(&id);
                    tone_later(id, self.to_self.clone());
                }
                Cmd::ToneDone(id, ok) => {
                    if ok {
                        self.tone_err.remove(&id);
                    } else {
                        self.tone_err.insert(id, "The test sound could not start. A device set to an encoded format (such as Dolby) cannot play it.".into());
                    }
                }
                Cmd::Enable(id, on) => {
                    self.enable_err = None;
                    enable_later(id, on, self.to_self.clone());
                }
                Cmd::EnableDone(err) => self.enable_err = err,
                Cmd::Spatial(id, subtype) => {
                    self.spatial_err.remove(&id);
                    spatial_later(id, subtype, self.to_self.clone());
                }
                Cmd::SpatialDone(id, r) => match r {
                    Ok(()) => {
                        self.spatial_err.remove(&id);
                    }
                    Err(e) => {
                        self.spatial_err.insert(id, e);
                    }
                },
                Cmd::Meter(id) => {
                    self.meter = id.and_then(|id| dev::open(&self.en, &id)).and_then(|d| dev::meter(&d));
                }
                Cmd::Refresh | Cmd::Reattach | Cmd::Stop => {}
            }
        }
        for (id, v) in volumes {
            if let Some(d) = dev::open(&self.en, &id) {
                dev::set_volume(&d, v);
            }
        }
        for (id, b) in balances {
            if let Some(d) = dev::open(&self.en, &id) {
                dev::set_balance(&d, b);
            }
        }
        for (key, v) in session_volumes {
            self.sessions.set_volume(&key, v);
        }
    }

    fn page(&mut self) -> Page {
        Page {
            output: self.section(Flow::Output),
            input: self.section(Flow::Input),
            sessions: self.sessions.read(),
            notice: self.enable_err.clone(),
        }
    }

    fn section(&mut self, flow: Flow) -> Section {
        let devices = dev::list(&self.en, flow);
        let selected = resolve_selected(&devices, self.chosen.get(&flow).map(String::as_str));
        let detail = selected.as_deref().and_then(|id| self.detail(flow, id));
        let disabled = dev::list_disabled(&self.en, flow);
        Section { devices, disabled, selected, detail }
    }

    fn detail(&mut self, flow: Flow, id: &str) -> Option<Detail> {
        let d = dev::open(&self.en, id)?;
        let current = format::device_format(id).and_then(|r| r.spec());
        let supported = match current {
            Some(cur) => match self.probed.get(&flow) {
                Some((pid, pcur, list)) if pid == id && *pcur == cur => list.clone(),
                _ => {
                    let list = format::probe(&d, &cur);
                    self.probed.insert(flow, (id.to_owned(), cur, list.clone()));
                    list
                }
            },
            None => Vec::new(),
        };
        let output = flow == Flow::Output;
        Some(Detail {
            balance: if output { dev::balance(&d) } else { None },
            format: current,
            can_undo: self.undo.contains_key(id),
            supported,
            spatial: if output { spatial::read(id) } else { None },
            spatial_error: self.spatial_err.get(id).cloned(),
            tone_error: self.tone_err.get(id).cloned(),
        })
    }
}

/// Play the test tone from a thread of its own, one at a time: it takes its length to play, and
/// the worker must stay free to hear the volume move meanwhile.
fn tone_later(id: String, tx: Sender<Cmd>) {
    static BUSY: AtomicBool = AtomicBool::new(false);
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    live::spawn("sound-tone", move || {
        live::init_mta();
        let ok = match audio::enumerator().and_then(|en| dev::open(&en, &id)) {
            Some(d) => dev::play_tone(&d),
            None => false,
        };
        BUSY.store(false, Ordering::SeqCst);
        let _ = tx.send(Cmd::ToneDone(id, ok));
    });
}

/// Turn a device on or off from a thread of its own: directly first, and through the elevated
/// helper (which shows the UAC prompt and is waited for) only when Windows says access denied.
fn enable_later(id: String, on: bool, tx: Sender<Cmd>) {
    live::spawn("sound-enable", move || {
        live::init_mta();
        let err = match sound_enable::next_step(sound_enable::set_enabled(&id, on)) {
            Next::Done => None,
            Next::Fail(why) => Some(why),
            Next::Elevate => match Op::sound_device(on, &id) {
                Some(op) => elevate::run(&op).message(),
                None => Some("Windows gave this device an id the helper does not accept.".into()),
            },
        };
        // The device-state notification refreshes the page; this carries any message.
        let _ = tx.send(Cmd::EnableDone(err));
    });
}

/// Ask Windows for a spatial format from a thread of its own (the answer is awaited), and tell the
/// worker how it went; the change itself reaches the page through ConfigurationChanged.
fn spatial_later(id: String, subtype: String, tx: Sender<Cmd>) {
    live::spawn("sound-spatial", move || {
        live::init_mta();
        let r = spatial::set(&id, &subtype);
        let _ = tx.send(Cmd::SpatialDone(id, r));
    });
}

/// Hears volume and mute changes on the attached endpoints.
#[implement(IAudioEndpointVolumeCallback)]
struct Levels {
    tx: Sender<Cmd>,
}

impl IAudioEndpointVolumeCallback_Impl for Levels_Impl {
    fn OnNotify(&self, _data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<()> {
        let _ = self.tx.send(Cmd::Refresh);
        Ok(())
    }
}

/// Hears devices coming, going, a new default, and a device's format changing.
#[implement(IMMNotificationClient)]
struct Devices {
    tx: Sender<Cmd>,
}

impl IMMNotificationClient_Impl for Devices_Impl {
    fn OnDeviceStateChanged(&self, _id: &PCWSTR, _state: DEVICE_STATE) -> Result<()> {
        let _ = self.tx.send(Cmd::Reattach);
        Ok(())
    }

    fn OnDeviceAdded(&self, _id: &PCWSTR) -> Result<()> {
        let _ = self.tx.send(Cmd::Reattach);
        Ok(())
    }

    fn OnDeviceRemoved(&self, _id: &PCWSTR) -> Result<()> {
        let _ = self.tx.send(Cmd::Reattach);
        Ok(())
    }

    fn OnDefaultDeviceChanged(&self, _flow: EDataFlow, _role: ERole, _id: &PCWSTR) -> Result<()> {
        let _ = self.tx.send(Cmd::Reattach);
        Ok(())
    }

    fn OnPropertyValueChanged(&self, _id: &PCWSTR, key: &PROPERTYKEY) -> Result<()> {
        // Fired often for things the page does not show; only a format change matters.
        if FORMAT_SETS.contains(&key.fmtid.to_u128()) {
            let _ = self.tx.send(Cmd::Refresh);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_are_set_apart_from_everything_else() {
        assert_eq!(roles_for(Role::Calls), &[eCommunications]);
        assert_eq!(roles_for(Role::Everything), &[eConsole, eMultimedia]);
    }
}
