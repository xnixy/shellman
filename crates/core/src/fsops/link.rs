//! What a shortcut (.lnk) points to, so "Open file location" can show it in Files instead of
//! handing it to Explorer.

use std::path::{Path, PathBuf};
use windows::core::{Interface, HSTRING};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    STGM_READ,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

/// The path a shortcut file points to, if it points to a file or folder (not, say, a printer).
pub fn link_target(link: &Path) -> Option<PathBuf> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let sl: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        sl.cast::<IPersistFile>()
            .ok()?
            .Load(&HSTRING::from(link.as_os_str()), STGM_READ)
            .ok()?;
        let mut buf = [0u16; 1024];
        // No resolving (which may search the disk or show a dialog): the stored path is enough.
        sl.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let path = PathBuf::from(String::from_utf16_lossy(&buf[..end]));
        (!path.as_os_str().is_empty()).then_some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_file_is_not_a_shortcut() {
        assert!(link_target(Path::new(r"C:\Windows\notepad.exe")).is_none());
    }
}
