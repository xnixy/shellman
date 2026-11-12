//! Sound: the default output's volume and mute, the list of outputs and which one is the
//! default, and the microphone's mute. One-shot calls for the top bar; `audio_watch` follows
//! the same state live for the control centre.

use windows::core::{GUID, PCWSTR, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eMultimedia, eRender, EDataFlow, ERole, IMMDevice,
    IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_APARTMENTTHREADED, STGM_READ,
};

/// Master volume of the default playback device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Volume {
    /// 0.0..=1.0
    pub level: f32,
    pub muted: bool,
}

/// A playback device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// The endpoint id, which `set_default_output` takes.
    pub id: String,
    /// As Sound settings shows it, such as "Speakers (Realtek(R) Audio)".
    pub name: String,
}

/// Everything the sound module shows, read in one go.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sound {
    /// None without any playback device.
    pub volume: Option<Volume>,
    pub outputs: Vec<Output>,
    /// The id of the default output, among `outputs`.
    pub default_output: Option<String>,
    /// Whether the default microphone is muted; None without one.
    pub mic_muted: Option<bool>,
}

/// COM must be initialised on whichever thread calls us. S_FALSE (already initialised) and
/// RPC_E_CHANGED_MODE (thread already MTA) both still leave COM usable, so the result is ignored.
fn init_com() {
    // SAFETY: plain FFI call with no pointers.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

pub(crate) fn enumerator() -> Option<IMMDeviceEnumerator> {
    // SAFETY: COM call with valid arguments.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok() }
}

/// The volume control of the default device for `flow` (playback or recording).
pub(crate) fn default_volume(en: &IMMDeviceEnumerator, flow: EDataFlow) -> Option<IAudioEndpointVolume> {
    // SAFETY: COM calls with valid arguments; errors map to None (no such device).
    unsafe {
        let dev = en.GetDefaultAudioEndpoint(flow, eConsole).ok()?;
        dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
    }
}

/// Activated on every call instead of cached: the default device can change (headphones, HDMI)
/// and a cached interface would silently control a stale endpoint.
fn endpoint() -> Option<IAudioEndpointVolume> {
    init_com();
    default_volume(&enumerator()?, eRender)
}

pub(crate) fn read_volume(ep: &IAudioEndpointVolume) -> Option<Volume> {
    // SAFETY: valid interface.
    unsafe {
        let level = ep.GetMasterVolumeLevelScalar().ok()?.clamp(0.0, 1.0);
        let muted = ep.GetMute().ok()?.as_bool();
        Some(Volume { level, muted })
    }
}

pub(crate) fn write_level(ep: &IAudioEndpointVolume, level: f32) {
    if level.is_nan() {
        return;
    }
    // SAFETY: a null event context is allowed.
    unsafe {
        let _ = ep.SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), std::ptr::null::<GUID>());
    }
}

pub(crate) fn write_mute(ep: &IAudioEndpointVolume, muted: bool) {
    // SAFETY: a null event context is allowed.
    unsafe {
        let _ = ep.SetMute(muted, std::ptr::null::<GUID>());
    }
}

pub fn volume() -> Option<Volume> {
    read_volume(&endpoint()?)
}

/// Sets the master volume; `level` is clamped to 0..=1. Does nothing without an audio device.
pub fn set_volume(level: f32) {
    if let Some(ep) = endpoint() {
        write_level(&ep, level);
    }
}

pub fn toggle_mute() {
    if let Some(ep) = endpoint() {
        if let Some(v) = read_volume(&ep) {
            write_mute(&ep, !v.muted);
        }
    }
}

/// The volume, every active output, the default one and the microphone, as they are now.
pub(crate) fn read_sound(en: &IMMDeviceEnumerator) -> Sound {
    Sound {
        volume: default_volume(en, eRender).and_then(|ep| read_volume(&ep)),
        outputs: outputs(en),
        default_output: default_id(en, eRender),
        mic_muted: default_volume(en, eCapture).and_then(|ep| read_volume(&ep)).map(|v| v.muted),
    }
}

fn default_id(en: &IMMDeviceEnumerator, flow: EDataFlow) -> Option<String> {
    // SAFETY: COM call with valid arguments.
    unsafe { device_id(&en.GetDefaultAudioEndpoint(flow, eConsole).ok()?) }
}

unsafe fn device_id(dev: &IMMDevice) -> Option<String> {
    let id: PWSTR = dev.GetId().ok()?;
    let s = id.to_string().ok();
    CoTaskMemFree(Some(id.0 as *const _));
    s
}

