//! The release signature: a short statement naming the version, the setup exe's file name, size
//! and SHA-256, signed with Ed25519. Each release has `shellman-X.Y.Z-setup.exe` and beside it
//! `shellman-X.Y.Z-setup.exe.sig`:
//!
//! ```text
//! shellman release v1
//! version 0.2.0
//! file shellman-0.2.0-setup.exe
//! size 31457280
//! sha256 <64 hex digits>
//! signature <128 hex digits>
//! ```
//!
//! The signature covers every byte of the lines before `signature` (each ending in `\n`). Signing
//! the version binds it: an old release served as the newest still says its own version, and is
//! refused as not newer ([`crate::version`]). The private key is in the maintainer's 1Password
//! only (see `docs/PLAN.md`); [`PUBLIC_KEY`] is compiled in.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

/// The release signing key's public half (Ed25519), made by `cargo xtask keygen` on 15 Nov 2026.
/// Fingerprint 0e5f:c242:acb4:9c9f:3192:2ea2:71b6:d4bf. Its private half is the
/// `SHELLMAN_RELEASE_SIGNING_KEY` secret of the maintainer's 1Password Environment
/// `shellman/release-signing`, and nowhere else.
pub const PUBLIC_KEY: [u8; 32] = [
    0x83, 0x24, 0xf3, 0xb4, 0xfc, 0xdd, 0x18, 0x1f, 0x1b, 0xd6, 0xe2, 0x10, 0xb8, 0xa6, 0xbb, 0x3e,
    0xbe, 0xd4, 0x45, 0x7e, 0x03, 0x2f, 0xf7, 0xec, 0x03, 0x19, 0x55, 0xf6, 0x96, 0xc0, 0x12, 0x19,
];

const HEADER: &str = "shellman release v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    pub version: String,
    pub file: String,
    pub size: u64,
    pub sha256: String,
}

impl Statement {
    /// The signed text.
    pub fn text(&self) -> String {
        format!("{HEADER}\nversion {}\nfile {}\nsize {}\nsha256 {}\n", self.version, self.file, self.size, self.sha256)
    }
}

/// The `.sig` file for `st`, signed with the 32-byte secret seed.
pub fn sign(st: &Statement, seed: &[u8; 32]) -> String {
    let key = SigningKey::from_bytes(seed);
    let text = st.text();
    let sig = key.sign(text.as_bytes());
    format!("{text}signature {}\n", crate::hex(&sig.to_bytes()))
}

/// The public key of a secret seed.
pub fn public_of(seed: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(seed).verifying_key().to_bytes()
}

/// A short, readable name for a public key: the first 16 bytes of its SHA-256, in groups.
pub fn fingerprint(key: &[u8; 32]) -> String {
    let h = crate::sha256(key);
    h[..32].as_bytes().chunks(4).map(|c| std::str::from_utf8(c).unwrap_or("")).collect::<Vec<_>>().join(":")
}

/// The keys a signature may be made with: [`PUBLIC_KEY`]; in a debug build, also the hex key in
/// `SHELLMAN_UPDATE_TEST_KEY` (tests against a local fake release). A release build never reads
/// that variable.
pub fn trusted_keys() -> Vec<[u8; 32]> {
    #[cfg_attr(not(debug_assertions), allow(unused_mut))]
    let mut keys = vec![PUBLIC_KEY];
    #[cfg(debug_assertions)]
    if let Some(k) = std::env::var("SHELLMAN_UPDATE_TEST_KEY").ok().and_then(|s| crate::unhex(&s)).and_then(|v| <[u8; 32]>::try_from(v).ok()) {
        keys.push(k);
    }
    keys
}

