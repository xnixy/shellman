//! Listing folders and drives, and the shell operations: open, recycle, copy, move.
//!
//! Copy, move, and delete go through `SHFileOperationW`, so Windows supplies its own progress,
//! conflict, and undo handling. They run on a worker thread so the window never blocks.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{FILETIME, HWND, SYSTEMTIME};
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};
use windows::Win32::System::Time::{
    FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime,
};
use windows::Win32::UI::Shell::{SHFileOperationW, ShellExecuteW, SHFILEOPSTRUCTW};
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
        let letter = self
            .root
            .to_string_lossy()
            .trim_end_matches('\\')
            .to_string();
        format!("{} ({letter})", self.label)
    }
}

pub fn list_dir(dir: &Path) -> std::io::Result<Vec<Entry>> {
    let mut clock = LocalClock::default();
    let mut out = Vec::with_capacity(512);
    for item in std::fs::read_dir(dir)? {
        let Ok(item) = item else { continue };
        let Ok(meta) = item.metadata() else { continue };
        let path = item.path();
        let mut is_dir = meta.is_dir();
        if meta.file_type().is_symlink() {
            // Junctions and directory links act as folders.
            is_dir = std::fs::metadata(&path)
                .map(|m| m.is_dir())
                .unwrap_or(false);
        }
        let name = item.file_name().to_string_lossy().into_owned();
        let attrs = meta.file_attributes();
        let modified = meta.modified().ok();
        out.push(Entry {
            key: name.to_lowercase(),
            kind: kind_of(&name, is_dir),
            modified_text: modified
                .map(|t| format_time_at(t, clock.offset_for(t)))
                .unwrap_or_default(),
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

/// Seconds between 1601 (FILETIME's epoch) and 1970.
const FILETIME_EPOCH_SECS: i64 = 11_644_473_600;

/// UTC-to-local offsets, looked up once per distinct hour.
///
/// Each date gets the daylight-saving rule that applied on it, exactly as Explorer shows it, yet
/// a folder of 100 000 files costs one Win32 call per distinct hour they were written in, not two
/// per file.
#[derive(Default)]
struct LocalClock {
    by_hour: HashMap<i64, i64>,
}

impl LocalClock {
    fn offset_for(&mut self, t: SystemTime) -> i64 {
        let Ok(d) = t.duration_since(SystemTime::UNIX_EPOCH) else {
            return 0;
        };
        let hour = d.as_secs() as i64 / 3600;
        *self
            .by_hour
            .entry(hour)
            .or_insert_with(|| tz_offset_at(hour * 3600))
    }
}

/// Seconds to add to a UTC time (seconds since 1970) to get local time at that moment.
fn tz_offset_at(utc_secs: i64) -> i64 {
    let ticks = ((utc_secs + FILETIME_EPOCH_SECS) * 10_000_000) as u64;
    let ft = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    let mut local_ft = FILETIME::default();
    let ok = unsafe {
        FileTimeToSystemTime(&ft, &mut utc).is_ok()
            && SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_ok()
            && SystemTimeToFileTime(&local, &mut local_ft).is_ok()
    };
    if !ok {
        return 0;
    }
    let local_ticks = (local_ft.dwHighDateTime as u64) << 32 | local_ft.dwLowDateTime as u64;
    (local_ticks / 10_000_000) as i64 - FILETIME_EPOCH_SECS - utc_secs
}

/// Local date and time like "2026-11-11 14:05", from pure arithmetic so a 100k-file listing makes
/// no per-entry system calls.
fn format_time_at(t: SystemTime, offset_secs: i64) -> String {
    let Ok(d) = t.duration_since(SystemTime::UNIX_EPOCH) else {
        return String::new();
    };
    let secs = d.as_secs() as i64 + offset_secs;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (Howard Hinnant), valid for the whole proleptic Gregorian calendar.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + (month <= 2) as i64;
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60
    )
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

/// Compares names the way Explorer does: digit runs compare as numbers, so "file2" comes before
/// "file10". Works in place on the bytes (UTF-8 byte order equals code point order), because it
/// runs O(n log n) times per sort and must not allocate. Callers pass lowercased keys.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (ei, ej) = (run_end(a, i), run_end(b, j));
            // Leading zeros don't count; then a longer run is a bigger number.
            let (ra, rb) = (&a[skip_zeros(a, i, ei)..ei], &b[skip_zeros(b, j, ej)..ej]);
            let ord = ra.len().cmp(&rb.len()).then_with(|| ra.cmp(rb));
            if ord != Ordering::Equal {
                return ord;
            }
            (i, j) = (ei, ej);
        } else if a[i] != b[j] {
            return a[i].cmp(&b[j]);
        } else {
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}

fn run_end(s: &[u8], mut i: usize) -> usize {
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    i
}

fn skip_zeros(s: &[u8], mut i: usize, end: usize) -> usize {
    while i < end && s[i] == b'0' {
        i += 1;
    }
    i
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
    if name
        .chars()
        .any(|c| r#"\/:*?"<>|"#.contains(c) || (c as u32) < 32)
    {
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
    fn natural_leading_zeros_and_prefixes() {
        assert_eq!(natural_cmp("a007", "a7"), Ordering::Equal);
        assert_eq!(natural_cmp("a007", "a8"), Ordering::Less);
        assert_eq!(natural_cmp("a0010", "a9"), Ordering::Greater);
        assert_eq!(natural_cmp("a1b", "a01c"), Ordering::Less);
        assert_eq!(natural_cmp("v1.10", "v1.9"), Ordering::Greater);
        assert_eq!(natural_cmp("abc", "abcd"), Ordering::Less);
        assert_eq!(natural_cmp("", "a"), Ordering::Less);
        assert_eq!(natural_cmp("x00", "x0"), Ordering::Equal);
    }

    #[test]
    fn natural_non_ascii() {
        assert_eq!(natural_cmp("é2", "é10"), Ordering::Less);
        assert_eq!(natural_cmp("z", "é"), Ordering::Less);
        assert_eq!(natural_cmp("日本1", "日本01"), Ordering::Equal);
        assert_eq!(natural_cmp("日本", "日本語"), Ordering::Less);
        assert_eq!(natural_cmp("ä", "ö"), Ordering::Less);
    }

    #[test]
    fn local_offsets_are_whole_quarter_hours() {
        let mut clock = LocalClock::default();
        let now = SystemTime::now();
        let winter = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_704_067_200);
        let summer = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_719_792_000);
        for t in [now, winter, summer] {
            let off = clock.offset_for(t);
            assert_eq!(off % 900, 0, "offset {off}");
            assert!(off.abs() <= 14 * 3600, "offset {off}");
        }
        // The same hour is answered from the cache.
        assert_eq!(clock.offset_for(now), clock.offset_for(now));
        assert!(clock.by_hour.len() <= 3);
    }

    #[test]
    fn time_format() {
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        assert_eq!(format_time_at(t, 0), "2023-11-14 22:13");
        assert_eq!(format_time_at(t, 3600), "2023-11-14 23:13");
        let leap = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_709_164_800);
        assert_eq!(format_time_at(leap, 0), "2024-02-29 00:00");
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

    /// Run with `cargo test --release -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn bench_list_and_sort() {
        for dir in [r"C:\Windows\System32", r"C:\Windows\WinSxS"] {
            let t0 = std::time::Instant::now();
            let Ok(mut list) = list_dir(Path::new(dir)) else {
                continue;
            };
            let listed = t0.elapsed();
            let t1 = std::time::Instant::now();
            list.sort_by(|a, b| {
                b.is_dir
                    .cmp(&a.is_dir)
                    .then_with(|| natural_cmp(&a.key, &b.key))
            });
            println!(
                "{dir}: {} entries, list_dir {listed:?}, natural sort {:?}",
                list.len(),
                t1.elapsed()
            );
        }
    }
}
