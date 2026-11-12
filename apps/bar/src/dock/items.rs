//! What the dock shows, in order: the Launcher, Files, pinned apps, running apps that are not pinned, and the
//! Recycle Bin. Pure logic over the window list and the pins, so it can be tested without windows.

use shellman_core::settings::dock::{self as pins, Pin};
use shellman_core::winlist::AppWindow;
use std::path::PathBuf;

/// Where an item sits. Dividers go between the sections.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Launcher,
    Files,
    Pinned,
    Running,
    Bin,
}

/// One dock icon: an app and its windows (front-most first), which may be none for a pin.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub section: Section,
    pub app_id: String,
    pub exe: Option<PathBuf>,
    pub windows: Vec<isize>,
}

pub const BIN: &str = "shell:RecycleBinFolder";
/// The Launcher's stand-in id: it is not an app with an exe, so nothing else can match it.
const LAUNCHER: &str = "shellman:launcher";

impl Item {
    fn new(section: Section, app_id: &str, exe: Option<PathBuf>) -> Self {
        Self { section, app_id: app_id.to_owned(), exe, windows: Vec::new() }
    }

    /// None for the Launcher, which `draw` paints itself.
    pub fn icon_source(&self) -> Option<PathBuf> {
        match self.section {
            Section::Launcher => None,
            Section::Bin => Some(PathBuf::from(BIN)),
            _ => pins::icon_source(&self.app_id, self.exe.as_deref()),
        }
    }

    /// What a click opens when no window is there to show.
    pub fn launch_target(&self) -> Option<PathBuf> {
        match self.section {
            Section::Launcher => None,
            Section::Bin => Some(PathBuf::from(BIN)),
            _ => pins::launch_target(&self.app_id, self.exe.as_deref()),
        }
    }

    pub fn name(&self) -> String {
        match self.section {
            Section::Launcher => "Launcher".into(),
            Section::Files => "Files".into(),
            Section::Bin => "Recycle Bin".into(),
            _ => shellman_core::system::app_name::display_name(&self.app_id, self.exe.as_deref()),
        }
    }

    /// Whether the right-click menu may pin or unpin it. The Launcher, Files and the bin are always there.
    pub fn pinnable(&self) -> bool {
        matches!(self.section, Section::Pinned | Section::Running)
    }
}

/// shellman's own Files app: the sibling program of the running bar.
pub fn files_pin() -> Pin {
    let exe = std::env::current_exe().ok().map(|p| p.with_file_name("shellman-files.exe"));
    Pin {
        app_id: exe.as_ref().map(|p| p.to_string_lossy().to_lowercase()).unwrap_or_default(),
        exe,
    }
}

/// The icons for these windows. `prev_running` is the order of the unpinned running apps last
/// time, so icons keep their places and new apps go at the end.
pub fn build(pins: &[Pin], files: &Pin, windows: &[AppWindow], prev_running: &[String]) -> Vec<Item> {
    let mut files_item = Item::new(Section::Files, &files.app_id, files.exe.clone());
    // A pin that duplicates Files would show it twice.
    let mut pinned: Vec<Item> = pins
        .iter()
        .filter(|p| !files.matches(&p.app_id))
        .map(|p| Item::new(Section::Pinned, &p.app_id, p.exe.clone()))
        .collect();
    let mut running: Vec<Item> = Vec::new();
    for w in windows {
        if files.matches(&w.app_id) {
            files_item.windows.push(w.hwnd);
        } else if let Some(item) = pinned.iter_mut().find(|i| i.app_id.eq_ignore_ascii_case(&w.app_id)) {
            item.windows.push(w.hwnd);
        } else if let Some(item) = running.iter_mut().find(|i| i.app_id.eq_ignore_ascii_case(&w.app_id)) {
            item.windows.push(w.hwnd);
        } else {
            let mut item = Item::new(Section::Running, &w.app_id, w.exe.clone());
            item.windows.push(w.hwnd);
            running.push(item);
        }
    }
    // Stable sort: known apps by their old place, new ones after them in window order.
    running.sort_by_key(|i| prev_running.iter().position(|p| p.eq_ignore_ascii_case(&i.app_id)).unwrap_or(usize::MAX));
    let mut out = vec![Item::new(Section::Launcher, LAUNCHER, None), files_item];
    out.extend(pinned);
    out.extend(running);
    out.push(Item::new(Section::Bin, BIN, None));
    out
}

