//! A file's identity on its volume, which survives renames and moves within the volume by any
//! program: the volume's serial number and the file's 128-bit id (`FileIdInfo`). Given an
//! identity, the file's path now is found with `OpenFileById` and `GetFinalPathNameByHandleW`.
//!
//! Volumes that cannot open files by id (FAT, exFAT, most network shares) give no identity, and
//! whoever keeps one falls back to the path.

use std::os::windows::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FileIdInfo, GetFileInformationByHandleEx, GetFinalPathNameByHandleW,
    GetVolumeInformationByHandleW, OpenFileById, ExtendedFileIdType, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_128, FILE_ID_DESCRIPTOR, FILE_ID_DESCRIPTOR_0,
    FILE_ID_INFO, FILE_NAME_NORMALIZED, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};

/// `FILE_SUPPORTS_OPEN_BY_FILE_ID`, from the volume's flags.
const OPEN_BY_ID: u32 = 0x0100_0000;
/// Reading attributes is all a handle here is for.
const READ_ATTRIBUTES: u32 = 0x80;

/// A file's identity: which volume, and which file on it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FileId {
    pub volume: u64,
    pub file: u128,
}

impl FileId {
    /// As text, for a settings file: the volume and the file in hex.
    pub fn to_text(self) -> String {
        format!("{:016x}:{:032x}", self.volume, self.file)
    }

    pub fn from_text(s: &str) -> Option<Self> {
        let (v, f) = s.split_once(':')?;
        Some(Self {
            volume: u64::from_str_radix(v, 16).ok()?,
            file: u128::from_str_radix(f, 16).ok()?,
        })
    }
}

/// What is at a path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Probe {
    /// Nothing (or nothing that can be opened).
    Missing,
    /// Something, with its identity when its volume keeps stable ids.
    Exists(Option<FileId>),
}

/// A handle closed when dropped.
struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

/// A handle for reading attributes of `p` itself (a folder, or a link rather than its target).
fn open(p: &Path) -> Option<Owned> {
    let w = wide(p);
    let h = unsafe {
        CreateFileW(
            PCWSTR(w.as_ptr()),
            READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .ok()?;
    Some(Owned(h))
}

fn id_of_handle(h: &Owned) -> Option<FileId> {
    let mut flags = 0u32;
    unsafe { GetVolumeInformationByHandleW(h.0, None, None, None, Some(&mut flags), None) }.ok()?;
    if flags & OPEN_BY_ID == 0 {
        return None;
    }
    let mut info = FILE_ID_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            h.0,
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    }
    .ok()?;
    let file = u128::from_le_bytes(info.FileId.Identifier);
    (file != 0).then_some(FileId {
        volume: info.VolumeSerialNumber,
        file,
    })
}

/// What is at `p`, and its identity.
pub fn probe(p: &Path) -> Probe {
    match open(p) {
        Some(h) => Probe::Exists(id_of_handle(&h)),
        None => Probe::Missing,
    }
}

/// The identity of the file at `p`, when there is one and its volume keeps stable ids.
pub fn file_id(p: &Path) -> Option<FileId> {
    match probe(p) {
        Probe::Exists(id) => id,
        Probe::Missing => None,
    }
}

/// The root of the volume `p` is on: `C:\`, or `\\server\share\`.
pub fn volume_root(p: &Path) -> Option<PathBuf> {
    match p.components().next()? {
        Component::Prefix(pre) => {
            let mut root = PathBuf::from(pre.as_os_str());
            root.push("\\");
            Some(root)
        }
        _ => None,
    }
}

/// Whether the volume `p` is on is there now (a drive unplugged, a share offline: not).
pub fn volume_online(p: &Path) -> bool {
    volume_root(p).is_some_and(|r| r.exists())
}

/// A path as `GetFinalPathNameByHandleW` gives it, without its `\\?\` (or as `\\server\share`
/// for `\\?\UNC\server\share`).
pub fn plain_path(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        PathBuf::from(s)
    }
}

/// The local drives' roots (no network drives or optical discs, which can be slow to answer).
fn local_roots() -> Vec<PathBuf> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| PathBuf::from(format!("{}:\\", (b'A' + i) as char)))
        // 4 = DRIVE_REMOTE, 5 = DRIVE_CDROM.
        .filter(|r| !matches!(unsafe { GetDriveTypeW(&HSTRING::from(r.as_os_str())) }, 4 | 5))
        .collect()
}

