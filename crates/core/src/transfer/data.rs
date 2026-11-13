//! Data objects: the shell's own for a set of files (what Explorer puts on the clipboard and
//! drags), and reading what another app's data object offers.

use super::{dib, dropfiles, Offer, Payload};
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use windows::core::{Result, HSTRING, PCWSTR};
use windows::Win32::Foundation::{GlobalFree, HGLOBAL};
use windows::Win32::System::Com::{
    CoTaskMemFree, IDataObject, IStream, DVASPECT_CONTENT, FORMATETC, STGMEDIUM, STGMEDIUM_0,
    STREAM_SEEK_SET, TYMED_HGLOBAL, TYMED_ISTREAM,
};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{ReleaseStgMedium, CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{SHCreateShellItemArrayFromIDLists, SHParseDisplayName, BHID_DataObject};

/// The formats Explorer and its kin use to say what a paste or drop did, or should do.
pub const PREFERRED_EFFECT: &str = "Preferred DropEffect";
pub const PERFORMED_EFFECT: &str = "Performed DropEffect";
pub const LOGICAL_EFFECT: &str = "Logical Performed DropEffect";
pub const PASTE_SUCCEEDED: &str = "Paste Succeeded";
pub const DROP_DESCRIPTION: &str = "DropDescription";
/// Browsers and image editors put pictures on the clipboard as PNG bytes under this name.
pub const PNG: &str = "PNG";

/// A registered clipboard format's number.
pub fn format(name: &str) -> u16 {
    unsafe { RegisterClipboardFormatW(&HSTRING::from(name)) as u16 }
}

fn etc(cf: u16, tymed: i32) -> FORMATETC {
    FORMATETC {
        cfFormat: cf,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: tymed as u32,
    }
}

/// An `HGLOBAL`'s bytes.
pub fn global_bytes(h: HGLOBAL) -> Option<Vec<u8>> {
    unsafe {
        let p = GlobalLock(h);
        if p.is_null() {
            return None;
        }
        let n = GlobalSize(h);
        let out = std::slice::from_raw_parts(p as *const u8, n).to_vec();
        let _ = GlobalUnlock(h);
        Some(out)
    }
}

/// New moveable memory holding `bytes`; whoever takes it frees it.
pub fn global_from(bytes: &[u8]) -> Option<HGLOBAL> {
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).ok()?;
        let p = GlobalLock(h);
        if p.is_null() {
            let _ = GlobalFree(Some(h));
            return None;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
        let _ = GlobalUnlock(h);
        Some(h)
    }
}

fn stream_bytes(s: &IStream) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    unsafe {
        let _ = s.Seek(0, STREAM_SEEK_SET, None);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut read = 0u32;
            let hr = s.Read(buf.as_mut_ptr().cast(), buf.len() as u32, Some(&mut read));
            if hr.is_err() {
                return None;
            }
            if read == 0 {
                break;
            }
            out.extend_from_slice(&buf[..read as usize]);
            if out.len() > 512 * 1024 * 1024 {
                return None;
            }
        }
    }
    Some(out)
}

/// The bytes a data object has in format `cf`, in memory or as a stream.
pub fn get_bytes(data: &IDataObject, cf: u16) -> Option<Vec<u8>> {
    let want = etc(cf, TYMED_HGLOBAL.0 | TYMED_ISTREAM.0);
    let mut medium = unsafe { data.GetData(&want) }.ok()?;
    let out = unsafe {
        if medium.tymed == TYMED_HGLOBAL.0 as u32 {
            global_bytes(medium.u.hGlobal)
        } else if medium.tymed == TYMED_ISTREAM.0 as u32 {
            medium.u.pstm.as_ref().and_then(stream_bytes)
        } else {
            None
        }
    };
    unsafe { ReleaseStgMedium(&mut medium) };
    out
}

/// Whether a data object has format `cf`.
pub fn has(data: &IDataObject, cf: u16) -> bool {
    unsafe { data.QueryGetData(&etc(cf, TYMED_HGLOBAL.0 | TYMED_ISTREAM.0)) }.is_ok()
}

