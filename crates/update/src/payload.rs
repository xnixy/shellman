//! Is a packed setup exe whole? The footer `shellman-installer --pack` writes (see
//! `apps/installer/src/payload.rs`, which owns the format) carries the SHA-256 of the files it
//! holds; this checks it before the exe is run, on top of the signature over the whole file.
//!
//! ```text
//! [stub exe][body][u64 body length LE][64 hex digits: SHA-256 of body][b"SHMNPAK1"]
//! ```

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const MAGIC: &[u8; 8] = b"SHMNPAK1";
const FOOTER: u64 = 8 + 64 + 8;

/// `Ok` when `exe` has a payload and its body matches the footer's hash.
pub fn check(exe: &Path) -> Result<(), String> {
    use sha2::Digest;
    let damaged = |why: &str| format!("the downloaded installer is damaged ({why})");
    let mut f = File::open(exe).map_err(|e| format!("cannot read {}: {e}", exe.display()))?;
    let size = f.metadata().map_err(|e| e.to_string())?.len();
    if size < FOOTER {
        return Err(damaged("too short"));
    }
    let mut foot = [0u8; FOOTER as usize];
    f.seek(SeekFrom::Start(size - FOOTER)).and_then(|_| f.read_exact(&mut foot)).map_err(|e| damaged(&e.to_string()))?;
    if &foot[72..] != MAGIC {
        return Err(damaged("no files inside"));
    }
    let len = u64::from_le_bytes(foot[..8].try_into().unwrap());
    let want = std::str::from_utf8(&foot[8..72]).map_err(|_| damaged("footer"))?.to_ascii_lowercase();
    let offset = (size - FOOTER).checked_sub(len).ok_or_else(|| damaged("footer"))?;
    f.seek(SeekFrom::Start(offset)).map_err(|e| damaged(&e.to_string()))?;
    let mut h = sha2::Sha256::new();
    let mut left = len;
    let mut buf = vec![0u8; 256 * 1024];
    while left > 0 {
        let n = (left as usize).min(buf.len());
        f.read_exact(&mut buf[..n]).map_err(|e| damaged(&e.to_string()))?;
        h.update(&buf[..n]);
        left -= n as u64;
    }
    if crate::hex(&h.finalize()) != want {
        return Err(damaged("its files do not match their checksum"));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn fake(body: &[u8]) -> Vec<u8> {
    let mut out = b"MZstub".to_vec();
    out.extend_from_slice(body);
    out.extend_from_slice(&(body.len() as u64).to_le_bytes());
    out.extend_from_slice(crate::sha256(body).as_bytes());
    out.extend_from_slice(MAGIC);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_damaged_and_unpacked() {
        let dir = std::env::temp_dir().join(format!("shellman-update-payload-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.exe");
        let mut bytes = fake(b"the files");
        std::fs::write(&p, &bytes).unwrap();
        check(&p).unwrap();
        bytes[8] ^= 1;
        std::fs::write(&p, &bytes).unwrap();
        assert!(check(&p).unwrap_err().contains("checksum"));
        std::fs::write(&p, b"MZ just a stub, no payload at all, long enough to have a footer-sized tail, and then some more padding here").unwrap();
        assert!(check(&p).unwrap_err().contains("no files"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
