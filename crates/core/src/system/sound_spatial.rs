//! Spatial sound of an output device through `Windows.Media.Audio.SpatialAudioDeviceConfiguration`
//! (documented: learn.microsoft.com/uwp/api/windows.media.audio.spatialaudiodeviceconfiguration).
//! The pure parts (names, the list of choices, the device-interface id) are tested; the calls
//! need an initialised (MTA) thread and are made from the sound worker.

use windows::core::HSTRING;
use windows::Foundation::TypedEventHandler;
use windows::Media::Audio::{
    SetDefaultSpatialAudioFormatStatus as Status, SpatialAudioDeviceConfiguration as Config,
    SpatialAudioFormatSubtype as Subtype,
};

/// What a device offers for spatial sound, as shown.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Spatial {
    /// Whether the device can do any spatial format at all.
    pub supported: bool,
    /// The subtype that is on; empty for off. Windows may have changed it from the default.
    pub active: String,
    /// What the user chose.
    pub default: String,
    /// The formats it takes right now (Off is always offered besides these): subtype and name.
    pub formats: Vec<(String, String)>,
}

/// An empty subtype or the all-zero GUID means spatial sound is off.
pub fn is_off_subtype(s: &str) -> bool {
    s.chars().all(|c| matches!(c, '0' | '-' | '{' | '}'))
}

impl Spatial {
    pub fn is_off(&self) -> bool {
        is_off_subtype(&self.active)
    }

    /// The name of the active format; an unknown one is shown as "Other (...)".
    pub fn active_name(&self) -> String {
        if self.is_off() {
            return "Off".into();
        }
        match self.formats.iter().find(|(id, _)| same(id, &self.active)) {
            Some((_, n)) => n.clone(),
            None => name_of(&known(), &self.active),
        }
    }
}

