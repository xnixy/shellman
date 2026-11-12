//! The entries Windows and installed apps add to a file's menu (Open with, Send to, 7-Zip, Git,
//! Open in Terminal, ...), read out of the shell so Files can show them in its own menu. The
//! Windows menu itself is never shown.
//!
//! - `target`: what the menu is for, and the shell's `IContextMenu` for it.
//! - `walk`: has the handlers fill a hidden menu and reads it into a tree of `Item`s, filling
//!   submenus that wait to be opened, timing each.
//! - `paint`: entry bitmaps as RGBA, and owner-drawn entries rendered off screen.
//! - `worker`: the apartment thread a walk and the command picked from it run on.
//! - `loader`: the window's side: what is wanted, what arrived, stand-ins, hung workers.
//! - `model`, `tidy`, `slow`: the plain data, what Files shows of it, and slow-handler memory.

#[cfg(test)]
mod fake;
mod loader;
mod model;
mod paint;
mod slow;
mod target;
mod tidy;
mod walk;
mod worker;

pub use loader::{Loader, View};
pub use model::{Cascade, Icon, Item};
pub use paint::Palette;
pub use slow::SlowBook;
pub use target::Target;
pub use tidy::Route;

use std::path::Path;
use std::time::{Duration, Instant};
use windows::core::HSTRING;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

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

/// How one walk went, from `probe`.
pub struct Probe {
    /// The shell's whole tree, before Files leaves anything out.
    pub raw: Vec<Item>,
    /// What Files shows of it.
    pub shown: Vec<Item>,
    /// Time to the top level (every handler's QueryContextMenu).
    pub top: Duration,
    /// Each submenu filled, and its time.
    pub fills: Vec<(String, Duration)>,
}

/// Walks the menu for `target` on the calling thread, which must be in a single-threaded
/// apartment. For tests and measuring; Files walks on its worker.
pub fn probe(target: &Target) -> Option<Probe> {
    let start = Instant::now();
    let cm = target::context_menu(HWND::default(), target)?;
    let palette = Palette { text: 0xF0F0F0, background: 0x202020 };
    let mut walk = walk::Walk::query(cm, target.extended, palette)?;
    let plan = SlowBook::default();
    let mut raw = walk.top(&plan);
    let top = start.elapsed();
    let mut fills = Vec::new();
    while let Some(f) = walk.fill_next(&mut raw, &plan) {
        fills.push(f);
    }
    let shown = tidy::tidy(raw.clone());
    Some(Probe { raw, shown, top, fills })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

    /// Walks the real menus of a file and of a folder's background, without showing anything.
    /// Catches broken COM plumbing without a person clicking.
    #[test]
    fn walks_menus_for_items_and_background() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let windows = Path::new(r"C:\Windows");
        let file = Target {
            folder: windows.to_path_buf(),
            items: vec![windows.join("notepad.exe")],
            extended: false,
        };
        let p = probe(&file).expect("item menu");
        assert!(!p.raw.is_empty(), "menu has entries");
        // Files has its own Copy and Properties, so the shell's are gone from what it shows.
        let verbs: Vec<_> = p.shown.iter().filter_map(|i| i.verb.clone()).collect();
        assert!(!verbs.iter().any(|v| v == "copy" || v == "properties"), "{verbs:?}");
        assert!(p.raw.iter().any(|i| i.verb.as_deref() == Some("properties")));
        let background = Target { folder: windows.to_path_buf(), items: vec![], extended: false };
        assert!(!probe(&background).expect("background menu").raw.is_empty());
    }
}
