//! Storage: the fixed drives with their space, and the size of the largest top-level folders of
//! the system drive's user data. The sizes come from a scan on worker threads that reports as
//! it goes, can be cancelled, and never follows a reparse point (a junction, a symlink, a cloud
//! placeholder), so it cannot loop or count what is not on this disk. Read-only: nothing here
//! deletes or changes a file.
//!
//! A size is the files' length, not the space they take up on disk, as Files shows it.

use crate::fsops::drives;
use crate::recycle_bin;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::HSTRING;
use windows::Win32::Storage::FileSystem::GetDriveTypeW;

const DRIVE_FIXED: u32 = 3;
const ATTR_REPARSE_POINT: u32 = 0x400;
/// Offline (OneDrive placeholder) and "recall on open / on data access": not on this disk.
const ATTR_NOT_LOCAL: u32 = 0x1000 | 0x4_0000 | 0x40_0000;

/// A fixed drive with its space.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Volume {
    pub root: PathBuf,
    pub label: String,
    pub total: u64,
    pub free: u64,
    /// The drive Windows is on.
    pub system: bool,
}

impl Volume {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.free)
    }
}

/// The share of the drive in use, 0 to 1.
pub fn used_fraction(total: u64, free: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }
    (total.saturating_sub(free) as f64 / total as f64).clamp(0.0, 1.0) as f32
}

/// The share `part` is of `whole`, 0 to 1.
pub fn share(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        return 0.0;
    }
    (part as f64 / whole as f64).clamp(0.0, 1.0) as f32
}

/// Whether the drive is nearly full: under a tenth free, where Windows itself starts warning.
pub fn nearly_full(total: u64, free: u64) -> bool {
    total > 0 && free * 10 < total
}

fn system_root() -> PathBuf {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
    PathBuf::from(format!("{}\\", drive.trim_end_matches('\\')))
}

/// The fixed drives (not removable, optical or network ones), system drive first.
pub fn fixed_volumes() -> Vec<Volume> {
    let sys = system_root();
    let mut out: Vec<Volume> = drives()
        .into_iter()
        .filter(|d| {
            let w = HSTRING::from(d.root.to_string_lossy().as_ref());
            // SAFETY: plain query on a drive root.
            unsafe { GetDriveTypeW(&w) == DRIVE_FIXED }
        })
        .map(|d| Volume { system: d.root.to_string_lossy().eq_ignore_ascii_case(&sys.to_string_lossy()), root: d.root, label: d.label, total: d.total, free: d.free })
        .collect();
    out.sort_by_key(|v| !v.system);
    out
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    Folder(PathBuf),
    RecycleBin,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Target {
    pub name: &'static str,
    pub kind: Kind,
}

/// The folders worth sizing, in the order they are shown.
pub fn user_targets() -> Vec<Target> {
    let home = dirs::home_dir().unwrap_or_default();
    let known = |p: Option<PathBuf>, fallback: &str| p.unwrap_or_else(|| home.join(fallback));
    let t = |name, path| Target { name, kind: Kind::Folder(path) };
    vec![
        t("Downloads", known(dirs::download_dir(), "Downloads")),
        t("Documents", known(dirs::document_dir(), "Documents")),
        t("Desktop", known(dirs::desktop_dir(), "Desktop")),
        t("Pictures", known(dirs::picture_dir(), "Pictures")),
        t("Videos", known(dirs::video_dir(), "Videos")),
        t("AppData", home.join("AppData")),
        Target { name: "Recycle Bin", kind: Kind::RecycleBin },
    ]
}

/// Where a target is, to open it in Files.
pub fn open_arg(t: &Target) -> String {
    match &t.kind {
        Kind::Folder(p) => p.to_string_lossy().into_owned(),
        Kind::RecycleBin => "--recycle-bin".to_owned(),
    }
}

/// How far one target's count has got.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum State {
    Waiting,
    Counting { bytes: u64, files: u64 },
    Done(u64),
    /// Stopped by the user, with what had been counted.
    Cancelled(u64),
    /// The folder is not there or cannot be read.
    Missing,
}

impl State {
    /// The bytes counted so far, if any.
    pub fn bytes(&self) -> Option<u64> {
        match self {
            State::Counting { bytes, .. } => Some(*bytes),
            State::Done(b) | State::Cancelled(b) => Some(*b),
            _ => None,
        }
    }

    pub fn pending(&self) -> bool {
        matches!(self, State::Waiting | State::Counting { .. })
    }
}

/// Add up the size of the files under `root`, skipping every reparse point, calling `progress`
/// with (bytes, files) so far now and then. `None` when cancelled or when `root` cannot be read;
/// the last progress call has what had been counted.
pub fn walk_size(root: &Path, cancel: &AtomicBool, progress: &mut dyn FnMut(u64, u64)) -> Option<u64> {
    use std::os::windows::fs::MetadataExt;
    if std::fs::read_dir(root).is_err() {
        return None;
    }
    let (mut bytes, mut files) = (0u64, 0u64);
    let mut stack = vec![root.to_path_buf()];
    let mut last = Instant::now();
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            progress(bytes, files);
            return None;
        }
        // A folder that cannot be opened (no access) is counted as empty, not as a failure.
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            // The attributes come with the directory listing: no extra call per file.
            let Ok(meta) = entry.metadata() else { continue };
            let attrs = meta.file_attributes();
            if attrs & ATTR_REPARSE_POINT != 0 {
                continue;
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else if attrs & ATTR_NOT_LOCAL == 0 {
                bytes += meta.len();
                files += 1;
            }
        }
        if last.elapsed() >= Duration::from_millis(120) {
            progress(bytes, files);
            last = Instant::now();
        }
    }
    progress(bytes, files);
    Some(bytes)
}

