//! Listing what is in the bin, through the shell namespace.
//!
//! The namespace gives the original location and the deletion date as properties and hands back
//! an item ID per entry, which is all that restore and delete need. Parsing `$Recycle.Bin`'s
//! `$I` files by hand would mean tracking their format (it changed between Windows 7 and 10),
//! reading across every drive's bin, and still going through the shell to act on the result.

use super::com::{bin_folder, Apartment};
use crate::fsops::time::{format_time_at, LocalClock};
use crate::fsops::Entry;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use windows::core::{GUID, PCWSTR};
use windows::Win32::Foundation::{FILETIME, HWND};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::UI::Shell::{
    IEnumIDList, ILFree, ILGetSize, IShellItem2, SHCreateItemWithParent, SHCONTF_FOLDERS,
    SHCONTF_INCLUDEHIDDEN, SHCONTF_NONFOLDERS, SIGDN_FILESYSPATH, SIGDN_PARENTRELATIVE,
};

/// System.Recycle.DeletedFrom: the folder the item was deleted from.
const DELETED_FROM: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9B174B33_40FF_11D2_A27E_00C04FC30871),
    pid: 2,
};
/// System.Recycle.DateDeleted.
const DATE_DELETED: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9B174B33_40FF_11D2_A27E_00C04FC30871),
    pid: 3,
};
/// System.Size.
const SIZE: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xB725F130_47EF_101A_A5F1_02608C9EEBAC),
    pid: 12,
};

/// What it takes to act on one item later: its item ID relative to the bin's folder. Stored as
/// 16-bit words so the pointer handed to the shell is aligned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handle(pub(super) Vec<u16>);

impl Handle {
    pub(super) fn pidl(&self) -> *const ITEMIDLIST {
        self.0.as_ptr().cast()
    }

    fn copy_of(pidl: *const ITEMIDLIST) -> Self {
        let bytes = unsafe { ILGetSize(Some(pidl)) } as usize;
        let words = unsafe { std::slice::from_raw_parts(pidl.cast::<u16>(), bytes.div_ceil(2)) };
        Self(words.to_vec())
    }
}

#[derive(Clone, Debug)]
pub struct BinItem {
    /// The name it had, with its extension whatever the "hide extensions" setting says.
    pub name: String,
    /// Where it was before it was deleted, name included.
    pub original: PathBuf,
    pub deleted: Option<SystemTime>,
    /// Formatted in local time, once, here.
    pub deleted_text: String,
    pub size: u64,
    pub is_dir: bool,
    /// Where the bin keeps it now (`X:\$Recycle.Bin\<SID>\$R...`): unique, so selections key on it.
    pub path: PathBuf,
    pub handle: Handle,
}

impl BinItem {
    /// The same item as a row of a folder listing, so the list, selection and thumbnails work on
    /// it unchanged. The original folder stands in for the type and the deletion date for the
    /// modified date; the bin view labels those columns itself.
    pub fn to_entry(&self) -> Entry {
        let dir = self.original.parent().map(|p| p.display().to_string()).unwrap_or_default();
        Entry {
            key: self.name.to_lowercase(),
            name: self.name.clone(),
            path: self.path.clone(),
            is_dir: self.is_dir,
            size: self.size,
            modified: self.deleted,
            modified_text: self.deleted_text.clone(),
            kind: dir,
            hidden: false,
        }
    }
}

/// Everything in the bin, on every drive. Blocks, so call it from a worker thread.
pub fn list() -> Result<Vec<BinItem>, String> {
    let _apartment = Apartment::enter();
    let folder = bin_folder().map_err(|e| e.message())?;
    let mut walk: Option<IEnumIDList> = None;
    let flags = (SHCONTF_FOLDERS.0 | SHCONTF_NONFOLDERS.0 | SHCONTF_INCLUDEHIDDEN.0) as u32;
    unsafe { folder.EnumObjects(HWND::default(), flags, &mut walk) }
        .ok()
        .map_err(|e| e.message())?;
    let Some(walk) = walk else { return Ok(Vec::new()) };
    let mut clock = LocalClock::default();
    let mut out = Vec::new();
    loop {
        let mut next = [std::ptr::null_mut::<ITEMIDLIST>()];
        let mut got = 0u32;
        // S_FALSE with nothing fetched ends the walk.
        if unsafe { walk.Next(&mut next, Some(&mut got)) }.is_err() || got == 0 {
            break;
        }
        let pidl = next[0];
        let handle = Handle::copy_of(pidl);
        let item: windows::core::Result<IShellItem2> = unsafe { SHCreateItemWithParent(None, &folder, pidl) };
        unsafe { ILFree(Some(pidl)) };
        if let Ok(item) = item {
            if let Some(one) = read(&item, handle, &mut clock) {
                out.push(one);
            }
        }
    }
    Ok(out)
}

