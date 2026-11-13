//! Whether items can go to the Recycle Bin, worked out before deleting them.
//!
//! The shell's delete with "allow undo" recycles what it can and, for the rest, deletes for good
//! without a word: a network share, a USB stick or SD card (no bin), a drive whose bin is turned
//! off ("Don't move files to the Recycle Bin"), or an item bigger than the bin may hold. So before
//! a delete the app asks [`unrecyclable`] which items would be lost, and says so first.
//!
//! The decision is plain logic over a [`Probe`]; [`RealProbe`] asks Windows. Run it on a worker:
//! a network path can take seconds to answer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// What a volume's Recycle Bin does with what is deleted there.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BinRules {
    /// The volume has a bin at all (a fixed drive). Network shares, removable drives, optical
    /// and RAM drives have none.
    pub has_bin: bool,
    /// "Don't move files to the Recycle Bin. Remove files immediately when deleted.", for this
    /// drive or by policy for all.
    pub nuke: bool,
    /// The bin's largest size, when it is known. Anything bigger is deleted for good.
    pub max_bytes: Option<u64>,
}

/// The questions the decision asks.
pub trait Probe {
    /// The root of the volume `item` is on (`C:\`, `\\server\share\`, or a folder a volume is
    /// mounted on). None when there is none to find.
    fn root(&self, item: &Path) -> Option<String>;
    /// The bin's rules on the volume at `root`.
    fn rules(&self, root: &str) -> BinRules;
    /// How big `item` is, at least: a folder too big to add up quickly says what it found so far.
    fn size_at_least(&self, item: &Path) -> u64;
}

/// The items of `items` that would not go to the Recycle Bin but be deleted for good.
pub fn unrecyclable(items: &[PathBuf], probe: &impl Probe) -> Vec<PathBuf> {
    let mut rules: HashMap<String, BinRules> = HashMap::new();
    items
        .iter()
        .filter(|item| {
            let Some(root) = probe.root(item) else { return true };
            let r = *rules
                .entry(root.to_lowercase())
                .or_insert_with(|| probe.rules(&root));
            if !r.has_bin || r.nuke {
                return true;
            }
            r.max_bytes.is_some_and(|max| probe.size_at_least(item) > max)
        })
        .cloned()
        .collect()
}

/// How many entries a folder's size is added up over before giving up on an exact answer.
const SIZE_BUDGET: usize = 50_000;

/// Windows' answers.
pub struct RealProbe;

impl Probe for RealProbe {
    fn root(&self, item: &Path) -> Option<String> {
        use windows::Win32::Storage::FileSystem::GetVolumePathNameW;
        let wide = super::shell::wide_path(item);
        let mut buf = [0u16; 1024];
        unsafe { GetVolumePathNameW(windows::core::PCWSTR(wide.as_ptr()), &mut buf) }.ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let root = String::from_utf16_lossy(&buf[..len]);
        (!root.is_empty()).then_some(root)
    }

    fn rules(&self, root: &str) -> BinRules {
        real::rules(root)
    }

    fn size_at_least(&self, item: &Path) -> u64 {
        real::size_at_least(item, SIZE_BUDGET)
    }
}