/// Active playback devices, in the order Windows lists them.
fn outputs(en: &IMMDeviceEnumerator) -> Vec<Output> {
    let mut list = Vec::new();
    // SAFETY: COM calls with valid arguments; a device that fails to answer is skipped.
    unsafe {
        let Ok(all) = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else {
            return list;
        };
        for i in 0..all.GetCount().unwrap_or(0) {
            let Ok(dev) = all.Item(i) else { continue };
            let Some(id) = device_id(&dev) else { continue };
            let name = friendly_name(&dev).unwrap_or_else(|| "Audio device".into());
            list.push(Output { id, name });
        }
    }
    list
}

unsafe fn friendly_name(dev: &IMMDevice) -> Option<String> {
    let store = dev.OpenPropertyStore(STGM_READ).ok()?;
    let mut value = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
    let text = PropVariantToStringAlloc(&value).ok();
    let _ = PropVariantClear(&mut value);
    let text = text?;
    let s = text.to_string().ok();
    CoTaskMemFree(Some(text.0 as *const _));
    s.filter(|s| !s.is_empty())
}

// ---- Choosing the default output ----
//
// Windows has no public call for this. Sound settings, the quick settings flyout and every
// third-party switcher use the same internal interface, IPolicyConfig, unchanged since Windows 7
// and still there in Windows 11. Only `SetDefaultEndpoint` is called; the slots before it are
// declared so it lands in the right place in the table, with their real argument counts.

#[allow(non_snake_case)]
mod policy {
    use windows::core::{interface, IUnknown, IUnknown_Vtbl, HRESULT, PCWSTR};
    use windows::Win32::Media::Audio::ERole;

    #[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub unsafe trait IPolicyConfig: IUnknown {
        fn GetMixFormat(&self, id: PCWSTR, format: *mut *mut core::ffi::c_void) -> HRESULT;
        fn GetDeviceFormat(&self, id: PCWSTR, default: i32, format: *mut *mut core::ffi::c_void) -> HRESULT;
        fn ResetDeviceFormat(&self, id: PCWSTR) -> HRESULT;
        fn SetDeviceFormat(&self, id: PCWSTR, endpoint: *mut core::ffi::c_void, mix: *mut core::ffi::c_void) -> HRESULT;
        fn GetProcessingPeriod(&self, id: PCWSTR, default: i32, period: *mut i64, min: *mut i64) -> HRESULT;
        fn SetProcessingPeriod(&self, id: PCWSTR, period: *mut i64) -> HRESULT;
        fn GetShareMode(&self, id: PCWSTR, mode: *mut core::ffi::c_void) -> HRESULT;
        fn SetShareMode(&self, id: PCWSTR, mode: *mut core::ffi::c_void) -> HRESULT;
        fn GetPropertyValue(&self, id: PCWSTR, key: *const core::ffi::c_void, value: *mut core::ffi::c_void) -> HRESULT;
        fn SetPropertyValue(&self, id: PCWSTR, key: *const core::ffi::c_void, value: *mut core::ffi::c_void) -> HRESULT;
        fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT;
        fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT;
    }

    /// The macro keeps the methods private to this module.
    pub unsafe fn set_default(policy: &IPolicyConfig, id: PCWSTR, role: ERole) -> HRESULT {
        policy.SetDefaultEndpoint(id, role)
    }
}
use policy::IPolicyConfig;

/// CPolicyConfigClient, the object behind `IPolicyConfig`.
const POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

/// The roles a device is the default for. Sound settings sets all three at once, so apps that
/// ask for the "communications" device follow the choice too.
pub const ROLES: [ERole; 3] = [eConsole, eMultimedia, eCommunications];

/// Make the output `id` the default for every role. False when Windows refused.
pub fn set_default_output(id: &str) -> bool {
    init_com();
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    // SAFETY: the interface is created by its own class; `wide` outlives the calls.
    unsafe {
        let Ok(policy) = CoCreateInstance::<_, IPolicyConfig>(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)
        else {
            return false;
        };
        ROLES
            .iter()
            .all(|&role| policy::set_default(&policy, PCWSTR(wide.as_ptr()), role).is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_in_range() {
        if let Some(v) = volume() {
            assert!((0.0..=1.0).contains(&v.level));
        }
    }

    /// What this machine has; run with `--ignored --nocapture`. Reads only.
    #[test]
    #[ignore]
    fn probe() {
        init_com();
        println!("{:?}", read_sound(&enumerator().unwrap()));
    }
    #[test]
    fn every_role_is_set() {
        let mut roles: Vec<i32> = ROLES.iter().map(|r| r.0).collect();
        roles.sort();
        assert_eq!(roles, vec![0, 1, 2]);
    }

    #[test]
    fn the_default_output_is_listed() {
        init_com();
        let Some(en) = enumerator() else { return };
        let sound = read_sound(&en);
        if let Some(id) = &sound.default_output {
            assert!(sound.outputs.iter().any(|o| &o.id == id));
        }
    }
}