fn read(item: &IShellItem2, handle: Handle, clock: &mut LocalClock) -> Option<BinItem> {
    let text = |sigdn| unsafe { item.GetDisplayName(sigdn) }.ok().map(take_string);
    let shown = text(SIGDN_PARENTRELATIVE)?;
    let path = PathBuf::from(text(SIGDN_FILESYSPATH)?);
    let is_dir = std::fs::metadata(&path).is_ok_and(|m| m.is_dir());
    let from = unsafe { item.GetString(&DELETED_FROM) }.ok().map(take_string).unwrap_or_default();
    let name = original_name(&shown, &path, is_dir);
    let deleted = unsafe { item.GetFileTime(&DATE_DELETED) }.ok().and_then(filetime_to_system);
    Some(BinItem {
        original: Path::new(&from).join(&name),
        deleted_text: deleted.map(|t| format_time_at(t, clock.offset_for(t))).unwrap_or_default(),
        deleted,
        size: unsafe { item.GetUInt64(&SIZE) }.unwrap_or(0),
        name,
        is_dir,
        path,
        handle,
    })
}

/// Read a shell-allocated string and free it.
fn take_string(s: windows::core::PWSTR) -> String {
    let out = unsafe { PCWSTR(s.0).to_string() }.unwrap_or_default();
    unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(s.0.cast())) };
    out
}

fn filetime_to_system(ft: FILETIME) -> Option<SystemTime> {
    let ticks = (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime);
    // 100 ns ticks since 1601; the Unix epoch is this many ticks in.
    const UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;
    let after = ticks.checked_sub(UNIX_EPOCH_TICKS)?;
    SystemTime::UNIX_EPOCH.checked_add(Duration::from_nanos(after.checked_mul(100)?))
}

/// The item's real file name. The shell's name for it leaves the extension off when
/// the user hides extensions, but the original path needs it, and the bin's own copy
/// (`$R1A2B3C.txt`) keeps it. Folders have no extension to restore.
fn original_name(shown: &str, stored: &Path, is_dir: bool) -> String {
    if is_dir {
        return shown.to_owned();
    }
    let Some(ext) = stored.extension().map(|e| e.to_string_lossy()) else {
        return shown.to_owned();
    };
    let dotted = format!(".{ext}");
    if shown.to_lowercase().ends_with(&dotted.to_lowercase()) {
        shown.to_owned()
    } else {
        format!("{shown}{dotted}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_comes_back_when_the_display_name_hides_it() {
        let stored = Path::new(r"C:\$Recycle.Bin\S-1-5-21\$R1A2B3C.txt");
        assert_eq!(original_name("notes", stored, false), "notes.txt");
        assert_eq!(original_name("notes.txt", stored, false), "notes.txt");
        assert_eq!(original_name("NOTES.TXT", stored, false), "NOTES.TXT");
        // A folder named like a file, or a file with no extension, is left alone.
        assert_eq!(original_name("v1.2", Path::new(r"C:\$R1A2B3C"), true), "v1.2");
        assert_eq!(original_name("Makefile", Path::new(r"C:\$R1A2B3C"), false), "Makefile");
    }

    #[test]
    fn filetime_conversion() {
        // 2023-11-14 22:13:20 UTC.
        let ticks = (1_700_000_000u64 * 10_000_000) + 116_444_736_000_000_000;
        let ft = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
        let t = filetime_to_system(ft).unwrap();
        assert_eq!(t.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(), 1_700_000_000);
        assert!(filetime_to_system(FILETIME::default()).is_none());
    }
}
