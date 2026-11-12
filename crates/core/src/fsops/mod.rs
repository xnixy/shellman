//! Listing folders and drives, and the shell operations: open, recycle, copy, move.
//!
//! Copy, move, and delete go through `SHFileOperationW`, so Windows supplies its own progress,
//! conflict, and undo handling. They run on a worker thread so the window never blocks.

mod drives;
mod link;
mod listing;
mod names;
mod shell;
pub(crate) mod time;

pub use drives::{drives, Drive};
pub use link::link_target;
pub use listing::{list_dir, Entry};
pub use names::{check_name, natural_cmp, unique_child};
pub use shell::{open, shell_op, Op};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(50 * 1024 * 1024), "50 MB");
    }
}
