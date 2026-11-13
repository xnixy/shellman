//! `CF_HDROP`'s bytes: a `DROPFILES` header, then the paths, each ending with a NUL, and one
//! more NUL after the last.

use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

/// `DROPFILES`: the offset of the list, a point, whether it is in client coordinates, and
/// whether the names are UTF-16.
const HEADER: usize = 20;

/// The bytes of a `CF_HDROP` for `paths` (wide names).
pub fn build(paths: &[PathBuf]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER + paths.len() * 64);
    out.extend_from_slice(&(HEADER as u32).to_le_bytes());
    out.extend_from_slice(&[0; 12]);
    out.extend_from_slice(&1u32.to_le_bytes());
    for p in paths {
        for u in p.as_os_str().encode_wide() {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out.extend_from_slice(&[0, 0]);
    }
    out.extend_from_slice(&[0, 0]);
    out
}

/// The paths in a `CF_HDROP`'s bytes; an empty list for anything malformed.
pub fn parse(bytes: &[u8]) -> Vec<PathBuf> {
    if bytes.len() < HEADER {
        return Vec::new();
    }
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let offset = word(0) as usize;
    let wide = word(16) != 0;
    if offset > bytes.len() {
        return Vec::new();
    }
    let list = &bytes[offset..];
    let mut out = Vec::new();
    if wide {
        let units: Vec<u16> = list
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        for name in units.split(|&u| u == 0) {
            if name.is_empty() {
                break;
            }
            out.push(PathBuf::from(std::ffi::OsString::from_wide(name)));
        }
    } else {
        // Narrow names are in the system's ANSI code page; ASCII is all that is read right.
        for name in list.split(|&b| b == 0) {
            if name.is_empty() {
                break;
            }
            out.push(PathBuf::from(String::from_utf8_lossy(name).into_owned()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let paths = vec![
            PathBuf::from(r"C:\Temp\a b.txt"),
            PathBuf::from(r"C:\Temp\ünïcode — 名前"),
            PathBuf::from(r"\\srv\share\x"),
        ];
        assert_eq!(parse(&build(&paths)), paths);
        assert!(parse(&build(&[])).is_empty());
    }

    #[test]
    fn malformed_is_empty_and_narrow_names_read() {
        assert!(parse(&[1, 2, 3]).is_empty());
        let mut bad = build(&[PathBuf::from("x")]);
        bad[0] = 0xff;
        assert!(parse(&bad).is_empty());
        let mut narrow = Vec::new();
        narrow.extend_from_slice(&20u32.to_le_bytes());
        narrow.extend_from_slice(&[0; 16]);
        narrow.extend_from_slice(b"C:\\a.txt\0C:\\b\0\0");
        assert_eq!(parse(&narrow), [PathBuf::from(r"C:\a.txt"), PathBuf::from(r"C:\b")]);
    }
}
