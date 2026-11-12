//! COM plumbing shared by the bin's calls: an apartment for the calling thread, and the bin's
//! shell folder.

use windows::core::Result;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    FOLDERID_RecycleBinFolder, ILFree, SHGetDesktopFolder, SHGetKnownFolderIDList, IShellFolder,
};

/// An apartment for the calling thread for as long as this lives. Every bin call runs on a worker
/// thread of its own, as thumbnails and the shell menu do, so the shell's apartment-bound
/// objects never cross threads and the window never waits on them.
pub(super) struct Apartment(bool);

impl Apartment {
    pub(super) fn enter() -> Self {
        // S_OK and S_FALSE both count a reference that must be released; a mode clash (an
        // existing multithreaded apartment) does not.
        Self(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

/// The Recycle Bin's folder in the shell namespace. It needs no Explorer: the namespace is
/// served from shell32 in the calling process.
pub(super) fn bin_folder() -> Result<IShellFolder> {
    unsafe {
        let pidl: *mut ITEMIDLIST = SHGetKnownFolderIDList(&FOLDERID_RecycleBinFolder, Default::default(), None)?;
        let folder = SHGetDesktopFolder().and_then(|desktop| desktop.BindToObject::<_, IShellFolder>(pidl, None::<&windows::Win32::System::Com::IBindCtx>));
        ILFree(Some(pidl));
        folder
    }
}
