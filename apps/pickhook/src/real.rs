//! The real Windows dialogs, made straight from `comdlg32.dll` in System32 through its own
//! `DllGetClassObject`. This never goes through the registry, so it reaches the real dialog even
//! when this DLL is registered in its place.

use windows::core::{s, w, Interface, GUID, HRESULT};
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::System::Com::IClassFactory;
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
};
use windows::Win32::UI::Shell::IFileDialog;
use windows_core::{Error, Result};

/// `CLSID_FileOpenDialog`.
pub const CLSID_OPEN: GUID = GUID::from_u128(0xDC1C5A9C_E88A_4DDE_A5A1_60F82A20AEF7);
/// `CLSID_FileSaveDialog`.
pub const CLSID_SAVE: GUID = GUID::from_u128(0xC0B4E2F3_BA21_4773_8DBA_335EC946EB8B);

type GetClassObject =
    unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut core::ffi::c_void) -> HRESULT;

/// A new real open (or save) dialog.
pub fn create(save: bool) -> Result<IFileDialog> {
    let clsid = if save { CLSID_SAVE } else { CLSID_OPEN };
    unsafe {
        // Only System32's copy; the library stays loaded for the life of the process.
        let dll = LoadLibraryExW(w!("comdlg32.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)?;
        let get = GetProcAddress(dll, s!("DllGetClassObject")).ok_or_else(|| Error::from(E_FAIL))?;
        let get: GetClassObject = std::mem::transmute(get);
        let mut raw = std::ptr::null_mut();
        get(&clsid, &IClassFactory::IID, &mut raw).ok()?;
        let factory = IClassFactory::from_raw(raw);
        factory.CreateInstance(None)
    }
}