/// Give a data object `bytes` in format `cf` (it keeps them).
pub fn set_bytes(data: &IDataObject, cf: u16, bytes: &[u8]) -> Result<()> {
    let Some(h) = global_from(bytes) else {
        return Err(windows::core::Error::from_win32());
    };
    let medium = STGMEDIUM {
        tymed: TYMED_HGLOBAL.0 as u32,
        u: STGMEDIUM_0 { hGlobal: h },
        pUnkForRelease: std::mem::ManuallyDrop::new(None),
    };
    let r = unsafe { data.SetData(&etc(cf, TYMED_HGLOBAL.0), &medium, true) };
    if r.is_err() {
        unsafe {
            let _ = GlobalFree(Some(h));
        }
    }
    r
}

/// A 32-bit number in a named format (the drop effects).
pub fn get_dword(data: &IDataObject, name: &str) -> Option<u32> {
    let b = get_bytes(data, format(name))?;
    Some(u32::from_le_bytes(b.get(..4)?.try_into().ok()?))
}

pub fn set_dword(data: &IDataObject, name: &str, v: u32) -> Result<()> {
    set_bytes(data, format(name), &v.to_le_bytes())
}

/// The shell's data object for these files and folders: what Explorer itself would drag or
/// copy, with every format other apps look for. The items may be in different folders.
pub fn for_paths(paths: &[PathBuf]) -> Result<IDataObject> {
    let mut pidls: Vec<*mut ITEMIDLIST> = Vec::with_capacity(paths.len());
    let mut result = Ok(());
    for p in paths {
        let w: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let mut pidl = std::ptr::null_mut();
        match unsafe { SHParseDisplayName(PCWSTR(w.as_ptr()), None, &mut pidl, 0, None) } {
            Ok(()) => pidls.push(pidl),
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    let data = result.and_then(|()| unsafe {
        let list: Vec<*const ITEMIDLIST> = pidls.iter().map(|p| *p as *const _).collect();
        let items = SHCreateShellItemArrayFromIDLists(&list)?;
        items.BindToHandler::<_, IDataObject>(None, &BHID_DataObject)
    });
    for p in pidls {
        unsafe { CoTaskMemFree(Some(p as *const _)) };
    }
    data
}

/// The files a data object carries (`CF_HDROP`), if any.
pub fn files(data: &IDataObject) -> Vec<PathBuf> {
    get_bytes(data, CF_HDROP.0)
        .map(|b| dropfiles::parse(&b))
        .unwrap_or_default()
}

/// What a data object offers: its files, and whether a picture or text is there.
pub fn offer(data: &IDataObject) -> Offer {
    Offer {
        files: files(data),
        image: has(data, format(PNG)) || has(data, CF_DIBV5.0) || has(data, CF_DIB.0),
        text: has(data, CF_UNICODETEXT.0),
    }
}

/// Text from UTF-16 bytes, up to the first NUL.
pub fn text_from_wide(b: &[u8]) -> String {
    let units: Vec<u16> = b
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// A picture's PNG bytes from what a format held: PNG as it is, a DIB converted.
pub fn picture(png_bytes: Option<Vec<u8>>, dib_bytes: impl FnOnce() -> Option<Vec<u8>>) -> Option<Vec<u8>> {
    if let Some(b) = png_bytes.filter(|b| dib::is_png(b)) {
        return Some(b);
    }
    dib_bytes().and_then(|b| dib::decode(&b)).and_then(|img| dib::png(&img))
}

/// What a data object carries, read in full: files first, then a picture, then text.
pub fn payload(data: &IDataObject) -> Payload {
    let files = files(data);
    if !files.is_empty() {
        return Payload::Files(files);
    }
    let pic = picture(get_bytes(data, format(PNG)), || {
        get_bytes(data, CF_DIBV5.0).or_else(|| get_bytes(data, CF_DIB.0))
    });
    if let Some(png) = pic {
        return Payload::Image(png);
    }
    match get_bytes(data, CF_UNICODETEXT.0).map(|b| text_from_wide(&b)) {
        Some(t) if !t.is_empty() => Payload::Text(t),
        _ => Payload::Nothing,
    }
}

/// `DROPIMAGETYPE`s for a drop description.
pub fn image_type(effect: u32) -> i32 {
    match effect {
        super::COPY => 1,
        super::MOVE => 2,
        super::LINK => 4,
        _ => 0,
    }
}

/// The bytes of a `DROPDESCRIPTION`: an image type, a message (with `%1` where the insert
/// goes), and the insert.
pub fn description_bytes(kind: i32, message: &str, insert: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 260 * 4);
    out.extend_from_slice(&kind.to_le_bytes());
    for s in [message, insert] {
        let mut units: Vec<u16> = s.encode_utf16().take(259).collect();
        units.resize(260, 0);
        for u in units {
            out.extend_from_slice(&u.to_le_bytes());
        }
    }
    out
}

/// Tell the drag image what a drop here would do ("Move to Pictures"); `None` clears it.
pub fn describe(data: &IDataObject, what: Option<(u32, &str)>) {
    let bytes = match what {
        Some((effect, insert)) if effect != super::NONE => {
            description_bytes(image_type(effect), super::verb(effect), insert)
        }
        // DROPIMAGE_INVALID: the shell's own default.
        _ => description_bytes(-1, "", ""),
    };
    let _ = set_bytes(data, format(DROP_DESCRIPTION), &bytes);
    // The shell's drag image window ("DragWindow" in the data object) redraws its line when
    // told (DDWM_UPDATEWINDOW = WM_USER + 3).
    if let Some(h) = get_dword(data, "DragWindow") {
        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_USER};
        unsafe {
            let _ = PostMessageW(Some(HWND(h as usize as *mut _)), WM_USER + 3, WPARAM(0), LPARAM(0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_are_fixed_size_and_cut_long_text() {
        let b = description_bytes(2, "Move to %1", "Pictures");
        assert_eq!(b.len(), 4 + 520 + 520);
        assert_eq!(&b[..4], &2i32.to_le_bytes());
        assert_eq!(text_from_wide(&b[4..524]), "Move to %1");
        assert_eq!(text_from_wide(&b[524..]), "Pictures");
        let long = "x".repeat(400);
        let b = description_bytes(1, &long, "");
        assert_eq!(text_from_wide(&b[4..524]).len(), 259, "room is left for the NUL");
    }

    #[test]
    fn png_bytes_win_over_a_dib() {
        let png = dib::png(&dib::Rgba { width: 1, height: 1, pixels: vec![0, 0, 0, 255] }).unwrap();
        assert_eq!(picture(Some(png.clone()), || None), Some(png));
        // Not PNG after all: the DIB is used.
        assert_eq!(picture(Some(vec![1, 2, 3]), || None), None);
    }

    /// The shell's data object for real files carries them as `CF_HDROP`, which is how other
    /// apps read them: built and read back without the clipboard.
    #[test]
    fn the_shells_data_object_carries_the_files() {
        unsafe {
            let _ = windows::Win32::System::Ole::OleInitialize(None);
        }
        let dir = std::env::temp_dir().join(format!("shellman-data-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("one")).unwrap();
        std::fs::create_dir_all(dir.join("two")).unwrap();
        let a = dir.join("one").join("a.txt");
        let b = dir.join("two").join("b folder");
        std::fs::write(&a, "a").unwrap();
        std::fs::create_dir_all(&b).unwrap();
        // Items from two folders, as a tag place has them.
        let data = for_paths(&[a.clone(), b.clone()]).unwrap();
        let got = files(&data);
        let keys: Vec<String> = got.iter().map(|p| super::super::path_key(p)).collect();
        assert_eq!(keys, [super::super::path_key(&a), super::super::path_key(&b)]);
        assert!(offer(&data).files.len() == 2);
        // The effect it should have, as Copy and Cut give it.
        set_dword(&data, PREFERRED_EFFECT, super::super::MOVE).unwrap();
        assert_eq!(get_dword(&data, PREFERRED_EFFECT), Some(super::super::MOVE));
        assert_eq!(payload(&data), Payload::Files(got));
        // A path that is not there.
        assert!(for_paths(&[dir.join("missing")]).is_err());
        drop(data);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
