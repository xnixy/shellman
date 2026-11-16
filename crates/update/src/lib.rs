//! Signed releases and the decisions around updating, kept free of I/O where it matters so they
//! can be tested: which version is newer ([`version`]), what a release signature says and whether
//! it holds ([`sig`]), whether a setup exe's payload is whole ([`payload`]), what the release feed
//! offers ([`feed`]), when to check and when to hold off ([`schedule`]), and what the last check
//! found ([`state`]).
//!
//! `shellman-update.exe` (`apps/update`) does the fetching and installing; the session only
//! decides when to start it; the release command (`xtask`) makes the signatures. See
//! `docs/PLAN.md`, "Install and update".

pub mod feed;
pub mod payload;
pub mod schedule;
pub mod sig;
pub mod state;
pub mod version;

/// Where releases live.
pub const REPO: &str = "xnixy/shellman";
/// The page of every release (the "release notes" link when no newer release is known).
pub const RELEASES_PAGE: &str = "https://github.com/xnixy/shellman/releases";
/// The latest published release (drafts and pre-releases are not "latest").
pub const FEED: &str = "https://api.github.com/repos/xnixy/shellman/releases/latest";

/// Lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Bytes from hex (either case); `None` for anything else.
pub fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// Hex SHA-256 of bytes.
pub fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex(&sha2::Sha256::digest(bytes))
}

/// Hex SHA-256 of a file, read in pieces.
pub fn sha256_file(path: &std::path::Path) -> std::io::Result<String> {
    use sha2::Digest;
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = sha2::Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(hex(&h.finalize()));
        }
        h.update(&buf[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        assert_eq!(hex(&[0, 0xab, 0x10]), "00ab10");
        assert_eq!(unhex("00AB10").unwrap(), [0, 0xab, 0x10]);
        assert!(unhex("abc").is_none());
        assert!(unhex("zz").is_none());
        assert_eq!(sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