mod real {
    use super::BinRules;
    use crate::registry::{Hive, Registry, Value};
    use std::path::Path;
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumeNameForVolumeMountPointW};

    const DRIVE_FIXED: u32 = 3;
    const ATTR_REPARSE_POINT: u32 = 0x400;
    const BIT_BUCKET: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\BitBucket\Volume";
    const POLICY: &str = r"Software\Microsoft\Windows\CurrentVersion\Policies\Explorer";

    fn dword(reg: &crate::registry::Real, hive: Hive, key: &str, name: &str) -> Option<u32> {
        match reg.get(hive, key, name)? {
            Value::Dword(v) => Some(v),
            _ => None,
        }
    }

    pub(super) fn rules(root: &str) -> BinRules {
        let mut root = root.to_owned();
        if !root.ends_with('\\') {
            root.push('\\');
        }
        let kind = unsafe { GetDriveTypeW(&HSTRING::from(root.as_str())) };
        if kind != DRIVE_FIXED {
            return BinRules::default();
        }
        let reg = crate::registry::Real;
        let policy = [Hive::Hkcu, Hive::Hklm]
            .into_iter()
            .any(|h| dword(&reg, h, POLICY, "NoRecycleFiles").is_some_and(|v| v != 0));
        let mut rules = BinRules { has_bin: true, nuke: policy, max_bytes: None };
        // The drive's own settings are kept under its volume GUID.
        let mut name = [0u16; 64];
        if unsafe { GetVolumeNameForVolumeMountPointW(&HSTRING::from(root.as_str()), &mut name) }.is_ok() {
            let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            let name = String::from_utf16_lossy(&name[..len]);
            if let Some(guid) = name.find('{').and_then(|a| name[a..].find('}').map(|b| &name[a..=a + b])) {
                let key = format!(r"{BIT_BUCKET}\{guid}");
                rules.nuke |= dword(&reg, Hive::Hkcu, &key, "NukeOnDelete").is_some_and(|v| v != 0);
                // In megabytes.
                rules.max_bytes = dword(&reg, Hive::Hkcu, &key, "MaxCapacity").map(|mb| mb as u64 * 1024 * 1024);
            }
        }
        rules
    }

    /// The size of a file, or a folder's files added up over at most `budget` entries. Reparse
    /// points (junctions, links) are not followed. Only metadata is read: nothing is downloaded.
    pub(super) fn size_at_least(item: &Path, budget: usize) -> u64 {
        use std::os::windows::fs::MetadataExt;
        let Ok(meta) = std::fs::symlink_metadata(item) else { return 0 };
        if !meta.is_dir() {
            return meta.len();
        }
        let (mut total, mut seen) = (0u64, 0usize);
        let mut stack = vec![item.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(list) = std::fs::read_dir(&dir) else { continue };
            for e in list.flatten() {
                seen += 1;
                if seen > budget {
                    return total;
                }
                let Ok(m) = e.metadata() else { continue };
                if m.is_dir() {
                    if m.file_attributes() & ATTR_REPARSE_POINT == 0 {
                        stack.push(e.path());
                    }
                } else {
                    total += m.len();
                }
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Volumes by root, roots by the start of a path, and sizes by path.
    struct Fake {
        volumes: Vec<(&'static str, BinRules)>,
        sizes: Vec<(&'static str, u64)>,
        asked: Cell<usize>,
    }

    impl Probe for Fake {
        fn root(&self, item: &Path) -> Option<String> {
            let s = item.to_string_lossy().to_lowercase();
            self.volumes
                .iter()
                .map(|(r, _)| r.to_string())
                .filter(|r| s.starts_with(&r.to_lowercase()))
                .max_by_key(|r| r.len())
        }
        fn rules(&self, root: &str) -> BinRules {
            self.asked.set(self.asked.get() + 1);
            self.volumes.iter().find(|(r, _)| r.eq_ignore_ascii_case(root)).map(|v| v.1).unwrap_or_default()
        }
        fn size_at_least(&self, item: &Path) -> u64 {
            self.sizes.iter().find(|(p, _)| Path::new(p) == item).map_or(0, |s| s.1)
        }
    }

    const BIN: BinRules = BinRules { has_bin: true, nuke: false, max_bytes: Some(1000) };

    fn fake() -> Fake {
        Fake {
            volumes: vec![
                (r"C:\", BIN),
                (r"C:\mnt\usb\", BinRules::default()),
                (r"D:\", BinRules { has_bin: true, nuke: true, max_bytes: None }),
                (r"E:\", BinRules::default()),
                (r"\\nas\share\", BinRules::default()),
                (r"F:\", BinRules { has_bin: true, nuke: false, max_bytes: None }),
            ],
            sizes: vec![(r"C:\big.iso", 5000), (r"C:\small.txt", 10), (r"F:\huge", u64::MAX)],
            asked: Cell::new(0),
        }
    }

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn a_local_drive_with_a_bin_takes_everything() {
        let f = fake();
        assert!(unrecyclable(&paths(&[r"C:\small.txt", r"C:\a\b", r"F:\huge"]), &f).is_empty());
    }

    #[test]
    fn what_cannot_go_to_the_bin_is_named() {
        let f = fake();
        let items = paths(&[
            r"C:\small.txt",
            r"C:\big.iso",            // bigger than the bin
            r"D:\x.txt",              // bin turned off for the drive
            r"E:\photo.jpg",          // a USB stick: no bin
            r"\\nas\share\doc.txt",   // a network share
            r"C:\mnt\usb\inside.txt", // a removable volume mounted in a folder
            r"C:\other.txt",
        ]);
        assert_eq!(
            unrecyclable(&items, &f),
            paths(&[r"C:\big.iso", r"D:\x.txt", r"E:\photo.jpg", r"\\nas\share\doc.txt", r"C:\mnt\usb\inside.txt"])
        );
    }

    #[test]
    fn no_volume_found_counts_as_not_recyclable() {
        let f = fake();
        assert_eq!(unrecyclable(&paths(&[r"Z:\x"]), &f), paths(&[r"Z:\x"]));
    }

    #[test]
    fn each_volume_is_asked_once() {
        let f = fake();
        let _ = unrecyclable(&paths(&[r"C:\a", r"c:\b", r"C:\c", r"E:\d", r"E:\e"]), &f);
        assert_eq!(f.asked.get(), 2);
    }

    #[test]
    fn the_temp_folder_is_recyclable_here() {
        // The machine's own temp folder is on a fixed drive with a bin (as on any normal setup).
        let dir = std::env::temp_dir().join(format!("shellman-recycle-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        let root = RealProbe.root(&file).unwrap();
        let rules = RealProbe.rules(&root);
        println!("{root}: {rules:?}");
        assert!(rules.has_bin, "{root}");
        assert_eq!(RealProbe.size_at_least(&dir), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_size_stops_at_its_budget() {
        let dir = std::env::temp_dir().join(format!("shellman-recycle-size-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        for i in 0..5 {
            std::fs::write(dir.join(format!("{i}.bin")), vec![0u8; 100]).unwrap();
        }
        std::fs::write(dir.join("sub").join("x.bin"), vec![0u8; 50]).unwrap();
        assert_eq!(real::size_at_least(&dir, 100), 550);
        assert!(real::size_at_least(&dir, 3) < 550);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
