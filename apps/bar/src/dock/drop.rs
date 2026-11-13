//! What dragging files over the dock does, as pure rules tested without windows (the OLE side
//! is `drop_target`).
//!
//! - Over an app icon: resting there `SPRING` brings the app's window forward, so the drop can be
//!   finished inside it (spring-loading, as taskbars do); dropping opens the files with the app
//!   where that is well defined (`shellman_core::open_with`), else nothing happens.
//! - Over Files: a dropped folder opens in Files; a file opens its folder.
//! - Over the Recycle Bin: the files go to the bin (undoable). Never deleted for good.
//! - Near the edge between two pins (or after Files, or after the last pin), when everything
//!   dragged is a folder, a program or a shortcut: it is pinned there.

use shellman_core::open_with::{self, With};
use shellman_core::settings::dock::Pin;
use shellman_core::transfer::{self, COPY, LINK, NONE};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long the pointer rests on an app's icon before its window comes forward.
pub const SPRING: Duration = Duration::from_millis(700);

/// What an icon is, for a drop.
#[derive(Clone, Debug, PartialEq)]
pub enum Icon {
    Launcher,
    Files,
    Bin,
    App { app_id: String, exe: Option<PathBuf>, front: Option<isize> },
}

/// An icon's drop area: physical screen pixels (left, top, right, bottom), and its item's id.
#[derive(Clone, Debug, PartialEq)]
pub struct Spot {
    pub rect: [i32; 4],
    pub key: String,
    pub icon: Icon,
}

/// One dock window's drop areas, in physical screen pixels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Map {
    pub icons: Vec<Spot>,
    /// Where something pinned would go: the boundary's x and the pinned slot it is.
    pub gaps: Vec<(i32, usize)>,
    /// How far from a boundary still counts as aiming at it.
    pub zone: i32,
}

/// What the pointer is over.
#[derive(Clone, Debug, PartialEq)]
pub enum Aim {
    Nothing,
    Icon(usize),
    Gap(usize),
}

fn inside(r: &[i32; 4], (x, y): (i32, i32)) -> bool {
    x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}

/// What a point aims at. A boundary between pins wins near its edge, but only when what is
/// dragged can be pinned; otherwise icons take their whole cell.
pub fn aim(map: &Map, pt: (i32, i32), pinnable: bool) -> Aim {
    let on_row = map.icons.iter().any(|s| pt.1 >= s.rect[1] && pt.1 < s.rect[3]);
    if pinnable && on_row {
        if let Some(&(_, slot)) = map.gaps.iter().filter(|(x, _)| (pt.0 - x).abs() <= map.zone).min_by_key(|(x, _)| (pt.0 - x).abs()) {
            return Aim::Gap(slot);
        }
    }
    map.icons.iter().position(|s| inside(&s.rect, pt)).map_or(Aim::Nothing, Aim::Icon)
}

/// What a drop does.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    Nothing,
    /// Into the Recycle Bin, as Files' "Move to Recycle Bin" does.
    Recycle,
    /// Open this folder in Files.
    Reveal(PathBuf),
    OpenWith(With),
    /// Pin these at this slot.
    Pin(Vec<Pin>, usize),
}

/// What dropping `files` on `icon` does.
pub fn plan_icon(icon: &Icon, files: &[PathBuf], is_dir: impl Fn(&Path) -> bool) -> Plan {
    let Some(first) = files.first() else { return Plan::Nothing };
    match icon {
        Icon::Launcher => Plan::Nothing,
        Icon::Bin => Plan::Recycle,
        Icon::Files => {
            let folder = if is_dir(first) { Some(first.clone()) } else { first.parent().map(Path::to_path_buf) };
            folder.map_or(Plan::Nothing, Plan::Reveal)
        }
        Icon::App { app_id, exe, .. } => {
            open_with::with_for(app_id, exe.as_deref()).map_or(Plan::Nothing, Plan::OpenWith)
        }
    }
}

