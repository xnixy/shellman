//! A device's audio format: the rates and bit depths of the classic Sound dialog's Advanced tab.
//!
//! The format is kept as a `FormatSpec` and turned to and from the WAVEFORMATEXTENSIBLE bytes the
//! audio service speaks. The list a device offers is found as the classic dialog finds it: ask
//! the device, in exclusive mode, whether it takes each combination (nothing is opened or
//! played). The format is read and written through IPolicyConfig, the same undocumented
//! interface the default-device switch uses (see `audio`); the Advanced tab does the same.

use super::audio::policy;
use windows::core::{GUID, PCWSTR};
use windows::Win32::Media::Audio::{
    IAudioClient, IMMDevice, WAVEFORMATEX, AUDCLNT_SHAREMODE_EXCLUSIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL};

/// The usual rates, as the classic dialog lists them.
pub const RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
/// The bit depths it lists.
pub const DEPTHS: [u16; 3] = [16, 24, 32];

const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const SUBTYPE_PCM: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);
const SUBTYPE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
/// Size of a WAVEFORMATEXTENSIBLE: the 18-byte header and its 22 extra bytes.
const EXT_LEN: usize = 40;

/// One format: what the device is set to run at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatSpec {
    pub rate: u32,
    /// Bits in each sample's container.
    pub bits: u16,
    /// Bits that carry sound; fewer than `bits` for 24 in 32.
    pub valid_bits: u16,
    pub channels: u16,
    /// Which speakers the channels are (the speaker mask).
    pub mask: u32,
    pub float: bool,
    /// Not sound samples but an encoded stream passed through (Dolby, DTS over HDMI or S/PDIF).
    /// Shown, but never offered or built.
    pub encoded: bool,
    /// Which encoding, when `encoded`: the first field of the format's subtype GUID. 0 otherwise.
    pub codec: u32,
}

/// The speaker layouts a device is tried with: the common ones, as the classic dialog offers for
/// HDMI devices. (channels, speaker mask)
const LAYOUTS: [(u16, u32); 3] = [(2, 0x3), (6, 0x3F), (8, 0x63F)];

impl FormatSpec {
    /// PCM at `rate` and depth `bits` with the layout of `like`.
    pub fn pcm(like: &FormatSpec, rate: u32, bits: u16) -> Self {
        Self { rate, bits, valid_bits: bits, channels: like.channels, mask: like.mask, float: false, encoded: false, codec: 0 }
    }

    /// The label with the channels in front, as the classic dialog words it for a device that
    /// takes several layouts: "2 channel, 24 bit, 48000 Hz (Studio Quality)".
    pub fn full_label(&self) -> String {
        if self.encoded {
            return format!("{}, {} channel, {} Hz", self.codec_name(), self.channels, self.rate);
        }
        format!("{} channel, {}", self.channels, self.label())
    }

