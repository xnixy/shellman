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

/// Where in the pinned section an icon dragged to `center` (in the same space as `xs`, the icons'
/// left edges in the current layout, each `icon` wide) would land: an index among the pinned apps
/// other than itself, 0 being right after Files. None where it may not go: a running app that is
/// not pinned cannot be reordered among the running apps, only dropped into the pinned ones.
/// The launcher's and Files' places count as the start of the pinned section, never before it.
pub fn drop_slot(items: &[Item], xs: &[f32], icon: f32, dragged: &str, center: f32) -> Option<usize> {
    let from = items.iter().find(|i| i.app_id == dragged && i.pinnable())?;
    if from.section == Section::Running {
        let first = items.iter().position(|i| i.section == Section::Running)?;
        if center >= xs[first] {
            return None;
        }
    }
    let before = items
        .iter()
        .zip(xs)
        .filter(|(i, &x)| i.section == Section::Pinned && i.app_id != dragged && x + icon / 2.0 < center)
        .count();
    Some(before)
}

/// The icons as they will be with `dragged` dropped at pinned slot `slot` (see `drop_slot`): the
/// same list with that icon moved into the pinned section.
pub fn preview(items: &[Item], dragged: &str, slot: usize) -> Vec<Item> {
    let Some(at) = items.iter().position(|i| i.app_id == dragged) else { return items.to_vec() };
    let mut out = items.to_vec();
    let mut item = out.remove(at);
    item.section = Section::Pinned;
    // The pinned section starts after the Launcher and Files.
    let start = out.iter().position(|i| !matches!(i.section, Section::Launcher | Section::Files)).unwrap_or(out.len());
    let end = out.iter().position(|i| matches!(i.section, Section::Running | Section::Bin)).unwrap_or(out.len());
    out.insert((start + slot).min(end), item);
    out
}

/// Put `pin` at pinned slot `slot` (an index among the pins that are shown, which leave out one
/// that duplicates Files), adding it if it is not pinned yet. Returns whether the list changed.
pub fn move_pin(pins: &mut Vec<Pin>, files: &Pin, pin: Pin, slot: usize) -> bool {
    let before = pins.clone();
    let existing = pins.iter().position(|p| p.matches(&pin.app_id)).map(|at| pins.remove(at)).unwrap_or(pin);
    let shown: Vec<usize> = pins.iter().enumerate().filter(|(_, p)| !files.matches(&p.app_id)).map(|(i, _)| i).collect();
    let at = match shown.get(slot) {
        Some(&at) => at,
        None => shown.last().map_or(pins.len(), |&last| last + 1),
    };
    pins.insert(at, existing);
    *pins != before
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

    fn item(section: Section, id: &str) -> Item {
        Item { section, app_id: id.into(), exe: None, windows: Vec::new() }
    }

    /// Launcher, Files, pinned a b c, running x y, bin; 48 pt icons 8 pt apart.
    fn row() -> (Vec<Item>, Vec<f32>) {
        let items = vec![
            item(Section::Launcher, "l"),
            item(Section::Files, "f"),
            item(Section::Pinned, "a"),
            item(Section::Pinned, "b"),
            item(Section::Pinned, "c"),
            item(Section::Running, "x"),
            item(Section::Running, "y"),
            item(Section::Bin, "z"),
        ];
        let xs = vec![0.0, 56.0, 112.0, 168.0, 224.0, 292.0, 348.0, 0.0];
        (items, xs)
    }

    fn names(items: &[Item]) -> String {
        items.iter().map(|i| i.app_id.as_str()).collect()
    }

    #[test]
    fn a_pinned_icon_lands_among_the_pinned() {
        let (items, xs) = row();
        // Dragging b: over a's left half, between a and c, past c, and way out to the right.
        assert_eq!(drop_slot(&items, &xs, 48.0, "b", 120.0), Some(0));
        assert_eq!(drop_slot(&items, &xs, 48.0, "b", 200.0), Some(1));
        assert_eq!(drop_slot(&items, &xs, 48.0, "b", 260.0), Some(2));
        assert_eq!(drop_slot(&items, &xs, 48.0, "b", 280.0), Some(2));
        // Over the launcher or Files: first place, never before them.
        assert_eq!(drop_slot(&items, &xs, 48.0, "c", 10.0), Some(0));
        assert_eq!(names(&preview(&items, "b", 0)), "lfbacxyz");
        assert_eq!(names(&preview(&items, "a", 2)), "lfbcaxyz");
    }

    #[test]
    fn a_running_icon_pins_only_into_the_pinned_section() {
        let (items, xs) = row();
        assert_eq!(drop_slot(&items, &xs, 48.0, "x", 190.0), Some(1));
        assert_eq!(drop_slot(&items, &xs, 48.0, "y", 100.0), Some(0));
        // At the end of the pinned apps, just before the running ones.
        assert_eq!(drop_slot(&items, &xs, 48.0, "y", 280.0), Some(3));
        // Among the running apps: not reorderable.
        assert_eq!(drop_slot(&items, &xs, 48.0, "x", 300.0), None);
        assert_eq!(drop_slot(&items, &xs, 48.0, "y", 330.0), None);
        let shown = preview(&items, "y", 3);
        assert_eq!(names(&shown), "lfabcyxz");
        assert_eq!(shown[5].section, Section::Pinned);
        assert_eq!(shown[6].section, Section::Running);
    }

    #[test]
    fn fixed_icons_do_not_move() {
        let (items, xs) = row();
        for id in ["l", "f", "z"] {
            assert_eq!(drop_slot(&items, &xs, 48.0, id, 190.0), None, "{id}");
        }
        assert_eq!(drop_slot(&items, &xs, 48.0, "gone", 190.0), None);
        // Whatever the slot, the launcher and Files stay first and the bin last.
        for slot in 0..6 {
            let shown = preview(&items, "x", slot);
            assert_eq!(shown[..2].iter().map(|i| i.section).collect::<Vec<_>>(), [Section::Launcher, Section::Files]);
            assert_eq!(shown.last().unwrap().section, Section::Bin);
        }
    }

    fn pin_names(p: &[Pin]) -> String {
        p.iter().map(|p| p.app_id.as_str()).collect::<Vec<_>>().join(",")
    }

    #[test]
    fn moving_a_pin_saves_the_new_order() {
        let files = pin("files");
        let mut p = vec![pin("a"), pin("b"), pin("c")];
        assert!(move_pin(&mut p, &files, pin("c"), 0));
        assert_eq!(pin_names(&p), "c,a,b");
        assert!(move_pin(&mut p, &files, pin("c"), 2));
        assert_eq!(pin_names(&p), "a,b,c");
        // Dropped where it already is: nothing to save.
        assert!(!move_pin(&mut p, &files, pin("B"), 1));
        // A running app is pinned at the place.
        assert!(move_pin(&mut p, &files, pin("x"), 1));
        assert_eq!(pin_names(&p), "a,x,b,c");
        assert!(move_pin(&mut p, &files, pin("y"), 9));
        assert_eq!(pin_names(&p), "a,x,b,c,y");
        let mut none = Vec::new();
        assert!(move_pin(&mut none, &files, pin("x"), 0));
        assert_eq!(pin_names(&none), "x");
    }

    #[test]
    fn a_hidden_pin_for_files_does_not_count_as_a_slot() {
        let files = pin("files");
        let mut p = vec![pin("a"), pin("FILES"), pin("b")];
        // Shown slots: a, b. Slot 1 is before b.
        assert!(move_pin(&mut p, &files, pin("c"), 1));
        assert_eq!(pin_names(&p), "a,FILES,c,b");
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
