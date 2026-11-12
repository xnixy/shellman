//! What a menu is for, and the shell's `IContextMenu` for it: some items in one folder, or a
//! folder's empty background.

use std::path::{Path, PathBuf};
use windows::core::HSTRING;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    IContextMenu, ILFree, IShellFolder, SHBindToParent, SHGetDesktopFolder, SHParseDisplayName,
};

/// The things a menu acts on.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Target {
    /// The folder shown. With no items, the menu is this folder's background menu.
    pub folder: PathBuf,
    pub items: Vec<PathBuf>,
    /// Shift was held: the extra entries Explorer shows then are wanted too.
    pub extended: bool,
}

impl Target {
    /// Whether every item is a folder (the folder itself, for a background menu): then picks
    /// that would open them in Explorer open them in Files instead.
    pub fn folders_only(&self) -> bool {
        if self.items.is_empty() {
            self.folder.is_dir()
        } else {
            self.items.iter().all(|p| p.is_dir())
        }
    }
}

/// The shell menu object for `target`, owned by `hwnd` for any window its commands open.
pub(super) fn context_menu(hwnd: HWND, target: &Target) -> Option<IContextMenu> {
    if target.items.is_empty() {
        background_menu(hwnd, &target.folder)
    } else {
        items_menu(hwnd, &target.items)
    }
}

/// The menu for some items in one folder.
pub(super) fn items_menu(hwnd: HWND, items: &[PathBuf]) -> Option<IContextMenu> {
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

/// The menu for a folder's empty space: New, Paste shortcut, Open in Terminal, and so on.
pub(super) fn background_menu(hwnd: HWND, folder: &Path) -> Option<IContextMenu> {
    let pidl = Pidl::parse(folder)?;
    unsafe {
        let desktop = SHGetDesktopFolder().ok()?;
        let shell_folder: IShellFolder = desktop.BindToObject(pidl.0, None).ok()?;
        shell_folder.CreateViewObject(hwnd).ok()
    }
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