/// Read a `.sig` file and check its signature against `keys`. The statement it returns is then
/// to be checked against the file itself ([`Statement::matches`]).
pub fn verify(sig_file: &str, keys: &[[u8; 32]]) -> Result<Statement, String> {
    let bad = |why: &str| Err(format!("the release signature is not valid ({why})"));
    let Some(at) = sig_file.find("\nsignature ") else { return bad("no signature line") };
    let (text, rest) = sig_file.split_at(at + 1);
    let Some(hexsig) = rest.strip_prefix("signature ").map(|s| s.strip_suffix('\n').unwrap_or(s)) else { return bad("no signature line") };
    let Some(sig) = crate::unhex(hexsig).and_then(|v| <[u8; 64]>::try_from(v).ok()) else { return bad("the signature is not 64 bytes of hex") };
    let sig = Signature::from_bytes(&sig);
    let signed = keys.iter().filter(|k| **k != [0; 32]).any(|k| VerifyingKey::from_bytes(k).is_ok_and(|vk| vk.verify_strict(text.as_bytes(), &sig).is_ok()));
    if !signed {
        return bad("it was not made with shellman's release key");
    }
    // Only now is the text trusted enough to read.
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return bad("unknown format");
    }
    let mut field = |name: &str| lines.next().and_then(|l| l.strip_prefix(name)).and_then(|l| l.strip_prefix(' ')).map(str::to_string);
    let (Some(version), Some(file), Some(size), Some(sha256)) = (field("version"), field("file"), field("size"), field("sha256")) else {
        return bad("missing fields");
    };
    let Ok(size) = size.parse() else { return bad("size") };
    if lines.next().is_some() {
        return bad("extra lines");
    }
    Ok(Statement { version, file, size, sha256: sha256.to_ascii_lowercase() })
}

impl Statement {
    /// Whether a downloaded file is the one signed for.
    pub fn matches(&self, file_name: &str, size: u64, sha256: &str) -> Result<(), String> {
        if !self.file.eq_ignore_ascii_case(file_name) {
            return Err(format!("the signature is for {}, not {file_name}", self.file));
        }
        if self.size != size {
            return Err(format!("the download is {size} bytes; the signature says {}", self.size));
        }
        if !self.sha256.eq_ignore_ascii_case(sha256) {
            return Err("the download does not match its signature (SHA-256 differs)".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: [u8; 32] = [7; 32];

    fn st() -> Statement {
        Statement { version: "0.2.0".into(), file: "shellman-0.2.0-setup.exe".into(), size: 1234, sha256: "ab".repeat(32) }
    }

    #[test]
    fn a_signature_round_trips() {
        let file = sign(&st(), &SEED);
        assert!(file.starts_with("shellman release v1\nversion 0.2.0\n"), "{file}");
        let got = verify(&file, &[public_of(&SEED)]).unwrap();
        assert_eq!(got, st());
        got.matches("shellman-0.2.0-setup.exe", 1234, &"AB".repeat(32)).unwrap();
    }

    #[test]
    fn a_changed_statement_or_another_key_is_refused() {
        let file = sign(&st(), &SEED);
        let other = public_of(&[8; 32]);
        assert!(verify(&file, &[other]).is_err());
        assert!(verify(&file.replace("0.2.0", "9.9.9"), &[public_of(&SEED)]).is_err(), "the version is signed");
        assert!(verify(&file.replace("size 1234", "size 1235"), &[public_of(&SEED)]).is_err());
        assert!(verify(&file, &[[0; 32]]).is_err(), "an unset key trusts nothing");
        assert!(verify("", &[public_of(&SEED)]).is_err());
        assert!(verify(&file.replace("signature ", "signature 00"), &[public_of(&SEED)]).is_err());
    }

    #[test]
    fn the_file_must_match() {
        let s = st();
        assert!(s.matches("shellman-0.1.0-setup.exe", 1234, &"ab".repeat(32)).is_err());
        assert!(s.matches("shellman-0.2.0-setup.exe", 1, &"ab".repeat(32)).is_err());
        assert!(s.matches("shellman-0.2.0-setup.exe", 1234, &"cd".repeat(32)).is_err());
    }

    #[test]
    fn fingerprints_are_short_and_stable() {
        let f = fingerprint(&public_of(&SEED));
        assert_eq!(f.len(), 39);
        assert_eq!(f, fingerprint(&public_of(&SEED)));
    }

    #[test]
    fn the_compiled_in_key_is_a_real_key() {
        assert_ne!(PUBLIC_KEY, [0; 32], "run `cargo xtask keygen` and put its public key here");
        assert!(VerifyingKey::from_bytes(&PUBLIC_KEY).is_ok());
        assert_eq!(fingerprint(&PUBLIC_KEY), "0e5f:c242:acb4:9c9f:3192:2ea2:71b6:d4bf", "the fingerprint in the docs");
    }
}
