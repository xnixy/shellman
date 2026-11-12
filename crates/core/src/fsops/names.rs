//! Name rules: natural sort order, rename validation, and unused child names.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

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
    fn names() {
        assert!(check_name("ok.txt").is_ok());
        assert!(check_name("a:b").is_err());
        assert!(check_name("  ").is_err());
        assert!(check_name("trail.").is_err());
    }
}