    /// The name of the encoding of an encoded stream (the IEC 61937 subtypes of ksmedia.h).
    pub fn codec_name(&self) -> &'static str {
        match self.codec {
            0x92 => "Dolby Digital",
            0x0A => "Dolby Digital Plus",
            0x0C => "Dolby MAT (TrueHD, Atmos)",
            0x10C => "Dolby MAT 2.0 (Atmos)",
            0x30C => "Dolby MAT 2.1 (Atmos)",
            0x08 => "DTS",
            0x0B => "DTS-HD",
            0x10B | 0x30B => "DTS:X",
            _ => "Encoded stream",
        }
    }

    /// As the classic dialog words it: "24 bit, 48000 Hz (Studio Quality)".
    pub fn label(&self) -> String {
        let quality = match (self.bits.min(self.valid_bits), self.rate) {
            (16, 44_100) => " (CD Quality)",
            (16, 48_000) => " (DVD Quality)",
            (24.., _) if self.rate >= 48_000 => " (Studio Quality)",
            _ => "",
        };
        let kind = if self.float { " float" } else { "" };
        format!("{} bit{kind}, {} Hz{quality}", self.valid_bits, self.rate)
    }

    /// The channels, as people say it.
    pub fn channel_label(&self) -> String {
        match self.channels {
            1 => "Mono".into(),
            2 => "Stereo".into(),
            6 => "5.1".into(),
            8 => "7.1".into(),
            n => format!("{n} channels"),
        }
    }

    /// The WAVEFORMATEXTENSIBLE bytes.
    pub fn to_bytes(&self) -> [u8; EXT_LEN] {
        let mut b = [0u8; EXT_LEN];
        let block = self.channels * self.bits / 8;
        b[0..2].copy_from_slice(&WAVE_FORMAT_EXTENSIBLE.to_le_bytes());
        b[2..4].copy_from_slice(&self.channels.to_le_bytes());
        b[4..8].copy_from_slice(&self.rate.to_le_bytes());
        b[8..12].copy_from_slice(&(self.rate * u32::from(block)).to_le_bytes());
        b[12..14].copy_from_slice(&block.to_le_bytes());
        b[14..16].copy_from_slice(&self.bits.to_le_bytes());
        b[16..18].copy_from_slice(&22u16.to_le_bytes());
        b[18..20].copy_from_slice(&self.valid_bits.to_le_bytes());
        b[20..24].copy_from_slice(&self.mask.to_le_bytes());
        let sub = if self.float { SUBTYPE_FLOAT } else { SUBTYPE_PCM };
        b[24..28].copy_from_slice(&sub.data1.to_le_bytes());
        b[28..30].copy_from_slice(&sub.data2.to_le_bytes());
        b[30..32].copy_from_slice(&sub.data3.to_le_bytes());
        b[32..40].copy_from_slice(&sub.data4);
        b
    }

    /// Read a WAVEFORMATEX or WAVEFORMATEXTENSIBLE; None for anything that is not PCM or float.
    pub fn parse(b: &[u8]) -> Option<Self> {
        let u16_at = |i: usize| b.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]]));
        let u32_at = |i: usize| b.get(i..i + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
        let tag = u16_at(0)?;
        let (channels, rate, bits) = (u16_at(2)?, u32_at(4)?, u16_at(14)?);
        let (valid_bits, mask, float, encoded, codec) = match tag {
            1 => (bits, 0, false, false, 0),
            3 => (bits, 0, true, false, 0),
            WAVE_FORMAT_EXTENSIBLE if b.len() >= EXT_LEN => {
                let sub = b.get(24..28)?;
                let float = sub == SUBTYPE_FLOAT.data1.to_le_bytes();
                let pcm = sub == SUBTYPE_PCM.data1.to_le_bytes();
                let encoded = !float && !pcm;
                (u16_at(18)?, u32_at(20)?, float, encoded, if encoded { u32_at(24)? } else { 0 })
            }
            _ => return None,
        };
        if channels == 0 || rate == 0 || bits == 0 {
            return None;
        }
        Some(Self { rate, bits, valid_bits, channels, mask, float, encoded, codec })
    }
}

/// Every layout, depth and rate to try for a device set to `like`: its own layout (when it is
/// sound, not an encoded stream) and the common ones, each at every depth and rate.
pub fn candidates(like: &FormatSpec) -> Vec<FormatSpec> {
    let mut layouts: Vec<(u16, u32)> = LAYOUTS.to_vec();
    if !like.encoded && !layouts.iter().any(|l| l.0 == like.channels) {
        layouts.insert(0, (like.channels, like.mask));
    }
    let mut out = Vec::new();
    for (channels, mask) in layouts {
        let base = FormatSpec { channels, mask, ..*like };
        for &bits in &DEPTHS {
            out.extend(RATES.iter().map(|&rate| FormatSpec::pcm(&base, rate, bits)));
        }
    }
    out
}

/// Keep the formats `ok` accepts.
pub fn supported(like: &FormatSpec, ok: impl Fn(&FormatSpec) -> bool) -> Vec<FormatSpec> {
    candidates(like).into_iter().filter(|f| ok(f)).collect()
}

