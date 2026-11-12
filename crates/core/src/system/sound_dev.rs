//! The devices of the Sound page: every active output and microphone with its kind, volume and
//! mute, the default for each role, balance, the input level meter and a test tone. Everything
//! here is documented Core Audio (IMMDevice, IAudioEndpointVolume, IAudioMeterInformation,
//! IAudioClient in shared mode) except choosing a default, which is `audio::set_default`.

use super::audio::{read_volume, write_level, write_mute};
use windows::core::{GUID, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eRender, EDataFlow, IAudioClient, IAudioRenderClient,
    IMMDevice, IMMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED, DEVICE_STATE_ACTIVE, DEVICE_STATE_DISABLED, WAVEFORMATEX,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc, PropVariantToUInt32};
use windows::Win32::System::Com::{CoTaskMemFree, CLSCTX_ALL, STGM_READ};

/// Output or input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flow {
    Output,
    Input,
}

impl Flow {
    pub fn data_flow(self) -> EDataFlow {
        match self {
            Flow::Output => eRender,
            Flow::Input => eCapture,
        }
    }
}

/// What a device is, from its form factor, for the icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Speakers,
    Headphones,
    Headset,
    Microphone,
    Display,
    Digital,
    Line,
    Other,
}

impl Kind {
    /// From PKEY_AudioEndpoint_FormFactor (the EndpointFormFactor values).
    pub fn from_form_factor(v: u32, flow: Flow) -> Self {
        match v {
            1 => Kind::Speakers,
            2 => Kind::Line,
            3 => Kind::Headphones,
            4 => Kind::Microphone,
            5 | 6 => Kind::Headset,
            7 | 8 => Kind::Digital,
            9 => Kind::Display,
            _ if flow == Flow::Input => Kind::Microphone,
            _ => Kind::Other,
        }
    }
}

/// An active device and its controls, as they are now.
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    /// Default for the console and multimedia roles.
    pub default: bool,
    /// Default for the communications role.
    pub default_comm: bool,
    pub volume: f32,
    pub muted: bool,
}

/// FormFactor of an endpoint (PKEY_AudioEndpoint_FormFactor).
const FORM_FACTOR: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e),
    pid: 0,
};

pub(crate) unsafe fn device_id(dev: &IMMDevice) -> Option<String> {
    let id: PWSTR = dev.GetId().ok()?;
    let s = id.to_string().ok();
    CoTaskMemFree(Some(id.0 as *const _));
    s
}

unsafe fn name_and_kind(dev: &IMMDevice, flow: Flow) -> (String, Kind) {
    let Ok(store) = dev.OpenPropertyStore(STGM_READ) else {
        return ("Audio device".into(), Kind::Other);
    };
    let name = store.GetValue(&PKEY_Device_FriendlyName).ok().and_then(|mut v| {
        let text = PropVariantToStringAlloc(&v).ok();
        let _ = PropVariantClear(&mut v);
        let text = text?;
        let s = text.to_string().ok();
        CoTaskMemFree(Some(text.0 as *const _));
        s.filter(|s| !s.is_empty())
    });
    let form = store.GetValue(&FORM_FACTOR).ok().and_then(|mut v| {
        let n = PropVariantToUInt32(&v).ok();
        let _ = PropVariantClear(&mut v);
        n
    });
    (
        name.unwrap_or_else(|| "Audio device".into()),
        Kind::from_form_factor(form.unwrap_or(10), flow),
    )
}

/// The active devices of `flow`, in the order Windows lists them.
pub fn list(en: &IMMDeviceEnumerator, flow: Flow) -> Vec<Device> {
    let mut out = Vec::new();
    // SAFETY: COM calls with valid arguments; a device that fails to answer is skipped.
    unsafe {
        let default_of = |role| en.GetDefaultAudioEndpoint(flow.data_flow(), role).ok().and_then(|d| device_id(&d));
        let (console, comm) = (default_of(eConsole), default_of(eCommunications));
        let Ok(all) = en.EnumAudioEndpoints(flow.data_flow(), DEVICE_STATE_ACTIVE) else {
            return out;
        };
        for i in 0..all.GetCount().unwrap_or(0) {
            let Ok(dev) = all.Item(i) else { continue };
            let Some(id) = device_id(&dev) else { continue };
            let (name, kind) = name_and_kind(&dev, flow);
            let vol = dev
                .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
                .ok()
                .and_then(|ep| read_volume(&ep));
            out.push(Device {
                default: console.as_deref() == Some(id.as_str()),
                default_comm: comm.as_deref() == Some(id.as_str()),
                id,
                name,
                kind,
                volume: vol.map_or(0.0, |v| v.level),
                muted: vol.is_some_and(|v| v.muted),
            });
        }
    }
    out
}

