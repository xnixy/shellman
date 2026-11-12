//! Fixed, removable, and network drives with their labels and free space.

use std::path::PathBuf;
use windows::core::HSTRING;
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};

#[derive(Clone, Debug)]
pub struct Drive {
    pub root: PathBuf,
    pub label: String,
    pub total: u64,
    pub free: u64,
}

impl Drive {
    pub fn title(&self) -> String {
        let letter = self
            .root
            .to_string_lossy()
            .trim_end_matches('\\')
            .to_string();
        format!("{} ({letter})", self.label)
    }
}

/// Fixed and removable drives, with their labels and space. Network drives are listed without
/// asking them for anything, since a disconnected one can take a long time to answer.
pub fn drives() -> Vec<Drive> {
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = format!("{}:\\", (b'A' + i as u8) as char);
        let w = HSTRING::from(root.as_str());
        let kind = unsafe { GetDriveTypeW(&w) };
        // 4 = DRIVE_REMOTE, 5 = DRIVE_CDROM.
        let probe = kind != 4 && kind != 5;
        let mut label = String::new();
        let (mut total, mut free) = (0u64, 0u64);
        if probe {
            let mut name = [0u16; 261];
            unsafe {
                let _ = GetVolumeInformationW(&w, Some(&mut name), None, None, None, None);
                let _ = GetDiskFreeSpaceExW(&w, None, Some(&mut total), Some(&mut free));
            }
            label = from_wide(&name);
        }
        if label.is_empty() {
            label = match kind {
                2 => "Removable disk",
                4 => "Network drive",
                5 => "Optical drive",
                _ => "Local disk",
            }
            .into();
        }
        out.push(Drive {
            root: PathBuf::from(root),
            label,
            total,
            free,
        });
    }
    out
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}