/// Whether the device takes `f` in exclusive mode. Nothing is opened: the service only answers.
pub fn device_accepts(dev: &IMMDevice, f: &FormatSpec) -> bool {
    // SAFETY: valid interface; the bytes outlive the call and have a WAVEFORMATEX's layout.
    unsafe {
        let Ok(client) = dev.Activate::<IAudioClient>(CLSCTX_ALL, None) else {
            return false;
        };
        let bytes = f.to_bytes();
        client
            .IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, bytes.as_ptr() as *const WAVEFORMATEX, None)
            .0
            == 0
    }
}

/// The formats the device `dev` takes, around the format it is set to now.
pub fn probe(dev: &IMMDevice, current: &FormatSpec) -> Vec<FormatSpec> {
    supported(current, |f| device_accepts(dev, f))
}

/// What a read of the device format can find.
pub struct Raw(Vec<u8>);

fn policy_client() -> Option<policy::IPolicyConfig> {
    // SAFETY: the interface is created by its own class.
    unsafe { CoCreateInstance(&super::audio::POLICY_CONFIG_CLIENT, None, CLSCTX_ALL).ok() }
}

fn copy_blob(p: *mut core::ffi::c_void) -> Option<Raw> {
    if p.is_null() {
        return None;
    }
    // SAFETY: the service returns a WAVEFORMATEX, with cbSize saying how much follows it; freed
    // with CoTaskMemFree, as it was allocated.
    unsafe {
        let head = std::slice::from_raw_parts(p as *const u8, 18);
        let extra = usize::from(u16::from_le_bytes([head[16], head[17]]));
        let blob = std::slice::from_raw_parts(p as *const u8, 18 + extra).to_vec();
        CoTaskMemFree(Some(p));
        Some(Raw(blob))
    }
}

impl Raw {
    pub fn spec(&self) -> Option<FormatSpec> {
        FormatSpec::parse(&self.0)
    }
}

/// The format the device is set to (what the Advanced tab shows), exactly as Windows holds it.
pub fn device_format(id: &str) -> Option<Raw> {
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    let mut p = std::ptr::null_mut();
    // SAFETY: `wide` outlives the call; `p` receives an allocation `copy_blob` frees.
    let hr = unsafe { policy::get_device_format(&policy_client()?, PCWSTR(wide.as_ptr()), 0, &mut p) };
    if hr.is_ok() { copy_blob(p) } else { None }
}

/// The mix format the shared-mode engine runs at, exactly as Windows holds it.
pub fn mix_format(id: &str) -> Option<Raw> {
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    let mut p = std::ptr::null_mut();
    // SAFETY: as in `device_format`.
    let hr = unsafe { policy::get_mix_format(&policy_client()?, PCWSTR(wide.as_ptr()), &mut p) };
    if hr.is_ok() { copy_blob(p) } else { None }
}

/// Set the device to `spec`, and read it back to be sure. The mix format is set along, as the
/// Advanced tab does. True when the device now reports `spec`.
pub fn set_device_format(id: &str, spec: &FormatSpec) -> bool {
    let bytes = spec.to_bytes();
    apply(id, &bytes, &bytes) && device_format(id).and_then(|r| r.spec()) == Some(*spec)
}

/// Put back a format and mix format read earlier by `device_format` and `mix_format`.
pub fn restore(id: &str, device: &Raw, mix: &Raw) -> bool {
    apply(id, &device.0, &mix.0)
}

