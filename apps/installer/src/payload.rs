//! The files inside a packed installer. `shellman-installer.exe` as built is the stub: no files.
//! `--pack` appends them to a copy of it:
//!
//! ```text
//! [stub exe][body][footer]
//! body   = deflate( entry* ), entry = u16 name length, name (UTF-8, `\`-separated), u64 size, bytes
//! footer = u64 body length (LE), 64 hex digits of the body's SHA-256, b"SHMNPAK1"
//! ```
//!
//! The entries are what `shellman-setup install` would copy from the source folder (its
//! manifest, `shellman_setup::manifest::build`), at their install paths, plus a generated
//! `install-manifest` naming the ones that are not `shellman-*` files; extracted into an empty
//! folder they make a source folder `install` / `update` read as they would a release folder.
//! A body whose hash does not match is refused before anything is written.

use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use shellman_setup::{manifest, paths, sha256};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"SHMNPAK1";
const FOOTER: u64 = 8 + 64 + 8;

/// Where the body sits in an exe.
#[derive(Clone, Debug)]
pub struct Payload {
    pub exe: PathBuf,
    pub offset: u64,
    pub len: u64,
    pub hash: String,
}

/// The payload of `exe`, if it has one.
pub fn find(exe: &Path) -> Option<Payload> {
    let mut f = File::open(exe).ok()?;
    let size = f.metadata().ok()?.len();
    if size < FOOTER {
        return None;
    }
    f.seek(SeekFrom::Start(size - FOOTER)).ok()?;
    let mut foot = [0u8; FOOTER as usize];
    f.read_exact(&mut foot).ok()?;
    if &foot[72..] != MAGIC {
        return None;
    }
    let len = u64::from_le_bytes(foot[..8].try_into().ok()?);
    let hash = std::str::from_utf8(&foot[8..72]).ok()?.to_string();
    let offset = (size - FOOTER).checked_sub(len)?;
    Some(Payload { exe: exe.to_path_buf(), offset, len, hash })
}

/// How long the exe is without its payload (the stub).
pub fn stub_len(exe: &Path) -> io::Result<u64> {
    Ok(match find(exe) {
        Some(p) => p.offset,
        None => std::fs::metadata(exe)?.len(),
    })
}

fn read_exact_at(f: &mut File, offset: u64, len: u64) -> io::Result<Vec<u8>> {
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; len as usize];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

/// Unpack into `dir` (made empty first). `progress(done, total)` in bytes of the body. Returns
/// the files written, relative.
pub fn extract(p: &Payload, dir: &Path, progress: &mut dyn FnMut(u64, u64)) -> Result<Vec<String>, String> {
    let mut f = File::open(&p.exe).map_err(|e| format!("cannot read {}: {e}", p.exe.display()))?;
    let body = read_exact_at(&mut f, p.offset, p.len).map_err(|e| format!("the installer is damaged ({e})"))?;
    if sha256::of_bytes(&body) != p.hash {
        return Err("the installer is damaged (its files do not match their checksum); download it again".into());
    }
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let total = body.len() as u64;
    let mut z = DeflateDecoder::new(&body[..]);
    let mut out = Vec::new();
    loop {
        let mut n = [0u8; 2];
        match z.read_exact(&mut n) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("the installer is damaged ({e})")),
        }
        let mut name = vec![0u8; u16::from_le_bytes(n) as usize];
        let mut size = [0u8; 8];
        z.read_exact(&mut name).and_then(|_| z.read_exact(&mut size)).map_err(|e| format!("the installer is damaged ({e})"))?;
        let name = String::from_utf8(name).map_err(|_| "the installer is damaged (a file name)".to_string())?;
        // A checked relative name: the payload cannot write outside `dir`.
        let parts = paths::components(&name)?;
        let mut path = dir.to_path_buf();
        for part in &parts {
            path.push(part);
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        let mut file = File::create(&path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        let size = u64::from_le_bytes(size);
        let copied = io::copy(&mut (&mut z).take(size), &mut file).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        if copied != size {
            return Err("the installer is damaged (a file is cut short)".into());
        }
        out.push(parts.join("\\"));
        progress(z.total_in(), total);
    }
    Ok(out)
}

