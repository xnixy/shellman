//! Walk the shell's Apps folder.

use super::App;
use windows::core::PWSTR;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, SHGetKnownFolderItem,
    KF_FLAG_DEFAULT, SIGDN, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
};

/// Every app in the Apps folder, sorted by name, case-insensitively. Blocks for a moment (the
/// shell reads Start menu shortcuts and the Store registry), so call it from a worker thread.
/// Empty when the shell cannot be asked.
pub fn enumerate() -> Vec<App> {
    let mut apps = unsafe { walk() }.unwrap_or_default();
    apps.retain(|a| !is_noise(&a.name));
    apps.sort_by_cached_key(|a| (a.name.to_lowercase(), a.id.clone()));
    apps.dedup_by(|a, b| a.id == b.id);
    apps
}

/// Uninstallers are in the folder like any shortcut, but nobody searches for them to launch.
fn is_noise(name: &str) -> bool {
    name.trim().is_empty() || name.to_lowercase().starts_with("uninstall ")
}

unsafe fn walk() -> Option<Vec<App>> {
    // Shell folders want a single-threaded apartment; already initialised is fine.
    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    let folder: IShellItem =
        SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None).ok()?;
    let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems).ok()?;
    let mut apps = Vec::new();
    loop {
        let mut slot = [None];
        let mut fetched = 0u32;
        // S_FALSE at the end of the list comes back as success with nothing fetched.
        if items.Next(&mut slot, Some(&mut fetched)).is_err() || fetched == 0 {
            break;
        }
        let Some(item) = slot[0].take() else { break };
        let (Some(name), Some(id)) = (
            display_name(&item, SIGDN_NORMALDISPLAY),
            display_name(&item, SIGDN_PARENTRELATIVEPARSING),
        ) else {
            continue;
        };
        apps.push(App { name, id });
    }
    Some(apps)
}

unsafe fn display_name(item: &IShellItem, kind: SIGDN) -> Option<String> {
    let raw: PWSTR = item.GetDisplayName(kind).ok()?;
    let text = raw.to_string().ok();
    CoTaskMemFree(Some(raw.0.cast()));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_apps_folder_lists_apps_with_names_and_ids() {
        let apps = enumerate();
        assert!(!apps.is_empty(), "no apps from shell:AppsFolder");
        assert!(apps.iter().all(|a| !a.name.is_empty() && !a.id.is_empty()));
        assert!(apps
            .windows(2)
            .all(|w| w[0].name.to_lowercase() <= w[1].name.to_lowercase()));
    }

    #[test]
    fn uninstallers_are_noise() {
        assert!(is_noise("Uninstall Foo"));
        assert!(!is_noise("Foo"));
    }
}
