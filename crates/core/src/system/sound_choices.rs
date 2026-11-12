//! What the Format and Spatial sound controls of one output offer, decided from what Windows
//! reports. Pure, so it is tested without a device.
//!
//! Why this exists: a "home theater" spatial format (Dolby Atmos for home theater, DTS:X for home
//! theater) sends an encoded stream to an HDMI receiver, and Windows itself puts the endpoint on
//! the matching encoded device format (Dolby MAT, DTS) while it is on, and back on PCM when it is
//! turned off. The encoded format is therefore not a format to pick from the Format list: it
//! follows spatial sound. Picking a PCM format while it is on would knock the device out of
//! Dolby (and spatial sound with it), so the Format control is locked then.

use super::sound_format::FormatSpec;
use super::sound_spatial::Spatial;

/// What the Format control is.
#[derive(Debug, Clone, PartialEq)]
pub enum FormatControl {
    /// Windows reports no format.
    Unknown,
    /// An ordinary list to pick from.
    Pick,
    /// Shown, not changeable: spatial sound decides it. `by` is the spatial format's name, or
    /// "Off" when the device is encoded for a reason spatial sound does not give.
    Locked { by: String },
}

/// What the page shows for the Format and Spatial controls.
#[derive(Debug, Clone, PartialEq)]
pub struct Choices {
    pub format: FormatControl,
    /// A muted line under Spatial sound, when there is something to know.
    pub spatial_hint: Option<String>,
}

/// Whether a spatial format's name is one of the "home theater" ones that send an encoded stream.
pub fn is_home_theater(name: &str) -> bool {
    name.to_ascii_lowercase().contains("home theater")
}

/// The muted line under a locked format.
pub fn locked_note(by: &str) -> String {
    if by == "Off" {
        "An encoded format that Windows keeps; it changes with spatial sound".to_owned()
    } else {
        format!("Set by spatial sound ({by})")
    }
}

pub fn decide(format: Option<&FormatSpec>, spatial: Option<&Spatial>) -> Choices {
    let sp = spatial.filter(|s| s.supported);
    let active_ht = sp.filter(|s| !s.is_off()).map(Spatial::active_name).filter(|n| is_home_theater(n));
    let format_control = match (format, active_ht) {
        (None, _) => FormatControl::Unknown,
        // The device runs on spatial sound's encoded format, or should: leave it to spatial sound.
        (Some(_), Some(by)) => FormatControl::Locked { by },
        // Encoded, but spatial sound does not say why: still not to be knocked out by a PCM pick.
        (Some(f), None) if f.encoded => FormatControl::Locked { by: sp.map(Spatial::active_name).unwrap_or_else(|| "Off".into()) },
        (Some(_), None) => FormatControl::Pick,
    };
    let spatial_hint = match sp {
        Some(s) if s.is_off() => {
            let ht: Vec<&str> = s.formats.iter().map(|(_, n)| n.as_str()).filter(|n| is_home_theater(n)).collect();
            (!ht.is_empty()).then(|| format!("{} sends Dolby or DTS to the receiver, and sets the device's format for you.", ht.join(" or ")))
        }
        Some(_) if matches!(format_control, FormatControl::Locked { .. }) => {
            Some("Turning it off puts the device back on a plain PCM format.".to_owned())
        }
        _ => None,
    };
    Choices { format: format_control, spatial_hint }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm() -> FormatSpec {
        FormatSpec { rate: 48_000, bits: 16, valid_bits: 16, channels: 8, mask: 0x63F, float: false, encoded: false, codec: 0 }
    }
    fn mat() -> FormatSpec {
        FormatSpec { rate: 192_000, bits: 16, valid_bits: 16, channels: 8, mask: 0x63F, float: false, encoded: true, codec: 0x0C }
    }
    const ATMOS_HT: &str = "{b53d940c-b846-4d90-a3f1-d1e5d2cc3a00}";
    const SONIC: &str = "{b3415d0c-4eb7-4d85-a8df-2b3a3f1f7b8e}";
    fn spatial(active: &str) -> Spatial {
        Spatial {
            supported: true,
            active: active.into(),
            default: active.into(),
            formats: vec![
                (SONIC.into(), "Windows Sonic for Headphones".into()),
                (ATMOS_HT.into(), "Dolby Atmos for home theater".into()),
            ],
        }
    }

    #[test]
    fn plain_pcm_device_without_spatial_sound_picks_freely() {
        let c = decide(Some(&pcm()), None);
        assert_eq!(c, Choices { format: FormatControl::Pick, spatial_hint: None });
        let none = Spatial::default();
        assert_eq!(decide(Some(&pcm()), Some(&none)), c);
    }

    #[test]
    fn headphone_spatial_sound_leaves_format_free_and_says_nothing() {
        let sp = Spatial { formats: vec![(SONIC.into(), "Windows Sonic for Headphones".into())], ..spatial(SONIC) };
        assert_eq!(decide(Some(&pcm()), Some(&sp)), Choices { format: FormatControl::Pick, spatial_hint: None });
        let off = Spatial { formats: sp.formats.clone(), ..spatial("") };
        assert_eq!(decide(Some(&pcm()), Some(&off)).spatial_hint, None);
    }

    #[test]
    fn encoded_device_with_home_theater_is_locked_to_it() {
        let c = decide(Some(&mat()), Some(&spatial(ATMOS_HT)));
        assert_eq!(c.format, FormatControl::Locked { by: "Dolby Atmos for home theater".into() });
        assert!(c.spatial_hint.unwrap().contains("PCM"));
        // Even a PCM reading while home theater is on offers no PCM pick.
        let c = decide(Some(&pcm()), Some(&spatial(ATMOS_HT)));
        assert!(matches!(c.format, FormatControl::Locked { .. }));
        assert_eq!(locked_note("Dolby Atmos for home theater"), "Set by spatial sound (Dolby Atmos for home theater)");
    }

    #[test]
    fn home_theater_available_but_off_gets_a_hint() {
        let c = decide(Some(&pcm()), Some(&spatial("")));
        assert_eq!(c.format, FormatControl::Pick);
        let hint = c.spatial_hint.unwrap();
        assert!(hint.contains("Dolby Atmos for home theater") && hint.contains("receiver"));
    }

    #[test]
    fn encoded_without_a_known_reason_is_still_not_pickable() {
        let c = decide(Some(&mat()), Some(&spatial("")));
        assert_eq!(c.format, FormatControl::Locked { by: "Off".into() });
        assert_eq!(decide(None, None).format, FormatControl::Unknown);
    }
}
