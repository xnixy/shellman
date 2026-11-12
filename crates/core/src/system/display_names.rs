//! Monitors' own names ("DELL U2720Q"), keyed by the GDI device name a monitor handle reports
//! (`\\.\DISPLAY1`). Windows keeps them from each monitor's EDID; asking is a quick local call.

use std::collections::HashMap;
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
    DISPLAYCONFIG_TARGET_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::ERROR_SUCCESS;

/// GDI device name to the monitor's friendly name, for every active monitor that has one.
pub fn friendly_names() -> HashMap<String, String> {
    let mut names = HashMap::new();
    // SAFETY: buffers are sized by GetDisplayConfigBufferSizes and the counts QueryDisplayConfig
    // writes back; each request packet carries its own size.
    unsafe {
        let (mut paths_n, mut modes_n) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths_n, &mut modes_n) != ERROR_SUCCESS {
            return names;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); paths_n as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); modes_n as usize];
        let status = QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut paths_n,
            paths.as_mut_ptr(),
            &mut modes_n,
            modes.as_mut_ptr(),
            None,
        );
        if status != ERROR_SUCCESS {
            return names;
        }
        for path in &paths[..paths_n as usize] {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            source.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            source.header.adapterId = path.sourceInfo.adapterId;
            source.header.id = path.sourceInfo.id;
            let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            target.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            target.header.size = size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            target.header.adapterId = path.targetInfo.adapterId;
            target.header.id = path.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut source.header) != 0
                || DisplayConfigGetDeviceInfo(&mut target.header) != 0
            {
                continue;
            }
            let gdi = wide_str(&source.viewGdiDeviceName);
            let friendly = wide_str(&target.monitorFriendlyDeviceName);
            if !friendly.is_empty() {
                names.insert(gdi, friendly);
            }
        }
    }
    names
}

/// A NUL-terminated UTF-16 buffer as a String.
pub fn wide_str(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_strings_stop_at_nul() {
        let mut buf = [0u16; 8];
        for (i, c) in "ab".encode_utf16().enumerate() {
            buf[i] = c;
        }
        assert_eq!(wide_str(&buf), "ab");
        assert_eq!(wide_str(&[0x41, 0x42]), "AB");
    }

    #[test]
    fn names_do_not_panic() {
        let _ = friendly_names();
    }
}