/// A scan in progress: a few worker threads count the targets, each reporting into `states`.
/// Dropping it cancels what is left.
pub struct Scan {
    pub targets: Vec<Target>,
    states: Arc<Mutex<Vec<State>>>,
    cancel: Arc<AtomicBool>,
}

impl Scan {
    pub fn start(targets: Vec<Target>, wake: Arc<dyn Fn() + Send + Sync>) -> Scan {
        let states = Arc::new(Mutex::new(vec![State::Waiting; targets.len()]));
        let cancel = Arc::new(AtomicBool::new(false));
        let queue: Arc<Mutex<VecDeque<(usize, Target)>>> = Arc::new(Mutex::new(targets.iter().cloned().enumerate().collect()));
        // Three at a time: enough to keep a disk busy, not enough to make it a storm.
        for _ in 0..3.min(targets.len()) {
            let (states, cancel, queue, wake) = (states.clone(), cancel.clone(), queue.clone(), wake.clone());
            std::thread::spawn(move || loop {
                let next = queue.lock().ok().and_then(|mut q| q.pop_front());
                let Some((i, target)) = next else { return };
                let set = |s: State| {
                    if let Ok(mut all) = states.lock() {
                        all[i] = s;
                    }
                    wake();
                };
                if cancel.load(Ordering::Relaxed) {
                    set(State::Cancelled(0));
                    continue;
                }
                match &target.kind {
                    Kind::RecycleBin => set(recycle_bin::state().map_or(State::Missing, |b| State::Done(b.bytes))),
                    Kind::Folder(path) => {
                        set(State::Counting { bytes: 0, files: 0 });
                        let mut seen = 0;
                        let done = walk_size(path, &cancel, &mut |bytes, files| {
                            seen = bytes;
                            set(State::Counting { bytes, files });
                        });
                        set(match done {
                            Some(b) => State::Done(b),
                            None if cancel.load(Ordering::Relaxed) => State::Cancelled(seen),
                            None => State::Missing,
                        });
                    }
                }
            });
        }
        Scan { targets, states, cancel }
    }

    pub fn states(&self) -> Vec<State> {
        self.states.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn running(&self) -> bool {
        self.states().iter().any(State::pending)
    }
}

impl Drop for Scan {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("shellman-storage-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn fractions() {
        assert_eq!(used_fraction(100, 25), 0.75);
        assert_eq!(used_fraction(0, 0), 0.0);
        assert_eq!(used_fraction(100, 500), 0.0);
        assert_eq!(share(50, 200), 0.25);
        assert_eq!(share(5, 0), 0.0);
        assert!(nearly_full(100, 9));
        assert!(!nearly_full(100, 10));
        assert!(!nearly_full(0, 0));
    }

    #[test]
    fn sizes_add_up_through_folders() {
        let root = temp("sum");
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("one"), vec![0u8; 100]).unwrap();
        fs::write(root.join("a/two"), vec![0u8; 250]).unwrap();
        fs::write(root.join("a/b/three"), vec![0u8; 1000]).unwrap();
        let cancel = AtomicBool::new(false);
        let mut last = (0, 0);
        assert_eq!(walk_size(&root, &cancel, &mut |b, f| last = (b, f)), Some(1350));
        assert_eq!(last, (1350, 3));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_junction_is_not_followed() {
        let root = temp("junction");
        let outside = temp("junction-outside");
        fs::write(outside.join("big"), vec![0u8; 5000]).unwrap();
        fs::write(root.join("small"), vec![0u8; 10]).unwrap();
        let link = root.join("link");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if made {
            assert!(link.join("big").exists());
            let cancel = AtomicBool::new(false);
            assert_eq!(walk_size(&root, &cancel, &mut |_, _| {}), Some(10));
        }
        // `remove_dir` on the junction removes the link, not what it points at.
        let _ = fs::remove_dir(&link);
        assert!(!made || outside.join("big").exists());
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn a_cancel_stops_the_walk() {
        let root = temp("cancel");
        fs::write(root.join("f"), b"x").unwrap();
        let cancel = AtomicBool::new(true);
        assert_eq!(walk_size(&root, &cancel, &mut |_, _| {}), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_folder_is_none() {
        let cancel = AtomicBool::new(false);
        assert_eq!(walk_size(Path::new("Z:\\no\\such\\folder"), &cancel, &mut |_, _| {}), None);
    }

    #[test]
    fn a_scan_finishes_and_a_missing_target_is_marked() {
        let root = temp("scan");
        fs::write(root.join("f"), vec![0u8; 7]).unwrap();
        let targets = vec![
            Target { name: "here", kind: Kind::Folder(root.clone()) },
            Target { name: "gone", kind: Kind::Folder(PathBuf::from("Z:\\no\\such")) },
        ];
        let scan = Scan::start(targets, Arc::new(|| {}));
        for _ in 0..200 {
            if !scan.running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(scan.states(), vec![State::Done(7), State::Missing]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn this_machine_has_a_system_drive() {
        let v = fixed_volumes();
        assert!(v.first().is_some_and(|d| d.system && d.total > 0));
        assert!(user_targets().len() >= 6);
    }
}
