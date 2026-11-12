//! The real Windows context menu, the one Explorer shows under "Show more options", with every
//! entry other apps add: 7-Zip, Open with, Send to, Git, and so on.
//!
//! `show` builds it for some items or a folder's background and runs it where the pointer is.
//! While it is open, submenus that fill themselves in (Open with, Send to) need the window's menu
//! messages; the window procedure passes them to `forward`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use windows::core::{Interface, HSTRING, PCSTR, PSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    IContextMenu, IContextMenu2, IContextMenu3, ILFree, IShellFolder, SHBindToParent,
    SHGetDesktopFolder, SHParseDisplayName, ShellExecuteExW, CMINVOKECOMMANDINFO,
    SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetCursorPos, TrackPopupMenuEx, SW_SHOWNORMAL, TPM_RETURNCMD,
    TPM_RIGHTBUTTON,
};

/// What the user picked, when shellman should handle it itself.
pub enum Picked {
    Nothing,
    /// Windows ran the command (or the menu was dismissed).
    Done,
    /// "Rename": shellman renames in place, since the shell's rename needs Explorer's own view.
    Rename,
    /// "Open" on folders: shellman opens them itself, since the shell would hand them to Explorer.
    OpenFolders,
    /// "Open in new window" (or tab) on folders: shellman opens them in new tabs.
    OpenFoldersInTabs,
}

/// The picks shellman handles itself: rename, and the folder open verbs.
fn own_verb(verb: Option<&str>, folders_only: bool) -> Option<Picked> {
    if verb == Some("rename") {
        Some(Picked::Rename)
    } else {
        intercept(verb, folders_only)
    }
}

/// What shellman takes over when `verb` is picked on `folders_only` targets. Verbs on files, and
/// on a mix, go to Windows unchanged.
fn intercept(verb: Option<&str>, folders_only: bool) -> Option<Picked> {
    if !folders_only {
        return None;
    }
    match verb? {
        "open" | "explore" => Some(Picked::OpenFolders),
        "opennewwindow" | "opennewtab" | "opennewprocess" => Some(Picked::OpenFoldersInTabs),
        _ => None,
    }
}

thread_local! {
    /// The open menu, for `forward`. Menus run modally on the UI thread, so one slot is enough.
    static OPEN: RefCell<Option<IContextMenu2>> = const { RefCell::new(None) };
}

// QueryContextMenu flags: CMF_NORMAL 0, CMF_EXPLORE 0x4, CMF_CANRENAME 0x10,
// CMF_EXTENDEDVERBS 0x100 (the extra entries Explorer shows with Shift held).
const CMF_EXPLORE: u32 = 0x4;
const CMF_CANRENAME: u32 = 0x10;
const CMF_EXTENDEDVERBS: u32 = 0x100;
const FIRST_ID: u32 = 1;
const LAST_ID: u32 = 0x7FFF;
const GCS_VERBW: u32 = 4;

/// Show the shell menu for `items` (all in one folder), or for `folder`'s background when `items`
/// is empty. Blocks until the menu closes, like any Windows menu.
pub fn show(hwnd: isize, folder: &Path, items: &[PathBuf], extended: bool) -> Picked {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let hwnd = HWND(hwnd as *mut _);
    // The folder itself stands in for the items when the background menu is up.
    let folders_only = if items.is_empty() {
        folder.is_dir()
    } else {
        items.iter().all(|p| p.is_dir())
    };
    let menu = if items.is_empty() {
        background_menu(hwnd, folder)
    } else {
        items_menu(hwnd, items)
    };
    match menu {
        Some(cm) => run(hwnd, &cm, extended, folders_only),
        None => Picked::Nothing,
    }
}

