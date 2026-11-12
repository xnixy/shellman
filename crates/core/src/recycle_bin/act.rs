//! Acting on items in the bin: put them back, or delete them for good.

use super::com::{bin_folder, Apartment};
use super::items::Handle;
use windows::core::PCSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    FileOperation, IContextMenu, IFileOperation, IShellItem, SHCreateItemWithParent,
    CMINVOKECOMMANDINFO, CMF_NORMAL, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
};
use windows::Win32::UI::WindowsAndMessaging::{CreatePopupMenu, DestroyMenu, SW_SHOWNORMAL};

const FIRST_ID: u32 = 1;
const LAST_ID: u32 = 0x7FFF;

/// Put items back where they were deleted from, creating folders that are gone. This invokes the
/// bin's own "undelete" command, so a name that is taken again is settled by Windows' usual
/// replace-or-skip question, shown over `hwnd`.
pub fn restore(hwnd: isize, items: &[Handle]) -> Result<(), String> {
    if items.is_empty() {
        return Ok(());
    }
    let _apartment = Apartment::enter();
    let folder = bin_folder().map_err(|e| e.message())?;
    let owner = HWND(hwnd as *mut _);
    let pidls: Vec<*const ITEMIDLIST> = items.iter().map(Handle::pidl).collect();
    let menu: IContextMenu = unsafe { folder.GetUIObjectOf(owner, &pidls, None) }.map_err(|e| e.message())?;
    unsafe {
        // The handlers fill their commands in only once asked for a menu, though none is shown.
        let hmenu = CreatePopupMenu().map_err(|e| e.message())?;
        let queried = menu.QueryContextMenu(hmenu, 0, FIRST_ID, LAST_ID, CMF_NORMAL);
        let info = CMINVOKECOMMANDINFO {
            cbSize: size_of::<CMINVOKECOMMANDINFO>() as u32,
            hwnd: owner,
            lpVerb: PCSTR(c"undelete".as_ptr().cast()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        let done = queried.ok().and_then(|()| menu.InvokeCommand(&info));
        let _ = DestroyMenu(hmenu);
        done.map_err(|e| e.message())
    }
}

/// Delete items for good. No shell question and no error window: the caller confirmed with the
/// user and reports the result.
pub fn delete(hwnd: isize, items: &[Handle]) -> Result<(), String> {
    if items.is_empty() {
        return Ok(());
    }
    let _apartment = Apartment::enter();
    let folder = bin_folder().map_err(|e| e.message())?;
    unsafe {
        let op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL).map_err(|e| e.message())?;
        op.SetOwnerWindow(HWND(hwnd as *mut _)).map_err(|e| e.message())?;
        op.SetOperationFlags(FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT).map_err(|e| e.message())?;
        for handle in items {
            // Deleting an item that is already in the bin removes it for good.
            let item: IShellItem = SHCreateItemWithParent(None, &folder, handle.pidl()).map_err(|e| e.message())?;
            op.DeleteItem(&item, None).map_err(|e| e.message())?;
        }
        op.PerformOperations().map_err(|e| e.message())?;
        if op.GetAnyOperationsAborted().is_ok_and(|b| b.as_bool()) {
            return Err("Windows could not delete everything".into());
        }
    }
    Ok(())
}
