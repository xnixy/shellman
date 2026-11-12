//! Directory listing: the `Entry` record and the loader that builds it.

use super::time::{format_time_at, LocalClock};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

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

#[cfg(test)]
mod tests {
    use super::super::names::{natural_cmp, unique_child};
    use super::*;

    #[test]
    fn listing_and_unique() {
        let dir = std::env::temp_dir().join(format!("shellman-test-{}", std::process::id()));
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