/// The pin for something dropped between pins: a folder (opened in Files), a program, or a
/// shortcut (pinned as the program it starts, when it starts one, else as itself). `target` is
/// a shortcut's target, read by the caller.
pub fn pin_for(path: &Path, is_dir: bool, target: Option<PathBuf>) -> Option<Pin> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase());
    let pin = |p: &Path| Pin { app_id: p.to_string_lossy().to_lowercase(), exe: Some(p.to_path_buf()) };
    if is_dir {
        return Some(pin(path));
    }
    match ext.as_deref() {
        Some("exe") => Some(pin(path)),
        Some("lnk") => {
            let program = target.filter(|t| t.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")));
            Some(pin(program.as_deref().unwrap_or(path)))
        }
        _ => None,
    }
}

/// The drop effect to show for a plan, from what the source allows. Nothing here ever asks the
/// source to delete what it dragged: the bin's move is done here and reported as done (see
/// `transfer::target`), and opening or pinning is a copy or a link.
pub fn effect(plan: &Plan, files: &[PathBuf], allowed: u32) -> u32 {
    let prefer = |first: u32, second: u32| {
        if allowed & first != 0 {
            first
        } else if allowed & second != 0 {
            second
        } else {
            NONE
        }
    };
    match plan {
        Plan::Nothing => NONE,
        Plan::Recycle => transfer::choose_bin(files, allowed),
        Plan::Reveal(_) | Plan::OpenWith(_) => prefer(COPY, LINK),
        Plan::Pin(..) => prefer(LINK, COPY),
    }
}

/// Spring-loading: resting on one app's icon for `SPRING` brings its window forward, once.
#[derive(Default)]
pub struct Spring {
    on: Option<(String, Instant)>,
    fired: bool,
}