/// The devices of `flow` that are turned off, as they have no volume or default to show.
pub fn list_disabled(en: &IMMDeviceEnumerator, flow: Flow) -> Vec<Device> {
    let mut out = Vec::new();
    // SAFETY: COM calls with valid arguments; a device that fails to answer is skipped.
    unsafe {
        let Ok(all) = en.EnumAudioEndpoints(flow.data_flow(), DEVICE_STATE_DISABLED) else {
            return out;
        };
        for i in 0..all.GetCount().unwrap_or(0) {
            let Ok(dev) = all.Item(i) else { continue };
            let Some(id) = device_id(&dev) else { continue };
            let (name, kind) = name_and_kind(&dev, flow);
            out.push(Device { id, name, kind, default: false, default_comm: false, volume: 0.0, muted: false });
        }
    }
    out
}

/// The device with this id, if it is still there.
pub fn open(en: &IMMDeviceEnumerator, id: &str) -> Option<IMMDevice> {
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    // SAFETY: `wide` outlives the call.
    unsafe { en.GetDevice(windows::core::PCWSTR(wide.as_ptr())).ok() }
}

pub fn endpoint(dev: &IMMDevice) -> Option<IAudioEndpointVolume> {
    // SAFETY: valid interface.
    unsafe { dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok() }
}

pub fn set_volume(dev: &IMMDevice, level: f32) {
    if let Some(ep) = endpoint(dev) {
        write_level(&ep, level);
    }
}

pub fn set_muted(dev: &IMMDevice, muted: bool) {
    if let Some(ep) = endpoint(dev) {
        write_mute(&ep, muted);
    }
}

// ---- The classic applet ----

/// Open the classic Sound applet (`mmsys.cpl`) on its Playback (0) or Recording (1) tab. Runs
/// `rundll32 shell32.dll,Control_RunDLL`, which opens it without Explorer, with no console.
pub fn open_classic(tab: u8) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new("rundll32.exe")
        .args(["shell32.dll,Control_RunDLL", &format!("mmsys.cpl,,{tab}")])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

// ---- Balance ----

/// Balance as -1 (all left) ..= 1 (all right) from the two channel volumes; 0 when even.
pub fn balance_of(left: f32, right: f32) -> f32 {
    if left >= right {
        if left <= 0.0 { 0.0 } else { -(1.0 - right / left) }
    } else {
        1.0 - left / right
    }
}

/// The two channel volumes for a balance: the far side is turned down, the near one stays full.
pub fn channels_for(balance: f32) -> (f32, f32) {
    let b = balance.clamp(-1.0, 1.0);
    (1.0 - b.max(0.0), 1.0 + b.min(0.0))
}

/// The balance of a device with at least two channels.
pub fn balance(dev: &IMMDevice) -> Option<f32> {
    let ep = endpoint(dev)?;
    // SAFETY: valid interface.
    unsafe {
        if ep.GetChannelCount().ok()? < 2 {
            return None;
        }
        Some(balance_of(ep.GetChannelVolumeLevelScalar(0).ok()?, ep.GetChannelVolumeLevelScalar(1).ok()?))
    }
}

pub fn set_balance(dev: &IMMDevice, balance: f32) {
    let Some(ep) = endpoint(dev) else { return };
    let (l, r) = channels_for(balance);
    // SAFETY: valid interface; a null event context is allowed.
    unsafe {
        let _ = ep.SetChannelVolumeLevelScalar(0, l, std::ptr::null());
        let _ = ep.SetChannelVolumeLevelScalar(1, r, std::ptr::null());
    }
}

// ---- Level meter ----

/// The peak level now, 0..=1, of a device (what a microphone hears, or what a speaker plays).
pub fn meter(dev: &IMMDevice) -> Option<IAudioMeterInformation> {
    // SAFETY: valid interface.
    unsafe { dev.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None).ok() }
}

pub fn peak(m: &IAudioMeterInformation) -> f32 {
    // SAFETY: valid interface.
    unsafe { m.GetPeakValue().unwrap_or(0.0).clamp(0.0, 1.0) }
}

/// Whether anything is playing on any output: the default output's peak is above silence.
pub fn is_playing(en: &IMMDeviceEnumerator) -> bool {
    // SAFETY: COM calls with valid arguments.
    unsafe {
        en.GetDefaultAudioEndpoint(eRender, eConsole)
            .ok()
            .and_then(|d| meter(&d))
            .is_some_and(|m| peak(&m) > 0.0005)
    }
}

// ---- Test tone ----

/// A sine of `ms` milliseconds at `hz` as interleaved f32 frames, faded in and out so it does
/// not click. `amp` is its peak, 0..=1.
pub fn tone(rate: u32, channels: usize, hz: f32, ms: u32, amp: f32) -> Vec<f32> {
    let frames = (u64::from(rate) * u64::from(ms) / 1000) as usize;
    let fade = (frames / 8).max(1);
    let mut out = Vec::with_capacity(frames * channels);
    for n in 0..frames {
        let edge = n.min(frames - 1 - n);
        let env = (edge as f32 / fade as f32).min(1.0);
        let s = (std::f32::consts::TAU * hz * n as f32 / rate as f32).sin() * amp * env;
        out.extend(std::iter::repeat_n(s, channels));
    }
    out
}

