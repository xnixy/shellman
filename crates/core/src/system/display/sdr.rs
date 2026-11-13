//! Brightness of a monitor running HDR: Windows' "SDR content brightness", the white level that
//! ordinary (SDR) content is drawn at. In HDR the monitor's own brightness setting (DDC/CI) is
//! usually ignored, so this is the one that changes the picture.
//!
//! Reading is documented: DisplayConfigGetDeviceInfo with DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
//! in thousandths of 80 nits. Writing has no documented call; Windows Settings uses
//! DisplayConfigSetDeviceInfo with the private type -18 (0xFFFFFFEE) and the packet below, and
//! Windows itself stores the level per monitor, so nothing else needs writing to keep it.

use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, DisplayConfigSetDeviceInfo, DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL, DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_DEVICE_INFO_TYPE,
    DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SDR_WHITE_LEVEL,
};
use windows::Win32::Devices::Display::{GetDisplayConfigBufferSizes, QueryDisplayConfig, QDC_ONLY_ACTIVE_PATHS};
use windows::Win32::Foundation::{ERROR_SUCCESS, LUID};

/// The private "set SDR white level" type Windows Settings sends.
const SET_SDR_WHITE_LEVEL: i32 = -18;
/// The range of Windows' "SDR content brightness" slider, in the API's units (80..480 nits).
pub const LEVEL_MIN: u32 = 1000;
pub const LEVEL_MAX: u32 = 6000;

/// Which monitor: the target side of a display path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub adapter: (u32, i32),
    pub id: u32,
}

impl Target {
    pub fn of(p: &DISPLAYCONFIG_PATH_INFO) -> Self {
        Self { adapter: (p.targetInfo.adapterId.LowPart, p.targetInfo.adapterId.HighPart), id: p.targetInfo.id }
    }

    fn header(&self, kind: i32, size: usize) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
        DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_TYPE(kind),
            size: size as u32,
            adapterId: LUID { LowPart: self.adapter.0, HighPart: self.adapter.1 },
            id: self.id,
        }
    }
}

#[repr(C)]
struct SetWhiteLevel {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    level: u32,
    /// 1 on the last value of a drag; Windows stores the level then.
    final_value: u8,
}

/// One active monitor as this module sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// The GDI device name of the picture it shows, such as `\\.\DISPLAY1`.
    pub device: String,
    pub target: Target,
    pub hdr: bool,
    /// The SDR white level (meaningful when `hdr`).
    pub level: Option<u32>,
}

/// Every active monitor's HDR state and SDR white level: a few cheap calls, no DDC.
pub fn states() -> Vec<State> {
    let mut out = Vec::new();
    // SAFETY: arrays are sized by GetDisplayConfigBufferSizes and truncated to the counts written back.
    let paths = unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) != ERROR_SUCCESS {
            return out;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![windows::Win32::Devices::Display::DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None) != ERROR_SUCCESS {
            return out;
        }
        paths.truncate(np as usize);
        paths
    };
    for p in &paths {
        let Some(device) = super::os_read::source_name(p) else { continue };
        let target = Target::of(p);
        let hdr = hdr_on(target);
        out.push(State { device, target, hdr, level: if hdr { white_level(target) } else { None } });
    }
    out
}

/// Whether HDR is on for the monitor (advanced colour supported and enabled).
pub fn hdr_on(t: Target) -> bool {
    let mut a = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
        header: t.header(DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO.0, size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>()),
        ..Default::default()
    };
    // SAFETY: the packet carries its own size; the union is plain bits.
    unsafe { DisplayConfigGetDeviceInfo(&mut a.header) == 0 && a.Anonymous.value & 3 == 3 }
}

/// DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 (Windows 11 24H2, type 15), not in the bindings yet.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AdvancedColor2 {
    /// Bits: 0 advanced colour supported, 1 active, 3 limited by policy, 4 HDR supported,
    /// 5 HDR user-enabled, 6 wide colour supported, 7 wide colour user-enabled.
    pub bits: u32,
    pub encoding: i32,
    pub bits_per_channel: u32,
    /// 0 SDR, 1 wide colour, 2 HDR.
    pub mode: i32,
}

/// The newer advanced-colour report, for diagnosis.
pub fn advanced_color_2(t: Target) -> Option<AdvancedColor2> {
    #[repr(C)]
    struct Packet {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
        body: AdvancedColor2,
    }
    let mut p = Packet { header: t.header(15, size_of::<Packet>()), body: AdvancedColor2::default() };
    // SAFETY: the packet carries its own size.
    (unsafe { DisplayConfigGetDeviceInfo(&mut p.header) } == 0).then_some(p.body)
}

/// The current SDR white level, in the API's units (1000 = 80 nits).
pub fn white_level(t: Target) -> Option<u32> {
    let mut w = DISPLAYCONFIG_SDR_WHITE_LEVEL {
        header: t.header(DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL.0, size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>()),
        SDRWhiteLevel: 0,
    };
    // SAFETY: the packet carries its own size.
    (unsafe { DisplayConfigGetDeviceInfo(&mut w.header) } == 0 && w.SDRWhiteLevel > 0).then_some(w.SDRWhiteLevel)
}

/// Set the SDR white level; `final_value` on the last of a drag. True when Windows took it.
pub fn set_white_level(t: Target, level: u32, final_value: bool) -> bool {
    let s = SetWhiteLevel {
        header: t.header(SET_SDR_WHITE_LEVEL, size_of::<SetWhiteLevel>()),
        level: level.clamp(LEVEL_MIN, LEVEL_MAX),
        final_value: final_value as u8,
    };
    // SAFETY: the packet carries its own size.
    unsafe { DisplayConfigSetDeviceInfo(&s.header) == 0 }
}

/// A slider position (0..=100) as a level: Windows' slider is linear from 80 to 480 nits.
pub fn level_for(percent: u8) -> u32 {
    LEVEL_MIN + (LEVEL_MAX - LEVEL_MIN) * percent.min(100) as u32 / 100
}

/// A level as the slider position Windows Settings would show for it.
pub fn percent_for(level: u32) -> u8 {
    let l = level.clamp(LEVEL_MIN, LEVEL_MAX) - LEVEL_MIN;
    ((l * 100 + (LEVEL_MAX - LEVEL_MIN) / 2) / (LEVEL_MAX - LEVEL_MIN)) as u8
}

/// A level in nits, as people know it.
pub fn nits(level: u32) -> u32 {
    level * 80 / 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_matches_windows_range() {
        assert_eq!(level_for(0), 1000);
        assert_eq!(level_for(100), 6000);
        assert_eq!(level_for(40), 3000);
        assert_eq!(nits(level_for(40)), 240);
        assert_eq!(percent_for(3000), 40);
        assert_eq!(percent_for(500), 0);
        assert_eq!(percent_for(9000), 100);
        for p in 0..=100u8 {
            assert_eq!(percent_for(level_for(p)), p);
        }
    }

    #[test]
    fn packet_is_the_size_windows_expects() {
        // header (20) + level (4) + final flag (1), padded to 4.
        assert_eq!(size_of::<DISPLAYCONFIG_DEVICE_INFO_HEADER>(), 20);
        assert_eq!(size_of::<SetWhiteLevel>(), 28);
    }
}
