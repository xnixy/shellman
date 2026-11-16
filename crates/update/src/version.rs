//! Versions, and which is newer.
//!
//! A release is a tag `vX.Y.Z` (no pre-release tags). A build's version comes from its stamp
//! (`shellman_buildinfo::STAMP`):
//!
//! - a release build is stamped `X.Y.Z <date>` by the release command;
//! - a developer build is `<crate version>+<git describe> <date>`. When `git describe` starts at a
//!   release tag (`v0.2.0-5-gabc1234`) the build is 0.2.0 plus 5 commits, so it is newer than
//!   0.2.0 and only a later release replaces it; with no tag it is the crate version.
//!
//! A release replaces an install only when its X.Y.Z is greater: the same version is never
//! reinstalled and an older one (a downgrade, even a validly signed one) never installed.

use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    /// Commits past the tag (developer builds); 0 for a release.
    pub ahead: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self { major, minor, patch, ahead: 0 }
    }

    /// `X.Y.Z`, strictly: three numbers, nothing else.
    pub fn parse(s: &str) -> Option<Self> {
        let mut it = s.split('.');
        let mut num = || -> Option<u32> {
            let p = it.next()?;
            (!p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit())).then(|| p.parse().ok())?
        };
        let v = Self::new(num()?, num()?, num()?);
        it.next().is_none().then_some(v)
    }

    /// A release tag: `vX.Y.Z`.
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::parse(tag.strip_prefix('v')?)
    }

    /// The version a build stamp says; `None` when it does not start with one.
    pub fn from_stamp(stamp: &str) -> Option<Self> {
        let first = stamp.split_whitespace().next()?;
        let (base, describe) = match first.split_once('+') {
            Some((b, d)) => (b, Some(d)),
            None => (first, None),
        };
        if let Some(d) = describe.and_then(|d| d.strip_prefix('v')) {
            // `0.2.0` or `0.2.0-5-gabc1234`.
            let mut parts = d.splitn(2, '-');
            if let Some(mut v) = parts.next().and_then(Self::parse) {
                if let Some(rest) = parts.next() {
                    v.ahead = rest.split('-').next().and_then(|n| n.parse().ok()).unwrap_or(0);
                }
                return Some(v);
            }
        }
        Self::parse(base)
    }

    fn triple(&self) -> (u32, u32, u32) {
        (self.major, self.minor, self.patch)
    }

    /// Whether `self` (a release) should replace `installed`.
    pub fn replaces(&self, installed: &Version) -> bool {
        self.triple().cmp(&installed.triple()) == Ordering::Greater
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if self.ahead > 0 {
            write!(f, "+{}", self.ahead)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_parsing() {
        assert_eq!(Version::parse("0.2.10"), Some(Version::new(0, 2, 10)));
        for bad in ["", "1", "1.2", "1.2.3.4", "1.2.x", "1..3", "-1.2.3", "1.2.3-beta", " 1.2.3", "1.2.3 "] {
            assert_eq!(Version::parse(bad), None, "{bad:?}");
        }
        assert_eq!(Version::from_tag("v1.0.0"), Some(Version::new(1, 0, 0)));
        assert_eq!(Version::from_tag("1.0.0"), None);
        assert_eq!(Version::from_tag("v1.0.0-rc1"), None);
    }

    #[test]
    fn stamps() {
        assert_eq!(Version::from_stamp("0.2.0 2026-11-20"), Some(Version::new(0, 2, 0)));
        assert_eq!(Version::from_stamp("0.1.0+1d46382 2026-11-15"), Some(Version::new(0, 1, 0)));
        assert_eq!(Version::from_stamp("0.1.0+v0.3.1 2026-11-15"), Some(Version::new(0, 3, 1)));
        assert_eq!(Version::from_stamp("0.1.0+v0.3.1-5-gabc1234 2026-11-15"), Some(Version { ahead: 5, ..Version::new(0, 3, 1) }));
        assert_eq!(Version::from_stamp("0.1.0"), Some(Version::new(0, 1, 0)));
        assert_eq!(Version::from_stamp("garbage"), None);
        assert_eq!(Version::from_stamp(""), None);
    }

    #[test]
    fn only_a_greater_release_replaces() {
        let v = |s| Version::parse(s).unwrap();
        assert!(v("0.2.0").replaces(&v("0.1.9")));
        assert!(v("0.10.0").replaces(&v("0.9.0")), "numbers, not text");
        assert!(v("1.0.0").replaces(&v("0.99.99")));
        assert!(!v("0.2.0").replaces(&v("0.2.0")), "the same version is not reinstalled");
        assert!(!v("0.1.0").replaces(&v("0.2.0")), "never a downgrade");
        let dev = Version::from_stamp("0.1.0+v0.2.0-3-gabc 2026-11-15").unwrap();
        assert!(!v("0.2.0").replaces(&dev), "a build past a tag is newer than that release");
        assert!(v("0.2.1").replaces(&dev));
        assert_eq!(dev.to_string(), "0.2.0+3");
    }
}