/// Play a short quiet tone on the output `dev` through the shared-mode mixer. Blocks for the
/// tone's length, so call it from a thread of its own. False when the device would not play.
pub fn play_tone(dev: &IMMDevice) -> bool {
    // SAFETY: COM calls with valid arguments; the mix format is freed after use and the buffer
    // is only written within the frames the client handed out.
    unsafe {
        let Ok(client) = dev.Activate::<IAudioClient>(CLSCTX_ALL, None) else { return false };
        let Ok(mix) = client.GetMixFormat() else { return false };
        let (rate, channels, bits, tag) = {
            let m: &WAVEFORMATEX = &*mix;
            (m.nSamplesPerSec, usize::from(m.nChannels), { m.wBitsPerSample }, { m.wFormatTag })
        };
        // The shared mix format is float, which is all the tone is written in.
        let float = bits == 32 && (tag == 3 || tag == 0xFFFE);
        let ok = float && play(&client, mix, rate, channels);
        CoTaskMemFree(Some(mix as *const _));
        ok
    }
}

unsafe fn play(client: &IAudioClient, mix: *mut WAVEFORMATEX, rate: u32, channels: usize) -> bool {
    // 200 ms of buffer; the call takes 100 ns units.
    if client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 2_000_000, 0, mix, None).is_err() {
        return false;
    }
    let Ok(frames) = client.GetBufferSize() else { return false };
    let Ok(render) = client.GetService::<IAudioRenderClient>() else { return false };
    let samples = tone(rate, channels, 440.0, 400, 0.06);
    let total = samples.len() / channels;
    let mut sent = 0;
    if client.Start().is_err() {
        return false;
    }
    while sent < total {
        let used = client.GetCurrentPadding().unwrap_or(frames);
        let room = (frames - used) as usize;
        let n = room.min(total - sent);
        if n > 0 {
            let Ok(buf) = render.GetBuffer(n as u32) else { break };
            let src = &samples[sent * channels..(sent + n) * channels];
            std::ptr::copy_nonoverlapping(src.as_ptr(), buf as *mut f32, src.len());
            let _ = render.ReleaseBuffer(n as u32, 0);
            sent += n;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // Let the last frames play before the stream stops.
    std::thread::sleep(std::time::Duration::from_millis(120));
    let _ = client.Stop();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tone on the default output, with the result; by hand only.
    #[test]
    #[ignore]
    fn tone_on_the_default_output() {
        crate::system::live::init_mta();
        let en = crate::system::audio::enumerator().unwrap();
        // The default first, then the others until one plays (each plays at most once).
        let mut all = list(&en, Flow::Output);
        all.sort_by_key(|d| !d.default);
        for d in all {
            let dev = open(&en, &d.id).unwrap();
            let ok = play_tone(&dev);
            println!("played on {}: {ok}", d.name);
            if ok {
                break;
            }
        }
    }

    #[test]
    fn form_factors_pick_the_kind() {
        assert_eq!(Kind::from_form_factor(1, Flow::Output), Kind::Speakers);
        assert_eq!(Kind::from_form_factor(3, Flow::Output), Kind::Headphones);
        assert_eq!(Kind::from_form_factor(9, Flow::Output), Kind::Display);
        assert_eq!(Kind::from_form_factor(10, Flow::Input), Kind::Microphone);
        assert_eq!(Kind::from_form_factor(10, Flow::Output), Kind::Other);
    }

    #[test]
    fn balance_and_channels_agree() {
        assert_eq!(channels_for(0.0), (1.0, 1.0));
        assert_eq!(channels_for(-1.0), (1.0, 0.0));
        assert_eq!(channels_for(0.5), (0.5, 1.0));
        for b in [-1.0f32, -0.5, 0.0, 0.25, 1.0] {
            let (l, r) = channels_for(b);
            assert!((balance_of(l, r) - b).abs() < 1e-6, "{b}");
        }
        assert_eq!(balance_of(0.0, 0.0), 0.0);
    }

    #[test]
    fn the_tone_is_short_quiet_and_fades() {
        let t = tone(48_000, 2, 440.0, 400, 0.06);
        assert_eq!(t.len(), 48_000 * 400 / 1000 * 2);
        assert!(t.iter().all(|s| s.abs() <= 0.08 + 1e-6));
        assert!(t[0].abs() < 1e-3 && t[t.len() - 1].abs() < 0.01, "fades in and out");
        assert!(t.iter().any(|s| s.abs() > 0.05), "it is audible at its middle");
        assert_eq!(t[0], t[1], "every channel carries the same");
    }
}