/// The window to show when stepping through an app's windows. Steps go through them in a fixed
/// order (by handle), not by depth, so each step moves on instead of swapping the top two.
/// With none of them in front, the front-most one.
pub fn cycle_target(windows: &[isize], front: Option<isize>, step: i32) -> Option<isize> {
    let first = *windows.first()?;
    let mut fixed = windows.to_vec();
    fixed.sort_unstable();
    let Some(at) = front.and_then(|f| fixed.iter().position(|&w| w == f)) else {
        return Some(first);
    };
    let n = fixed.len() as i32;
    Some(fixed[(at as i32 + step).rem_euclid(n) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(hwnd: isize, app: &str) -> AppWindow {
        AppWindow {
            hwnd,
            title: "t".into(),
            app_id: app.into(),
            exe: Some(PathBuf::from(app)),
            minimized: false,
        }
    }

    fn pin(app: &str) -> Pin {
        Pin { app_id: app.into(), exe: Some(PathBuf::from(app)) }
    }

    fn ids(items: &[Item]) -> Vec<(Section, &str, usize)> {
        items.iter().map(|i| (i.section, i.app_id.as_str(), i.windows.len())).collect()
    }

    #[test]
    fn order_is_launcher_files_pinned_running_bin() {
        let files = pin("c:\\files.exe");
        let items = build(
            &[pin("c:\\a.exe"), pin("c:\\b.exe")],
            &files,
            &[win(1, "c:\\x.exe"), win(2, "c:\\b.exe"), win(3, "c:\\files.exe"), win(4, "c:\\b.exe")],
            &[],
        );
        assert_eq!(
            ids(&items),
            vec![
                (Section::Launcher, LAUNCHER, 0),
                (Section::Files, "c:\\files.exe", 1),
                (Section::Pinned, "c:\\a.exe", 0),
                (Section::Pinned, "c:\\b.exe", 2),
                (Section::Running, "c:\\x.exe", 1),
                (Section::Bin, BIN, 0),
            ]
        );
        assert_eq!(items[3].windows, vec![2, 4]);
    }

    #[test]
    fn launcher_is_always_first_and_not_pinnable() {
        for windows in [vec![], vec![win(1, "c:\\x.exe")]] {
            let items = build(&[], &pin("f"), &windows, &[]);
            assert_eq!(items[0].section, Section::Launcher);
            assert!(items[0].windows.is_empty() && !items[0].pinnable());
            assert_eq!(items[1].section, Section::Files);
        }
    }

    #[test]
    fn pins_match_ignoring_case_and_files_is_never_doubled() {
        let files = pin("c:\\files.exe");
        let items = build(&[pin("C:\\FILES.exe"), pin("C:\\A.exe")], &files, &[win(1, "c:\\a.exe")], &[]);
        assert_eq!(items.iter().filter(|i| i.section == Section::Files).count(), 1);
        assert_eq!(items.iter().filter(|i| i.section == Section::Pinned).count(), 1);
        assert_eq!(items[2].windows, vec![1]);
    }

    #[test]
    fn running_apps_keep_their_places() {
        let files = pin("f");
        let prev = vec!["c:\\b.exe".to_string(), "c:\\a.exe".to_string()];
        let items = build(
            &[],
            &files,
            &[win(1, "c:\\a.exe"), win(2, "c:\\new.exe"), win(3, "c:\\B.exe"), win(4, "C:\\NEW.exe")],
            &prev,
        );
        let order: Vec<&str> =
            items.iter().filter(|i| i.section == Section::Running).map(|i| i.app_id.as_str()).collect();
        // Differences in case never split an app into two icons.
        assert_eq!(order, vec!["c:\\B.exe", "c:\\a.exe", "c:\\new.exe"]);
    }

    #[test]
    fn cycling_steps_in_a_fixed_order() {
        assert_eq!(cycle_target(&[30, 10, 20], Some(30), 1), Some(10));
        assert_eq!(cycle_target(&[20, 30, 10], Some(20), 1), Some(30));
        assert_eq!(cycle_target(&[20, 30, 10], Some(10), -1), Some(30));
        assert_eq!(cycle_target(&[20, 30, 10], Some(30), 1), Some(10));
        // Not in front: the front-most window.
        assert_eq!(cycle_target(&[20, 30, 10], Some(99), 1), Some(20));
        assert_eq!(cycle_target(&[], None, 1), None);
    }
}