impl Spring {
    /// The pointer is over the app `key` (None: over no app). True once, when the window should
    /// come forward now.
    pub fn over(&mut self, now: Instant, key: Option<&str>) -> bool {
        match (key, &self.on) {
            (None, _) => {
                self.on = None;
                false
            }
            (Some(k), Some((on, since))) if on == k => {
                if !self.fired && now >= *since + SPRING {
                    self.fired = true;
                    return true;
                }
                false
            }
            (Some(k), _) => {
                self.on = Some((k.to_owned(), now));
                self.fired = false;
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, exe: &str) -> Icon {
        Icon::App { app_id: id.into(), exe: Some(PathBuf::from(exe)), front: Some(1) }
    }

    /// Launcher, Files, pins a and b, a running app r, 48 px icons 8 px apart, and the bin on
    /// its own island; the row from y 100 to 160.
    fn map() -> Map {
        let spot = |l: i32, key: &str, icon: Icon| Spot { rect: [l - 4, 100, l + 52, 160], key: key.into(), icon };
        Map {
            icons: vec![
                spot(0, "l", Icon::Launcher),
                spot(56, "f", Icon::Files),
                spot(112, "a", app(r"c:\a.exe", r"C:\a.exe")),
                spot(168, "b", app("Pkg_1!App", r"C:\b.exe")),
                spot(236, "r", app("Valve.Steam", r"C:\s.exe")),
                spot(400, "bin", Icon::Bin),
            ],
            // After Files, between a and b, after b.
            gaps: vec![(108, 0), (164, 1), (224, 2)],
            zone: 12,
        }
    }

    #[test]
    fn icons_take_their_cells_and_gaps_their_edges_only_for_pinnable_drags() {
        let m = map();
        assert_eq!(aim(&m, (140, 130), false), Aim::Icon(2));
        assert_eq!(aim(&m, (140, 130), true), Aim::Icon(2));
        // Near the edge between a and b.
        assert_eq!(aim(&m, (166, 130), false), Aim::Icon(3));
        assert_eq!(aim(&m, (166, 130), true), Aim::Gap(1));
        assert_eq!(aim(&m, (110, 130), true), Aim::Gap(0));
        assert_eq!(aim(&m, (230, 130), true), Aim::Gap(2));
        // Above the row, and between islands: nothing.
        assert_eq!(aim(&m, (166, 90), true), Aim::Nothing);
        assert_eq!(aim(&m, (330, 130), false), Aim::Nothing);
        assert_eq!(aim(&m, (420, 130), false), Aim::Icon(5));
    }

    #[test]
    fn what_a_drop_on_each_icon_does() {
        let files = vec![PathBuf::from(r"C:\t\one.txt"), PathBuf::from(r"C:\t\two.txt")];
        let no_dirs = |_: &Path| false;
        let m = map();
        assert_eq!(plan_icon(&m.icons[0].icon, &files, no_dirs), Plan::Nothing);
        assert_eq!(plan_icon(&m.icons[1].icon, &files, no_dirs), Plan::Reveal(PathBuf::from(r"C:\t")));
        assert_eq!(plan_icon(&m.icons[1].icon, &files, |_| true), Plan::Reveal(PathBuf::from(r"C:\t\one.txt")));
        assert_eq!(plan_icon(&m.icons[2].icon, &files, no_dirs), Plan::OpenWith(With::Exe(PathBuf::from(r"C:\a.exe"))));
        assert_eq!(plan_icon(&m.icons[3].icon, &files, no_dirs), Plan::OpenWith(With::Packaged("Pkg_1!App".into())));
        // Not well defined: only spring-loading.
        assert_eq!(plan_icon(&m.icons[4].icon, &files, no_dirs), Plan::Nothing);
        assert_eq!(plan_icon(&m.icons[5].icon, &files, no_dirs), Plan::Recycle);
        assert_eq!(plan_icon(&m.icons[5].icon, &[], no_dirs), Plan::Nothing);
    }

    #[test]
    fn effects_never_ask_the_source_to_delete() {
        use shellman_core::transfer::MOVE;
        let f = vec![PathBuf::from(r"C:\x")];
        let all = COPY | MOVE | LINK;
        assert_eq!(effect(&Plan::Recycle, &f, all), MOVE);
        assert_eq!(effect(&Plan::Recycle, &f, COPY), NONE);
        assert_eq!(effect(&Plan::OpenWith(With::Exe("a".into())), &f, all), COPY);
        assert_eq!(effect(&Plan::OpenWith(With::Exe("a".into())), &f, MOVE), NONE);
        assert_eq!(effect(&Plan::Pin(vec![], 0), &f, all), LINK);
        assert_eq!(effect(&Plan::Pin(vec![], 0), &f, COPY | MOVE), COPY);
        assert_eq!(effect(&Plan::Nothing, &f, all), NONE);
    }

    #[test]
    fn what_can_be_pinned() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(pin_for(&p(r"C:\Work"), true, None).unwrap().app_id, r"c:\work");
        let exe = pin_for(&p(r"C:\Apps\Tool.EXE"), false, None).unwrap();
        assert_eq!((exe.app_id.as_str(), exe.exe), (r"c:\apps\tool.exe", Some(p(r"C:\Apps\Tool.EXE"))));
        // A shortcut to a program is pinned as the program, so its windows land on the pin.
        let lnk = pin_for(&p(r"C:\S\Tool.lnk"), false, Some(p(r"C:\Apps\tool.exe"))).unwrap();
        assert_eq!(lnk.app_id, r"c:\apps\tool.exe");
        // One to a document stays a shortcut.
        assert_eq!(pin_for(&p(r"C:\S\Doc.lnk"), false, Some(p(r"C:\d.txt"))).unwrap().app_id, r"c:\s\doc.lnk");
        assert_eq!(pin_for(&p(r"C:\d.txt"), false, None), None);
    }

    #[test]
    fn spring_fires_once_after_resting() {
        let t = Instant::now();
        let mut s = Spring::default();
        assert!(!s.over(t, Some("a")));
        assert!(!s.over(t + Duration::from_millis(699), Some("a")));
        assert!(s.over(t + SPRING, Some("a")));
        assert!(!s.over(t + Duration::from_secs(3), Some("a")));
        // Moving to another app starts again; leaving forgets.
        assert!(!s.over(t + Duration::from_secs(3), Some("b")));
        assert!(!s.over(t + Duration::from_millis(3500), None));
        assert!(!s.over(t + Duration::from_millis(3600), Some("b")));
        assert!(s.over(t + Duration::from_millis(4300), Some("b")));
    }
}