/// Build a packed installer at `out` from the source folder `from`, using `stub` (this exe) as the
/// program. Returns `(files, bytes of payload)`.
pub fn pack(stub: &Path, from: &Path, out: &Path) -> Result<(usize, u64), String> {
    let built = manifest::build(from, &[]);
    for w in &built.warnings {
        println!("  warning: {w}");
    }
    let rels: Vec<&str> = built.entries.iter().map(|e| e.rel.as_str()).collect();
    for need in ["shellman-session.exe", "shellman-bar.exe", "shellman-installer.exe"] {
        if !rels.iter().any(|r| r.eq_ignore_ascii_case(need)) {
            return Err(format!("{need} is not in {}; build the workspace first (cargo build --release --workspace)", from.display()));
        }
    }
    if out.parent().is_some_and(|d| shellman_setup::ops::same_dir(d, from)) {
        return Err("write the packed installer outside the source folder, or the next pack would put it inside itself".into());
    }
    let mut body = DeflateEncoder::new(Vec::new(), Compression::best());
    let mut extra = String::from("# written by shellman-installer --pack: the files that are not shellman-* files\n");
    for e in &built.entries {
        let data = std::fs::read(&e.src).map_err(|err| format!("cannot read {}: {err}", e.src.display()))?;
        put(&mut body, &e.rel, &data).map_err(|err| err.to_string())?;
        if !is_core(&e.rel) {
            extra.push_str(&format!("{} -> {}\n", e.rel, e.rel));
        }
        println!("  {:<34} {:>10} bytes", e.rel, data.len());
    }
    put(&mut body, manifest::MANIFEST_FILE, extra.as_bytes()).map_err(|e| e.to_string())?;
    let body = body.finish().map_err(|e| e.to_string())?;
    let hash = sha256::of_bytes(&body);

    let len = stub_len(stub).map_err(|e| format!("cannot read {}: {e}", stub.display()))?;
    let mut program = vec![0u8; len as usize];
    File::open(stub).and_then(|mut f| f.read_exact(&mut program)).map_err(|e| format!("cannot read {}: {e}", stub.display()))?;
    if let Some(d) = out.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = out.with_extension("tmp");
    let write = || -> io::Result<()> {
        let mut f = File::create(&tmp)?;
        f.write_all(&program)?;
        f.write_all(&body)?;
        f.write_all(&(body.len() as u64).to_le_bytes())?;
        f.write_all(hash.as_bytes())?;
        f.write_all(MAGIC)?;
        f.sync_all()
    };
    write().map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, out).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    Ok((built.entries.len(), body.len() as u64))
}

/// What `manifest::build` finds in a folder by itself (so the generated manifest lists the rest).
fn is_core(rel: &str) -> bool {
    let n = rel.to_ascii_lowercase();
    !n.contains('\\')
        && ((n.starts_with("shellman-") && n.ends_with(".exe"))
            || n == "shellman_pickhook.dll"
            || n.starts_with("license")
            || n.starts_with("notice")
            || n.starts_with("third-party"))
}

fn put(w: &mut impl Write, name: &str, data: &[u8]) -> io::Result<()> {
    w.write_all(&(name.len() as u16).to_le_bytes())?;
    w.write_all(name.as_bytes())?;
    w.write_all(&(data.len() as u64).to_le_bytes())?;
    w.write_all(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shellman-payload-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_pack_extracts_to_what_install_reads() {
        let src = tmp("src");
        for f in ["shellman-session.exe", "shellman-bar.exe", "shellman-installer.exe", "LICENSE"] {
            std::fs::write(src.join(f), f.as_bytes()).unwrap();
        }
        std::fs::create_dir_all(src.join("extras").join("sub")).unwrap();
        std::fs::write(src.join("extras").join("sub").join("x.dll"), b"extra").unwrap();
        let stub = src.join("shellman-installer.exe");
        let out_dir = tmp("out");
        let out = out_dir.join("packed.exe");
        let (n, _) = pack(&stub, &src, &out).unwrap();
        assert_eq!(n, 5);
        let p = find(&out).unwrap();
        assert_eq!(p.offset, "shellman-installer.exe".len() as u64);
        assert!(find(&stub).is_none());
        // Packing from a packed exe keeps only its program.
        assert_eq!(stub_len(&out).unwrap(), p.offset);
        let dest = tmp("dest");
        let files = extract(&p, &dest, &mut |_, _| {}).unwrap();
        assert!(files.iter().any(|f| f == "sub\\x.dll"), "{files:?}");
        assert_eq!(std::fs::read(dest.join("sub").join("x.dll")).unwrap(), b"extra");
        let again = manifest::build(&dest, &[]);
        let mut rels: Vec<_> = again.entries.iter().map(|e| e.rel.clone()).collect();
        rels.sort();
        assert_eq!(rels, ["LICENSE", "shellman-bar.exe", "shellman-installer.exe", "shellman-session.exe", "sub\\x.dll"], "{:?}", again.warnings);
        for d in [src, out_dir, dest] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn a_damaged_payload_is_refused() {
        let src = tmp("dmg");
        for f in ["shellman-session.exe", "shellman-bar.exe", "shellman-installer.exe"] {
            std::fs::write(src.join(f), f.as_bytes()).unwrap();
        }
        let out_dir = tmp("dmg-out");
        let out = out_dir.join("packed.exe");
        pack(&src.join("shellman-installer.exe"), &src, &out).unwrap();
        let mut bytes = std::fs::read(&out).unwrap();
        let p = find(&out).unwrap();
        bytes[p.offset as usize + 3] ^= 0xff;
        std::fs::write(&out, bytes).unwrap();
        let dest = tmp("dmg-dest");
        assert!(extract(&find(&out).unwrap(), &dest, &mut |_, _| {}).unwrap_err().contains("damaged"));
        for d in [src, out_dir, dest] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}
