//! The Start menu's shortcut folders, read directly: the Apps folder is not refreshed without
//! Explorer, so apps installed since are only here.

use super::merge::Shortcut;
use std::path::{Path, PathBuf};
use windows::core::{Interface, HSTRING, PWSTR};
use windows::Win32::Foundation::MAX_PATH;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ};
use windows::Win32::UI::Shell::{
    FOLDERID_CommonStartMenu, FOLDERID_StartMenu, IShellLinkW, SHGetKnownFolderPath, ShellLink, KF_FLAG_DEFAULT,
};

/// Every `.lnk` under the per-user and common Start menu `Programs` folders, with its target.
/// COM must be initialised on this thread.
pub fn shortcuts() -> Vec<Shortcut> {
    let mut files = Vec::new();
    for id in [&FOLDERID_StartMenu, &FOLDERID_CommonStartMenu] {
        if let Some(dir) = known_folder(id) {
            collect(&dir.join("Programs"), &mut files, 0);
        }
    }
    files
        .into_iter()
        .filter_map(|path| {
            let name = path.file_stem()?.to_str()?.to_string();
            let target = unsafe { target_of(&path) }.unwrap_or_default();
            Some(Shortcut { name, path: path.to_string_lossy().into_owned(), target })
        })
        .collect()
}

fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    unsafe {
        let raw: PWSTR = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let text = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0.cast()));
        text.map(PathBuf::from)
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() && depth < 4 => collect(&path, out, depth + 1),
            Ok(t) if t.is_file() && path.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) => out.push(path),
            _ => {}
        }
    }
}

/// The file the shortcut points at, without searching for it if it moved.
unsafe fn target_of(lnk: &Path) -> Option<String> {
    let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
    let file: IPersistFile = link.cast().ok()?;
    file.Load(&HSTRING::from(lnk.as_os_str()), STGM_READ).ok()?;
    let mut buf = [0u16; MAX_PATH as usize];
    link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len])).filter(|t| !t.is_empty())
}
