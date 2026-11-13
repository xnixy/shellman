//! What the shelf did and what failed, in `%LOCALAPPDATA%\shellman\shelf.log` (cut back to its
//! last half when it passes 64 KB). The shelf has no window to say it in.

use std::io::Write;
use std::path::PathBuf;

const MAX: u64 = 64 * 1024;

fn file() -> Option<PathBuf> {
    let dir = dirs_local()?.join("shellman");
    let _ = std::fs::create_dir_all(&dir);
    let name = match std::env::var("SHELLMAN_INSTANCE").ok().filter(|i| !i.is_empty()) {
        Some(i) => format!("shelf-{i}.log"),
        None => "shelf.log".into(),
    };
    Some(dir.join(name))
}

fn dirs_local() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

/// Add a line, with the time.
pub fn line(text: impl AsRef<str>) {
    let Some(path) = file() else { return };
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX) {
        if let Ok(old) = std::fs::read(&path) {
            let half = &old[old.len() / 2..];
            let start = half.iter().position(|&b| b == b'\n').map_or(0, |i| i + 1);
            let _ = std::fs::write(&path, &half[start..]);
        }
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{secs} {}", text.as_ref());
    }
}