fn apply(id: &str, device: &[u8], mix: &[u8]) -> bool {
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    let (mut d, mut m) = (device.to_vec(), mix.to_vec());
    let Some(client) = policy_client() else { return false };
    // SAFETY: both buffers are WAVEFORMATEX blobs that outlive the call.
    unsafe {
        policy::set_device_format(
            &client,
            PCWSTR(wide.as_ptr()),
            d.as_mut_ptr() as *mut _,
            m.as_mut_ptr() as *mut _,
        )
        .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo() -> FormatSpec {
        FormatSpec { rate: 48_000, bits: 16, valid_bits: 16, channels: 2, mask: 3, float: false, encoded: false, codec: 0 }
    }

    #[test]
    fn labels_follow_the_classic_dialog() {
        assert_eq!(FormatSpec::pcm(&stereo(), 44_100, 16).label(), "16 bit, 44100 Hz (CD Quality)");
        assert_eq!(stereo().label(), "16 bit, 48000 Hz (DVD Quality)");
        assert_eq!(FormatSpec::pcm(&stereo(), 96_000, 24).label(), "24 bit, 96000 Hz (Studio Quality)");
        assert_eq!(FormatSpec::pcm(&stereo(), 192_000, 16).label(), "16 bit, 192000 Hz");
    }

    #[test]
    fn bytes_round_trip() {
        for f in candidates(&stereo()) {
            assert_eq!(FormatSpec::parse(&f.to_bytes()), Some(f));
        }
    }

    #[test]
    fn parses_what_windows_reports() {
        // From a real device: 16 bit, 48 kHz, stereo, as WAVEFORMATEXTENSIBLE.
        let hex = "FEFF020080BB000000EE02000400100016001000030000000100000000001000800000AA00389B71";
        let bytes: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
        assert_eq!(FormatSpec::parse(&bytes), Some(stereo()));
    }

    #[test]
    fn plain_and_odd_formats() {
        let mut plain = stereo().to_bytes()[..18].to_vec();
        plain[0] = 1;
        plain[1] = 0;
        plain[16] = 0;
        assert_eq!(FormatSpec::parse(&plain).map(|f| (f.rate, f.mask)), Some((48_000, 0)));
        plain[0] = 2; // ADPCM
        assert_eq!(FormatSpec::parse(&plain), None);
        assert_eq!(FormatSpec::parse(&[1, 0]), None);
    }

    /// What the default output reports, by hand only; reads only.
    #[test]
    #[ignore]
    fn what_the_default_output_says() {
        crate::system::live::init_mta();
        let en = crate::system::audio::enumerator().unwrap();
        let d = unsafe { en.GetDefaultAudioEndpoint(windows::Win32::Media::Audio::eRender, windows::Win32::Media::Audio::eConsole).unwrap() };
        let id = unsafe { crate::system::sound_dev::device_id(&d).unwrap() };
        println!("device format {:?}", device_format(&id).map(|r| r.spec()));
        println!("mix format {:?}", mix_format(&id).map(|r| r.spec()));
        let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
        for default in [0, 1] {
            let mut p = std::ptr::null_mut();
            let hr = unsafe { policy::get_device_format(&policy_client().unwrap(), PCWSTR(wide.as_ptr()), default, &mut p) };
            println!("GetDeviceFormat({default}) = {hr:?} {:02X?}", copy_blob(p).map(|r| r.0));
        }
    }

    #[test]
    fn candidates_cover_rates_and_depths() {
        let all = candidates(&stereo());
        assert_eq!(all.len(), RATES.len() * DEPTHS.len() * LAYOUTS.len());
        let some = supported(&stereo(), |f| f.rate == 48_000 && f.channels == 2);
        assert_eq!(some.iter().map(|f| f.bits).collect::<Vec<_>>(), vec![16, 24, 32]);
        // An odd layout of the device's own is tried too, first.
        let odd = FormatSpec { channels: 4, mask: 0x33, ..stereo() };
        assert_eq!(candidates(&odd)[0].channels, 4);
        assert_eq!(candidates(&odd).len(), RATES.len() * DEPTHS.len() * 4);
    }

    #[test]
    fn an_encoded_stream_is_recognised_and_not_built_on() {
        // What an HDMI output set to Dolby passthrough reports: 8 channels, 192 kHz, not PCM.
        let hex = "FEFF0800 00EE0200 00E02E00 10001000 16001000 3F060000 0C000000 EA0C1000 800000AA00389B71";
        let hex: String = hex.split_whitespace().collect();
        let bytes: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
        let f = FormatSpec::parse(&bytes).unwrap();
        assert!(f.encoded && f.channels == 8 && f.rate == 192_000);
        assert_eq!(f.full_label(), "Dolby MAT (TrueHD, Atmos), 8 channel, 192000 Hz");
        assert!(candidates(&f).iter().all(|c| !c.encoded));
        assert_eq!(candidates(&f).len(), RATES.len() * DEPTHS.len() * LAYOUTS.len());
    }
}