/// Where the file `id` is now. `hint` is a path that was on its volume (the file's last known
/// one); when that volume is not the file's any more, every drive is tried.
pub fn path_of(id: FileId, hint: &Path) -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = volume_root(hint).into_iter().collect();
    for root in local_roots() {
        if !roots.iter().any(|r| r.as_os_str().eq_ignore_ascii_case(root.as_os_str())) {
            roots.push(root);
        }
    }
    for root in roots {
        let Some(vol) = open(&root) else { continue };
        if id_of_handle(&vol).map(|v| v.volume) != Some(id.volume) {
            continue;
        }
        let desc = FILE_ID_DESCRIPTOR {
            dwSize: std::mem::size_of::<FILE_ID_DESCRIPTOR>() as u32,
            Type: ExtendedFileIdType,
            Anonymous: FILE_ID_DESCRIPTOR_0 {
                ExtendedFileId: FILE_ID_128 {
                    Identifier: id.file.to_le_bytes(),
                },
            },
        };
        let h = unsafe {
            OpenFileById(
                vol.0,
                &desc,
                READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            )
        };
        let Ok(h) = h.map(Owned) else { return None };
        let mut buf = vec![0u16; 1024];
        loop {
            let n = unsafe { GetFinalPathNameByHandleW(h.0, &mut buf, FILE_NAME_NORMALIZED) } as usize;
            if n == 0 {
                return None;
            }
            if n < buf.len() {
                return Some(plain_path(&String::from_utf16_lossy(&buf[..n])));
            }
            buf.resize(n + 1, 0);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_as_text() {
        let id = FileId {
            volume: 0x1234_abcd,
            file: 0xdead_beef_0000_0000_0000_0000_0000_0042,
        };
        assert_eq!(FileId::from_text(&id.to_text()), Some(id));
        assert_eq!(FileId::from_text("nonsense"), None);
        assert_eq!(FileId::from_text("12:zz"), None);
    }

    #[test]
    fn final_paths_lose_their_prefix() {
        assert_eq!(plain_path(r"\\?\C:\a\b.txt"), PathBuf::from(r"C:\a\b.txt"));
        assert_eq!(plain_path(r"\\?\UNC\srv\share\x"), PathBuf::from(r"\\srv\share\x"));
        assert_eq!(plain_path(r"C:\plain"), PathBuf::from(r"C:\plain"));
    }

    #[test]
    fn volume_roots() {
        assert_eq!(volume_root(Path::new(r"C:\a\b")), Some(PathBuf::from(r"C:\")));
        assert_eq!(
            volume_root(Path::new(r"\\srv\share\a")),
            Some(PathBuf::from(r"\\srv\share\"))
        );
        assert_eq!(volume_root(Path::new(r"relative\x")), None);
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shellman-id-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn an_identity_follows_renames_and_moves_by_anyone() {
        let d = temp("follow");
        let a = d.join("a.txt");
        std::fs::write(&a, "x").unwrap();
        let Some(id) = file_id(&a) else {
            // The temp folder is not on a volume with ids; nothing to show here.
            let _ = std::fs::remove_dir_all(&d);
            return;
        };
        // Renamed behind our back.
        let b = d.join("renamed.txt");
        std::fs::rename(&a, &b).unwrap();
        assert_eq!(probe(&a), Probe::Missing);
        assert_eq!(file_id(&b), Some(id));
        let found = path_of(id, &a).unwrap();
        assert!(found.as_os_str().eq_ignore_ascii_case(b.as_os_str()), "{found:?}");
        // Moved to another folder.
        let sub = d.join("sub");
        std::fs::create_dir(&sub).unwrap();
        let c = sub.join("moved.txt");
        std::fs::rename(&b, &c).unwrap();
        let found = path_of(id, &b).unwrap();
        assert!(found.as_os_str().eq_ignore_ascii_case(c.as_os_str()), "{found:?}");
        // Folders have identities too.
        let sid = file_id(&sub).unwrap();
        let sub2 = d.join("sub2");
        std::fs::rename(&sub, &sub2).unwrap();
        assert!(path_of(sid, &sub).unwrap().as_os_str().eq_ignore_ascii_case(sub2.as_os_str()));
        // Deleted: gone.
        std::fs::remove_file(sub2.join("moved.txt")).unwrap();
        assert_eq!(path_of(id, &c), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_replace_on_save_is_a_new_file_at_the_same_path() {
        let d = temp("save");
        let a = d.join("doc.txt");
        std::fs::write(&a, "one").unwrap();
        let Some(before) = file_id(&a) else {
            let _ = std::fs::remove_dir_all(&d);
            return;
        };
        // An editor writes a new file and moves it over the old one.
        let tmp = d.join("doc.txt~new");
        std::fs::write(&tmp, "two").unwrap();
        std::fs::rename(&tmp, &a).unwrap();
        let after = file_id(&a).unwrap();
        assert_ne!(before, after);
        assert_eq!(path_of(before, &a), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