/// Subtypes are GUID strings; compare them without case.
fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// (subtype, friendly name) for every format Windows documents.
pub fn known() -> Vec<(String, &'static str)> {
    let list: [(windows::core::Result<HSTRING>, &'static str); 7] = [
        (Subtype::WindowsSonic(), "Windows Sonic for Headphones"),
        (Subtype::DolbyAtmosForHeadphones(), "Dolby Atmos for Headphones"),
        (Subtype::DolbyAtmosForHomeTheater(), "Dolby Atmos for home theater"),
        (Subtype::DolbyAtmosForSpeakers(), "Dolby Atmos for speakers"),
        (Subtype::DTSHeadphoneX(), "DTS Headphone:X"),
        (Subtype::DTSXUltra(), "DTS:X Ultra"),
        (Subtype::DTSXForHomeTheater(), "DTS:X for home theater"),
    ];
    list.into_iter().filter_map(|(s, n)| s.ok().map(|s| (s.to_string(), n))).collect()
}

/// "Off" for empty, the friendly name for a known subtype, else "Other (subtype)".
pub fn name_of(known: &[(String, &'static str)], subtype: &str) -> String {
    if is_off_subtype(subtype) {
        return "Off".into();
    }
    match known.iter().find(|(id, _)| same(id, subtype)) {
        Some((_, n)) => (*n).to_owned(),
        None => format!("Other ({subtype})"),
    }
}

/// The known formats `supports` accepts, with their names, in the documented order.
pub fn build_list(known: &[(String, &'static str)], supports: impl Fn(&str) -> bool) -> Vec<(String, String)> {
    known.iter().filter(|(id, _)| supports(id)).map(|(id, n)| (id.clone(), (*n).to_owned())).collect()
}

/// The WinRT device-interface id of an MMDevice endpoint id (`{0.0.0.00000000}.{guid}` for an
/// output). None for anything that is not an output endpoint id.
pub fn winrt_id(endpoint: &str) -> Option<String> {
    let rest = endpoint.strip_prefix("{0.0.0.00000000}.")?;
    Some(format!(r"\\?\SWD#MMDEVAPI#{{0.0.0.00000000}}.{rest}#{{e6327cad-dcec-4949-ae8a-991e976a79d2}}"))
}

pub fn config(endpoint: &str) -> Option<Config> {
    Config::GetForDeviceId(&HSTRING::from(winrt_id(endpoint)?)).ok()
}

/// Read the device's spatial state. None when the device has no configuration to ask.
pub fn read(endpoint: &str) -> Option<Spatial> {
    let cfg = config(endpoint)?;
    if !cfg.IsSpatialAudioSupported().ok()? {
        return Some(Spatial::default());
    }
    let active = cfg.ActiveSpatialAudioFormat().map(|s| s.to_string()).unwrap_or_default();
    let default = cfg.DefaultSpatialAudioFormat().map(|s| s.to_string()).unwrap_or_default();
    let formats = build_list(&known(), |id| cfg.IsSpatialAudioFormatSupported(&HSTRING::from(id)).unwrap_or(false));
    Some(Spatial { supported: true, active, default, formats })
}

/// Ask for `subtype` (empty for off), waiting for the answer: call from a thread of its own.
/// Err carries a sentence for the user.
pub fn set(endpoint: &str, subtype: &str) -> Result<(), String> {
    let cfg = config(endpoint).ok_or("Windows would not open this device's spatial settings.")?;
    let status = cfg
        .SetDefaultSpatialAudioFormatAsync(&HSTRING::from(subtype))
        .and_then(|op| op.get())
        .and_then(|r| r.Status())
        .map_err(|e| format!("Windows did not take the change ({e})."))?;
    match status {
        Status::Succeeded => Ok(()),
        Status::AccessDenied => Err("Windows says only the app that owns this format may select it.".into()),
        Status::LicenseExpired => Err("The license for this format has expired.".into()),
        Status::LicenseNotValidForAudioEndpoint => Err("This format is not licensed for this device.".into()),
        Status::NotSupportedOnAudioEndpoint => Err("This device does not support that format.".into()),
        _ => Err("Windows could not change the spatial format.".into()),
    }
}

/// Call `on_change` whenever the device's spatial state changes. Keep the returned guard.
pub fn watch(endpoint: &str, on_change: impl Fn() + Send + 'static) -> Option<Watch> {
    let cfg = config(endpoint)?;
    let on_change = std::sync::Mutex::new(on_change);
    let handler = TypedEventHandler::<Config, windows::core::IInspectable>::new(move |_, _| {
        if let Ok(f) = on_change.lock() {
            f();
        }
        Ok(())
    });
    let token = cfg.ConfigurationChanged(&handler).ok()?;
    Some(Watch { cfg, token })
}

pub struct Watch {
    cfg: Config,
    token: i64,
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.cfg.RemoveConfigurationChanged(self.token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Vec<(String, &'static str)> {
        vec![("{a}".into(), "Alpha"), ("{b}".into(), "Beta")]
    }

    #[test]
    fn names() {
        assert_eq!(name_of(&table(), ""), "Off");
        assert_eq!(name_of(&table(), "{00000000-0000-0000-0000-000000000000}"), "Off");
        assert_eq!(name_of(&table(), "{A}"), "Alpha");
        assert_eq!(name_of(&table(), "{zz}"), "Other ({zz})");
    }

    #[test]
    fn list_keeps_only_supported_in_order() {
        let l = build_list(&table(), |id| id == "{b}");
        assert_eq!(l, vec![("{b}".to_string(), "Beta".to_string())]);
        assert!(build_list(&table(), |_| false).is_empty());
    }

    #[test]
    fn id_conversion() {
        assert_eq!(
            winrt_id("{0.0.0.00000000}.{1234}").as_deref(),
            Some(r"\\?\SWD#MMDEVAPI#{0.0.0.00000000}.{1234}#{e6327cad-dcec-4949-ae8a-991e976a79d2}")
        );
        assert_eq!(winrt_id("{0.0.1.00000000}.{1234}"), None);
    }

    #[test]
    fn active_name_falls_back() {
        let s = Spatial { supported: true, active: "{q}".into(), default: String::new(), formats: vec![] };
        assert_eq!(s.active_name(), "Other ({q})");
        let s = Spatial { active: "{B}".into(), formats: vec![("{b}".into(), "Beta".into())], ..s };
        assert_eq!(s.active_name(), "Beta");
    }

    /// Hands-on: switches the Realtek USB output (never the default) to Windows Sonic and back,
    /// reading each result. `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_switch_and_restore() {
        crate::system::live::init_mta();
        let id = "{0.0.0.00000000}.{435f75b4-623a-426a-9aee-4ae895b00532}";
        let before = read(id).unwrap();
        println!("before: {}", before.active);
        let sonic = known()[0].0.clone();
        println!("set sonic: {:?}", set(id, &sonic));
        std::thread::sleep(std::time::Duration::from_millis(800));
        let mid = read(id).unwrap();
        println!("after: {} ({})", mid.active, mid.active_name());
        println!("restore: {:?}", set(id, &before.active));
        std::thread::sleep(std::time::Duration::from_millis(800));
        let end = read(id).unwrap();
        println!("end: {}", end.active);
        assert_eq!(end.active, before.active);
        assert_eq!(end.default, before.default);
    }

    /// Hands-on, read only: what every output device reports. `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_read() {
        use windows::Media::Devices::{AudioDeviceRole, MediaDevice};
        crate::system::live::init_mta();
        println!("known: {:?}", known());
        let en = crate::system::audio::enumerator().unwrap();
        for d in crate::system::sound_dev::list(&en, crate::system::sound_dev::Flow::Output) {
            println!("{} | {} | {:?} | {:?}", d.name, d.id, winrt_id(&d.id), read(&d.id));
        }
        println!("MediaDevice default render id: {:?}", MediaDevice::GetDefaultAudioRenderId(AudioDeviceRole::Default));
    }
}
