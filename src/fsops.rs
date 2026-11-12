//! Listing folders and drives, and the shell operations: open, recycle, copy, move.
//!
//! Copy, move, and delete go through `SHFileOperationW`, so Windows supplies its own progress,
//! conflict, and undo handling. They run on a worker thread so the window never blocks.

use std::cmp::Ordering;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{FILETIME, HWND};
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
use windows::Win32::UI::Shell::{ShellExecuteW, SHFileOperationW, SHFILEOPSTRUCTW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    /// Lowercased once, for sorting and filtering.
    pub key: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// Formatted once in local time, on the loader thread.
    pub modified_text: String,
    pub kind: String,
    pub hidden: bool,
}

#[derive(Clone, Debug)]
pub struct Drive {
    pub root: PathBuf,
    pub label: String,
    pub total: u64,
    pub free: u64,
}

impl Drive {
    pub fn title(&self) -> String {
        let letter = self.root.to_string_lossy().trim_end_matches('\\').to_string();
        format!("{} ({letter})", self.label)
    }
}

pub fn list_dir(dir: &Path) -> std::io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    for item in std::fs::read_dir(dir)? {
        let Ok(item) = item else { continue };
        let Ok(meta) = item.metadata() else { continue };
        let path = item.path();
        let mut is_dir = meta.is_dir();
        if meta.file_type().is_symlink() {
            // Junctions and directory links act as folders.
            is_dir = std::fs::metadata(&path).map(|m| m.is_dir()).unwrap_or(false);
        }
        let name = item.file_name().to_string_lossy().into_owned();
        let attrs = meta.file_attributes();
        let modified = meta.modified().ok();
        out.push(Entry {
            key: name.to_lowercase(),
            kind: kind_of(&name, is_dir),
            modified_text: modified.map(format_time).unwrap_or_default(),
            size: if is_dir { 0 } else { meta.len() },
            hidden: attrs & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0
                || name.starts_with('.'),
            name,
            path,
            is_dir,
            modified,
        });
    }
    Ok(out)
}

fn kind_of(name: &str, is_dir: bool) -> String {
    if is_dir {
        return "Folder".into();
    }
    match Path::new(name).extension() {
        Some(ext) => format!("{} file", ext.to_string_lossy().to_uppercase()),
        None => "File".into(),
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Local date and time, like "2026-11-11 14:05".
pub fn format_time(t: SystemTime) -> String {
    // FILETIME counts 100 ns ticks since 1601.
    const EPOCH_DIFF: u64 = 116_444_736_000_000_000;
    let ticks = match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => EPOCH_DIFF + d.as_nanos() as u64 / 100,
        Err(_) => return String::new(),
    };
    let ft = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    unsafe {
        let mut utc = Default::default();
        let mut local = Default::default();
        if FileTimeToSystemTime(&ft, &mut utc).is_err()
            || SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_err()
        {
            return String::new();
        }
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}",
            local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute
        )
    }
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if v < 10.0 {
        format!("{v:.1} {}", UNITS[unit])
    } else {
        format!("{v:.0} {}", UNITS[unit])
    }
}

/// Compares names the way Explorer does: case-insensitive, with digit runs compared as numbers,
/// so "file2" comes before "file10".
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = a.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    a.next();
                }
                let mut nb = String::new();
                while let Some(c) = b.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    b.next();
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                a.next();
                b.next();
            }
        }
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

/// Open a file with its default app, or run it.
pub fn open(path: &Path) {
    let p = HSTRING::from(path.as_os_str());
    unsafe {
        ShellExecuteW(None, &HSTRING::from("open"), &p, None, None, SW_SHOWNORMAL);
    }
}

/// Show the item selected in the real Explorer.
pub fn reveal_in_explorer(path: &Path) {
    let _ = std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn();
}

pub fn open_in_explorer(dir: &Path) {
    let _ = std::process::Command::new("explorer.exe").arg(dir).spawn();
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Recycle,
    Copy,
    /// Copy into the folder the items are already in, as "name - Copy".
    Duplicate,
    Move,
}

/// Runs a shell file operation. Blocks, so call it from a worker thread. Ok(false) when the user
/// cancelled part of it.
pub fn shell_op(hwnd: isize, op: Op, from: &[PathBuf], to: Option<&Path>) -> Result<bool, String> {
    // Both lists are NUL-separated and end with a double NUL.
    let mut src: Vec<u16> = Vec::new();
    for p in from {
        src.extend(p.as_os_str().encode_wide());
        src.push(0);
    }
    src.push(0);
    let dst: Option<Vec<u16>> = to.map(|t| {
        let mut v: Vec<u16> = t.as_os_str().encode_wide().collect();
        v.extend([0, 0]);
        v
    });
    // FO_MOVE = 1, FO_COPY = 2, FO_DELETE = 3. FOF_ALLOWUNDO = 0x40, FOF_NOCONFIRMATION = 0x10.
    let (func, flags) = match op {
        Op::Move => (1, 0x40),
        Op::Copy => (2, 0x40),
        // FOF_RENAMEONCOLLISION = 0x8.
        Op::Duplicate => (2, 0x40 | 0x8),
        Op::Recycle => (3, 0x40 | 0x10),
    };
    let mut info = SHFILEOPSTRUCTW {
        hwnd: HWND(hwnd as *mut _),
        wFunc: func,
        pFrom: PCWSTR(src.as_ptr()),
        pTo: dst.as_ref().map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr())),
        fFlags: flags,
        ..Default::default()
    };
    let code = unsafe { SHFileOperationW(&mut info) };
    if code != 0 {
        return Err(format!("Windows could not finish (error {code:#x})"));
    }
    Ok(!info.fAnyOperationsAborted.as_bool())
}

/// "New folder", or "New folder (2)" and up when that is taken.
pub fn unique_child(dir: &Path, base: &str) -> PathBuf {
    let first = dir.join(base);
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{base} ({n})")))
        .find(|p| !p.exists())
        .unwrap()
}

/// Checks a new name before renaming, with the reason Windows would refuse it.
pub fn check_name(name: &str) -> Result<(), &'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A name can't be empty.");
    }
    if name.chars().any(|c| r#"\/:*?"<>|"#.contains(c) || (c as u32) < 32) {
        return Err(r#"A name can't contain \ / : * ? " < > |"#);
    }
    if name.ends_with('.') {
        return Err("A name can't end with a dot.");
    }
    Ok(())
}

#[allow(dead_code)]
pub fn wide_path(p: &Path) -> Vec<u16> {
    wide(&p.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["file10", "file2", "File1", "a", "file02b"];
        v.sort_by(|a, b| natural_cmp(&a.to_lowercase(), &b.to_lowercase()));
        assert_eq!(v, ["a", "File1", "file2", "file02b", "file10"]);
    }

    #[test]
    fn sizes() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(50 * 1024 * 1024), "50 MB");
    }

    #[test]
    fn names() {
        assert!(check_name("ok.txt").is_ok());
        assert!(check_name("a:b").is_err());
        assert!(check_name("  ").is_err());
        assert!(check_name("trail.").is_err());
    }

    #[test]
    fn listing_and_unique() {
        let dir = std::env::temp_dir().join(format!("fileman-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("New folder")).unwrap();
        std::fs::write(dir.join("a.txt"), b"hi").unwrap();
        let list = list_dir(&dir).unwrap();
        assert_eq!(list.len(), 2);
        let f = list.iter().find(|e| e.name == "a.txt").unwrap();
        assert_eq!((f.size, f.is_dir, f.kind.as_str()), (2, false, "TXT file"));
        assert_eq!(unique_child(&dir, "New folder"), dir.join("New folder (2)"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
