//! Sound, followed live: a worker thread that owns the audio objects, takes commands, and hears
//! Windows' own change notifications (volume and mute from keys or other apps, devices plugged
//! in or out, a new default) instead of polling.
//!
//! Dropping the watch unregisters every callback and ends the thread.

use super::audio::{self, Sound};
use super::live::{self, Mailbox, Wake};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use windows::core::{implement, Result, PCWSTR};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{
    eCapture, eRender, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE,
};

enum Cmd {
    /// Something changed; read everything again.
    Refresh,
    /// The default device changed; attach to the new one, then read again.
    Reattach,
    Volume(f32),
    Mute(bool),
    MicMute(bool),
    Default(String),
    Stop,
}

pub struct SoundWatch {
    tx: Sender<Cmd>,
    mailbox: Arc<Mailbox<Sound>>,
}

impl SoundWatch {
    /// Start following the sound state; `wake` is called whenever a new reading is ready. The
    /// first reading follows at once.
    pub fn start(wake: Wake) -> Self {
        let (tx, rx) = channel();
        let mailbox = Arc::new(Mailbox::default());
        let (to_self, out) = (tx.clone(), mailbox.clone());
        live::spawn("sound-watch", move || run(rx, to_self, out, wake));
        let _ = tx.send(Cmd::Refresh);
        Self { tx, mailbox }
    }

    /// The newest reading since the last call.
    pub fn take(&self) -> Option<Sound> {
        self.mailbox.take()
    }

    pub fn set_volume(&self, level: f32) {
        let _ = self.tx.send(Cmd::Volume(level));
    }

    pub fn set_muted(&self, muted: bool) {
        let _ = self.tx.send(Cmd::Mute(muted));
    }

    pub fn set_mic_muted(&self, muted: bool) {
        let _ = self.tx.send(Cmd::MicMute(muted));
    }

    pub fn set_default_output(&self, id: String) {
        let _ = self.tx.send(Cmd::Default(id));
    }
}

impl Drop for SoundWatch {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

/// The endpoints the worker is attached to, with the callbacks it registered on them.
struct Attached {
    output: Option<IAudioEndpointVolume>,
    mic: Option<IAudioEndpointVolume>,
    callback: IAudioEndpointVolumeCallback,
}

impl Attached {
    fn new(en: &IMMDeviceEnumerator, callback: IAudioEndpointVolumeCallback) -> Self {
        let watch = |flow: EDataFlow| {
            let ep = audio::default_volume(en, flow)?;
            // SAFETY: valid interfaces; unregistered in `detach`.
            unsafe { ep.RegisterControlChangeNotify(&callback).ok()? };
            Some(ep)
        };
        Self {
            output: watch(eRender),
            mic: watch(eCapture),
            callback,
        }
    }

    fn detach(self) -> IAudioEndpointVolumeCallback {
        for ep in [self.output, self.mic].into_iter().flatten() {
            // SAFETY: the same callback that was registered.
            unsafe {
                let _ = ep.UnregisterControlChangeNotify(&self.callback);
            }
        }
        self.callback
    }
}

fn run(rx: Receiver<Cmd>, to_self: Sender<Cmd>, out: Arc<Mailbox<Sound>>, wake: Wake) {
    live::init_mta();
    let Some(en) = audio::enumerator() else { return };
    let devices: IMMNotificationClient = Devices { tx: to_self.clone() }.into();
    // SAFETY: valid interface; unregistered before the thread ends.
    let listening = unsafe { en.RegisterEndpointNotificationCallback(&devices).is_ok() };
    let mut attached = Attached::new(&en, Levels { tx: to_self }.into());
    while let Ok(first) = rx.recv() {
        // Take everything already waiting, so a slider drag or a burst of notifications is
        // handled once, with its last value.
        let mut batch = vec![first];
        batch.extend(rx.try_iter());
        if batch.iter().any(|c| matches!(c, Cmd::Stop)) {
            break;
        }
        if batch.iter().any(|c| matches!(c, Cmd::Reattach)) {
            let callback = attached.detach();
            attached = Attached::new(&en, callback);
        }
        let volume = batch.iter().rev().find_map(|c| match c {
            Cmd::Volume(v) => Some(*v),
            _ => None,
        });
        for cmd in &batch {
            match cmd {
                Cmd::Mute(m) => attached.output.iter().for_each(|ep| audio::write_mute(ep, *m)),
                Cmd::MicMute(m) => attached.mic.iter().for_each(|ep| audio::write_mute(ep, *m)),
                Cmd::Default(id) => {
                    // The default-changed notification re-attaches and reads again.
                    audio::set_default_output(id);
                }
                _ => {}
            }
        }
        if let (Some(v), Some(ep)) = (volume, &attached.output) {
            audio::write_level(ep, v);
        }
        out.put(audio::read_sound(&en));
        wake();
    }
    attached.detach();
    if listening {
        // SAFETY: the same client that was registered.
        unsafe {
            let _ = en.UnregisterEndpointNotificationCallback(&devices);
        }
    }
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

/// Hears devices coming, going, being renamed, and a new default.
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
        let _ = self.tx.send(Cmd::Refresh);
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

    fn OnPropertyValueChanged(&self, _id: &PCWSTR, _key: &PROPERTYKEY) -> Result<()> {
        // Fired often (levels, formats); names rarely change, so these are ignored.
        Ok(())
    }
}
