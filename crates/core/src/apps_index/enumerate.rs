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
    let mut folder = unsafe { walk() }.unwrap_or_default();
    folder.retain(|a| !is_noise(&a.name));
    // The Apps folder is not refreshed without Explorer, so read the Start menu too.
    let shortcuts = super::startmenu::shortcuts();
    let own = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(super::merge::own_apps))
        .unwrap_or_default();
    super::merge::merge(folder, own, shortcuts)
}

/// Nameless entries are of no use anywhere. (Help links and uninstallers are kept: they only
/// stay out of the launcher grid, see `App::is_listed`.)
fn is_noise(name: &str) -> bool {
    name.trim().is_empty()
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
        apps.push(App::new(name, id));
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
    #[ignore = "reads this machine's real Apps folder"]
    fn the_apps_folder_lists_apps_with_names_and_ids() {
        let apps = enumerate();
        assert!(!apps.is_empty(), "no apps from shell:AppsFolder");
        assert!(apps.iter().all(|a| !a.name.is_empty() && !a.id.is_empty()));
        assert!(apps
            .windows(2)
            .all(|w| w[0].name.to_lowercase() <= w[1].name.to_lowercase()));
    }

    #[test]
    #[ignore = "reads this machine's Apps folder and Start menu; set SHELLMAN_OWN_DIR to the folder holding the shellman exes"]
    fn the_four_own_apps_are_listed() {
        let t = std::time::Instant::now();
        let folder = unsafe { walk() }.unwrap_or_default();
        eprintln!("apps folder alone: {} items in {:?}", folder.len(), t.elapsed());
        let mut before: Vec<&str> = folder.iter().filter(|a| !is_noise(&a.name)).map(|a| a.id.as_str()).collect();
        before.sort();
        before.dedup();
        eprintln!("before this change the index held {}", before.len());
        let t = std::time::Instant::now();
        let shortcuts = super::super::startmenu::shortcuts();
        eprintln!("start menu scan: {} shortcuts in {:?}", shortcuts.len(), t.elapsed());
        let t = std::time::Instant::now();
        let mut apps = enumerate();
        eprintln!("{} apps in {:?}", apps.len(), t.elapsed());
        if let Ok(dir) = std::env::var("SHELLMAN_OWN_DIR") {
            let own = super::super::merge::own_apps(std::path::Path::new(&dir));
            apps = super::super::merge::merge(apps, own, vec![]);
        }
        for a in apps.iter().filter(|a| a.direct) {
            eprintln!("direct: {} [{}] own={} twin={}", a.name, a.id, a.own, a.windows_twin);
        }
        for a in apps.iter().filter(|a| a.windows_twin) {
            eprintln!("twin: {}", a.name);
        }
        for name in ["Files", "Settings", "Task Manager", "Terminal"] {
            let a = apps.iter().find(|a| a.own && a.name == name);
            assert!(a.is_some_and(|a| a.is_listed()), "{name} missing");
        }
    }

    #[test]
    fn only_nameless_entries_are_noise() {
        assert!(is_noise("  "));
        assert!(!is_noise("Foo"));
    }
}
