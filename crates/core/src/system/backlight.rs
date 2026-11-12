//! A laptop's built-in panel, through the video driver's brightness controls (the `\\.\LCD`
//! device). External monitors are not reached this way; they go through DDC/CI instead.

use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;

const QUERY_SUPPORTED: u32 = 0x0023_0494;
const QUERY_CURRENT: u32 = 0x0023_0498;
const SET_CURRENT: u32 = 0x0023_049C;
/// Apply the level on mains and on battery alike.
const BOTH_SOURCES: u8 = 3;

#[repr(C)]
#[derive(Default)]
struct DisplayBrightness {
    policy: u8,
    on_ac: u8,
    on_dc: u8,
}

/// The built-in panel, open for as long as this lives.
pub struct Backlight {
    file: HANDLE,
    /// The levels (0..=100) the panel supports, in rising order.
    levels: Vec<u8>,
}

// SAFETY: a file handle may be used from any thread.
unsafe impl Send for Backlight {}

impl Backlight {
    /// None on machines without a built-in panel the driver controls.
    pub fn open() -> Option<Self> {
        // SAFETY: a constant path; the handle is closed on drop.
        let file = unsafe {
            CreateFileW(
                w!(r"\\.\LCD"),
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
            .ok()?
        };
        let mut levels = [0u8; 256];
        let mut got = 0u32;
        // SAFETY: the output buffer and its size match.
        let ok = unsafe {
            DeviceIoControl(file, QUERY_SUPPORTED, None, 0, Some(levels.as_mut_ptr().cast()), 256, Some(&mut got), None)
        };
        let mut levels: Vec<u8> = levels[..(got as usize).min(256)].to_vec();
        levels.sort_unstable();
        levels.dedup();
        let me = Self { file, levels };
        (ok.is_ok() && !me.levels.is_empty()).then_some(me)
    }

    /// The current level, 0..=100.
    pub fn percent(&self) -> Option<u8> {
        let mut now = DisplayBrightness::default();
        // SAFETY: the output buffer and its size match.
        unsafe {
            DeviceIoControl(
                self.file,
                QUERY_CURRENT,
                None,
                0,
                Some((&raw mut now).cast()),
                size_of::<DisplayBrightness>() as u32,
                None,
                None,
            )
            .ok()?;
        }
        let on_battery = super::status::battery().is_some_and(|b| !b.charging);
        Some(if on_battery { now.on_dc } else { now.on_ac }.min(100))
    }

    pub fn set_percent(&self, percent: u8) -> bool {
        let level = nearest(&self.levels, percent);
        let mut want = DisplayBrightness { policy: BOTH_SOURCES, on_ac: level, on_dc: level };
        // SAFETY: the input buffer and its size match.
        unsafe {
            DeviceIoControl(
                self.file,
                SET_CURRENT,
                Some((&raw mut want).cast()),
                size_of::<DisplayBrightness>() as u32,
                None,
                0,
                None,
                None,
            )
            .is_ok()
        }
    }
}

impl Drop for Backlight {
    fn drop(&mut self) {
        // SAFETY: our own handle, closed once.
        unsafe {
            let _ = CloseHandle(self.file);
        }
    }
}

/// The supported level closest to `percent` (`levels` sorted, not empty).
fn nearest(levels: &[u8], percent: u8) -> u8 {
    levels
        .iter()
        .copied()
        .min_by_key(|&l| (l as i16 - percent as i16).abs())
        .unwrap_or(percent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_to_a_supported_level() {
        let levels = [0, 25, 50, 75, 100];
        assert_eq!(nearest(&levels, 60), 50);
        assert_eq!(nearest(&levels, 70), 75);
        assert_eq!(nearest(&levels, 100), 100);
        assert_eq!(nearest(&[], 42), 42);
    }
}