/// Explorer's Properties sheet for one item.
pub fn properties(hwnd: isize, path: &Path) {
    let file = HSTRING::from(path.as_os_str());
    let verb = HSTRING::from("properties");
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        // SEE_MASK_INVOKEIDLIST: go through the item's context menu, where "properties" lives.
        fMask: 0x0000_000C,
        hwnd: HWND(hwnd as *mut _),
        lpVerb: windows::core::PCWSTR(verb.as_ptr()),
        lpFile: windows::core::PCWSTR(file.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe {
        let _ = ShellExecuteExW(&mut info);
    }
}

/// Pass a menu message to the open menu's handlers. Some(result) when it was handled.
pub fn forward(msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    // WM_INITMENUPOPUP, WM_DRAWITEM, WM_MEASUREITEM, WM_MENUCHAR.
    if !matches!(msg, 0x0117 | 0x002B | 0x002C | 0x0120) {
        return None;
    }
    OPEN.with(|open| {
        let open = open.borrow();
        let cm2 = open.as_ref()?;
        let mut result = LRESULT(0);
        if let Ok(cm3) = cm2.cast::<IContextMenu3>() {
            unsafe { cm3.HandleMenuMsg2(msg, wparam, lparam, Some(&mut result)) }.ok()?;
        } else {
            unsafe { cm2.HandleMenuMsg(msg, wparam, lparam) }.ok()?;
        }
        Some(result)
    })
}

/// The menu for some items in one folder.
fn items_menu(hwnd: HWND, items: &[PathBuf]) -> Option<IContextMenu> {
    let pidls: Vec<Pidl> = items.iter().filter_map(|p| Pidl::parse(p)).collect();
    if pidls.is_empty() {
        return None;
    }
    let mut parent: Option<IShellFolder> = None;
    let mut children: Vec<*const ITEMIDLIST> = Vec::with_capacity(pidls.len());
    for pidl in &pidls {
        let mut child: *mut ITEMIDLIST = std::ptr::null_mut();
        // The child pointer points into `pidl`, which outlives the menu's creation below.
        let folder: IShellFolder = unsafe { SHBindToParent(pidl.0, Some(&mut child)) }.ok()?;
        parent.get_or_insert(folder);
        children.push(child);
    }
    unsafe { parent?.GetUIObjectOf(hwnd, &children, None) }.ok()
}

/// The menu for a folder's empty space: View, New, Paste, Properties, and so on.
fn background_menu(hwnd: HWND, folder: &Path) -> Option<IContextMenu> {
    let pidl = Pidl::parse(folder)?;
    unsafe {
        let desktop = SHGetDesktopFolder().ok()?;
        let shell_folder: IShellFolder = desktop.BindToObject(pidl.0, None).ok()?;
        shell_folder.CreateViewObject(hwnd).ok()
    }
}

fn run(hwnd: HWND, cm: &IContextMenu, extended: bool, folders_only: bool) -> Picked {
    unsafe {
        let Ok(hmenu) = CreatePopupMenu() else {
            return Picked::Nothing;
        };
        let mut flags = CMF_EXPLORE | CMF_CANRENAME;
        if extended {
            flags |= CMF_EXTENDEDVERBS;
        }
        if cm
            .QueryContextMenu(hmenu, 0, FIRST_ID, LAST_ID, flags)
            .is_err()
        {
            let _ = DestroyMenu(hmenu);
            return Picked::Nothing;
        }
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        OPEN.with(|open| *open.borrow_mut() = cm.cast::<IContextMenu2>().ok());
        let cmd = TrackPopupMenuEx(
            hmenu,
            (TPM_RETURNCMD | TPM_RIGHTBUTTON).0,
            pt.x,
            pt.y,
            hwnd,
            None,
        )
        .0 as u32;
        OPEN.with(|open| *open.borrow_mut() = None);
        let picked = if cmd < FIRST_ID {
            Picked::Nothing
        } else if let Some(own) = own_verb(verb(cm, cmd - FIRST_ID).as_deref(), folders_only) {
            own
        } else {
            let info = CMINVOKECOMMANDINFO {
                cbSize: size_of::<CMINVOKECOMMANDINFO>() as u32,
                hwnd,
                // MAKEINTRESOURCE: the command's offset, passed in place of a verb string.
                lpVerb: PCSTR((cmd - FIRST_ID) as usize as *const u8),
                nShow: SW_SHOWNORMAL.0,
                ..Default::default()
            };
            let _ = cm.InvokeCommand(&info);
            Picked::Done
        };
        let _ = DestroyMenu(hmenu);
        picked
    }
}

/// The language-independent name of a menu command, like "rename" or "delete", if it has one.
fn verb(cm: &IContextMenu, offset: u32) -> Option<String> {
    let mut buf = [0u16; 64];
    unsafe {
        cm.GetCommandString(
            offset as usize,
            GCS_VERBW,
            None,
            PSTR(buf.as_mut_ptr().cast()),
            buf.len() as u32,
        )
        .ok()?;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..end]).to_lowercase())
}

/// An absolute item ID list, freed when dropped.
struct Pidl(*mut ITEMIDLIST);

impl Pidl {
    fn parse(path: &Path) -> Option<Self> {
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        unsafe { SHParseDisplayName(&HSTRING::from(path.as_os_str()), None, &mut pidl, 0, None) }
            .ok()?;
        (!pidl.is_null()).then_some(Pidl(pidl))
    }
}

impl Drop for Pidl {
    fn drop(&mut self) {
        unsafe { ILFree(Some(self.0)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds (without showing) the menu for a real file and for a folder's background, and
    /// checks it has entries. Catches broken COM plumbing without a person clicking.
    #[test]
    fn builds_menus_for_items_and_background() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let windows = Path::new(r"C:\Windows");
        let notepad = windows.join("notepad.exe");
        for cm in [
            items_menu(HWND::default(), &[notepad]).expect("item menu"),
            background_menu(HWND::default(), windows).expect("background menu"),
        ] {
            unsafe {
                let hmenu = CreatePopupMenu().unwrap();
                cm.QueryContextMenu(hmenu, 0, FIRST_ID, LAST_ID, CMF_EXPLORE)
                    .ok()
                    .unwrap();
                let count = windows::Win32::UI::WindowsAndMessaging::GetMenuItemCount(Some(hmenu));
                assert!(count > 0, "menu has entries");
                let _ = DestroyMenu(hmenu);
            }
        }
    }

    #[test]
    fn folder_open_verbs_are_taken_over() {
        assert!(matches!(own_verb(Some("open"), true), Some(Picked::OpenFolders)));
        assert!(matches!(own_verb(Some("explore"), true), Some(Picked::OpenFolders)));
        assert!(matches!(own_verb(Some("opennewwindow"), true), Some(Picked::OpenFoldersInTabs)));
        assert!(matches!(own_verb(Some("opennewtab"), true), Some(Picked::OpenFoldersInTabs)));
        assert!(matches!(own_verb(Some("rename"), false), Some(Picked::Rename)));
    }

    #[test]
    fn file_verbs_and_other_folder_verbs_are_left_alone() {
        assert!(own_verb(Some("open"), false).is_none());
        assert!(own_verb(Some("opennewwindow"), false).is_none());
        assert!(own_verb(Some("properties"), true).is_none());
        assert!(own_verb(Some("copy"), true).is_none());
        assert!(own_verb(None, true).is_none());
    }
}
