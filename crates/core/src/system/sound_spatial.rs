//! Spatial sound, read only.
//!
//! What Windows documents for it is `ISpatialAudioClient`, which lets an app ask whether a device
//! offers object-based spatial audio and how many dynamic objects it can mix. That is all this
//! reads. It says what the device can do, not which format (Off, Windows Sonic, Dolby Atmos, DTS)
//! is chosen: measured on this machine it answers "up to 20 objects" for an output whose
//! property store and registry hold no spatial setting at all, so it must not be shown as "on".
//! Choosing and naming a format has no public API; the classic applet is the way to change it
//! (the page offers it), and nothing here writes.
//!
//! Source: Microsoft Learn, "Spatial Audio" (ISpatialAudioClient::IsSpatialAudioStreamAvailable,
//! GetMaxDynamicObjectCount).

use windows::Win32::Media::Audio::{ISpatialAudioClient, ISpatialAudioObjectRenderStream, IMMDevice};
use windows::Win32::System::Com::CLSCTX_ALL;
use windows::core::Interface;

/// What a device says about spatial sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spatial {
    /// Spatial formats can run on it, mixing up to this many dynamic objects.
    Supported { objects: u32 },
    NotSupported,
    /// Not asked: a microphone.
    Unknown,
}

impl Spatial {
    /// The line the page shows.
    pub fn label(&self) -> String {
        match self {
            Spatial::Supported { objects } => format!("Supported by this device, up to {objects} audio objects"),
            Spatial::NotSupported => "Not supported by this device".into(),
            Spatial::Unknown => String::new(),
        }
    }
}

/// Ask the output `dev`.
pub fn read(dev: &IMMDevice) -> Spatial {
    // SAFETY: valid interfaces; the stream GUID is the interface's own.
    unsafe {
        let Ok(client) = dev.Activate::<ISpatialAudioClient>(CLSCTX_ALL, None) else {
            return Spatial::NotSupported;
        };
        match client.IsSpatialAudioStreamAvailable(&ISpatialAudioObjectRenderStream::IID, None) {
            Ok(()) => match client.GetMaxDynamicObjectCount() {
                Ok(0) | Err(_) => Spatial::NotSupported,
                Ok(objects) => Spatial::Supported { objects },
            },
            Err(_) => Spatial::NotSupported,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_states_read_plainly() {
        assert_eq!(Spatial::NotSupported.label(), "Not supported by this device");
        assert_eq!(Spatial::Supported { objects: 15 }.label(), "Supported by this device, up to 15 audio objects");
    }
}
