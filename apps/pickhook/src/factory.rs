//! The class factory COM asks for a dialog, and the DLL's two exports.

use crate::dialog::{OpenDialog, SaveDialog};
use crate::guard::guard;
use crate::real::{CLSID_OPEN, CLSID_SAVE};
use std::ffi::c_void;
use windows::core::{implement, IUnknown, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_POINTER, E_UNEXPECTED, S_FALSE,
};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows_core::{Ref, Result, BOOL};

/// Makes open dialogs, or save dialogs.
#[implement(IClassFactory, Agile = false)]
pub struct Factory {
    pub save: bool,
}

// COM hands these pointers over under its own contract; a trait method cannot be `unsafe`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, outer: Ref<'_, IUnknown>, iid: *const GUID, out: *mut *mut c_void) -> Result<()> {
        guard(|| unsafe {
            if out.is_null() || iid.is_null() {
                return Err(E_POINTER.into());
            }
            *out = std::ptr::null_mut();
            if outer.is_some() {
                return Err(CLASS_E_NOAGGREGATION.into());
            }
            let object: IUnknown = if self.save {
                SaveDialog::new()?.into()
            } else {
                OpenDialog::new()?.into()
            };
            object.query(iid, out).ok()
        })
    }

    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}

/// The factory for `clsid`, as `riid`.
///
/// # Safety
/// `out` must be valid for writing an interface pointer.
pub unsafe fn class_object(clsid: &GUID, riid: &GUID, out: *mut *mut c_void) -> HRESULT {
    let r = guard(|| unsafe {
        if out.is_null() {
            return Err(E_POINTER.into());
        }
        *out = std::ptr::null_mut();
        let save = match *clsid {
            CLSID_OPEN => false,
            CLSID_SAVE => true,
            _ => return Err(CLASS_E_CLASSNOTAVAILABLE.into()),
        };
        let factory: IClassFactory = Factory { save }.into();
        factory.query(riid, out).ok()
    });
    match r {
        Ok(()) => HRESULT(0),
        Err(e) if e.code().is_err() => e.code(),
        Err(_) => E_UNEXPECTED,
    }
}

/// COM's way in: the factory for one of the two dialogs.
///
/// # Safety
/// Called by COM with valid pointers, as for any in-process server.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    clsid: *const GUID,
    riid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if clsid.is_null() || riid.is_null() {
        return E_POINTER;
    }
    unsafe { class_object(&*clsid, &*riid, out) }
}

/// Never unloaded: dialogs may be alive in ways COM cannot count, and staying is harmless.
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}
