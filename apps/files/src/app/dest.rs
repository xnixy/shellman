//! "Move to…" and "Copy to…": a small menu over the bottom bar listing where the selection
//! can go (the places it went last, newest first, then the rail's places and pins), and "Choose
//! folder…", which opens Files' own picker in folder mode as a separate window and waits for
//! its answer on a worker. The move or copy itself is the shell's, as for paste, with its
//! progress and undo.

use super::actions::Action;
use super::menu::item;
use super::Msg;
use egui::Rect;
use shellman_core::fsops::Op;
use shellman_ui::menu::popup::{Anchor, Entry};
use shellman_ui::theme::glyph;
use std::path::{Path, PathBuf};

/// How many recent places are remembered.
pub(super) const RECENT_MAX: usize = 8;

/// Put `p` first in the recent places, without repeats (Windows paths ignore case), keeping at
/// most `max`.
pub(super) fn remember(list: &mut Vec<PathBuf>, p: &Path, max: usize) {
    let key = crate::tags::key(p);
    list.retain(|q| crate::tags::key(q) != key);
    list.insert(0, p.to_path_buf());
    list.truncate(max);
}

/// The places the list offers: the recent ones that are still folders, then the rail's places
/// and pins not already listed. Each with whether it is recent.
pub(super) fn offered(recent: &[PathBuf], places: &[PathBuf], is_dir: impl Fn(&Path) -> bool) -> Vec<(PathBuf, bool)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (list, is_recent) in [(recent, true), (places, false)] {
        for p in list {
            if seen.insert(crate::tags::key(p)) && is_dir(p) {
                out.push((p.clone(), is_recent));
            }
        }
    }
    out
}

/// What moving or copying `items` into `dest` would do: None when it would do nothing (a move
/// into the folder they are already in) or cannot be done (a folder into itself).
pub(super) fn sensible(op: Op, items: &[PathBuf], dest: &Path) -> Result<(), &'static str> {
    if items.is_empty() {
        return Err("Nothing is selected");
    }
    let d = crate::tags::key(dest);
    for p in items {
        let k = crate::tags::key(p);
        if d == k || d.starts_with(&format!("{k}\\")) {
            return Err("A folder can't go inside itself");
        }
    }
    let all_here = items.iter().all(|p| p.parent().map(crate::tags::key) == Some(d.clone()));
    if op == Op::Move && all_here {
        return Err("They're already there");
    }
    Ok(())
}

/// A folder's name for the list, and the folder it is in, short.
fn names(p: &Path) -> (String, String) {
    let name = p
        .file_name()
        .map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned());
    let parent = p.parent().map(|q| q.display().to_string()).unwrap_or_default();
    (name, parent)
}

/// The Move to / Copy to menu: a heading saying what it does, the places (recent ones first,
/// with their folder as a quiet hint), and "Choose folder…".
pub(super) fn dest_entries(op: Op, count: usize, list: &[(PathBuf, bool)]) -> Vec<Entry<Action>> {
    let verb = if op == Op::Move { "Move" } else { "Copy" };
    let mut e = vec![Entry::heading(format!("{verb} {} to", super::text::count(count, "item")))];
    let mut recent_done = false;
    for (path, recent) in list {
        if !recent && !recent_done {
            recent_done = true;
            if list.first().is_some_and(|(_, r)| *r) {
                e.push(Entry::separator());
            }
        }
        let (name, parent) = names(path);
        let g = if *recent { glyph::UNDO } else { glyph::FOLDER };
        e.push(item(Action::SendTo(op, path.clone()), g, name).hint(parent));
    }
    e.push(Entry::separator());
    e.push(item(Action::ChooseDest(op), glyph::SEARCH, "Choose folder…"));
    e
}

impl super::FileMan {
    /// Open the list for `op` over the button at `anchor` (clicked again, it closes).
    pub(super) fn open_dest(&mut self, op: Op, anchor: Rect) {
        let count = self.tabs[self.active].selected.len();
        if count == 0 {
            return;
        }
        let mut places: Vec<PathBuf> = super::rail::quick_places().into_iter().map(|(_, _, p)| p).collect();
        places.extend(self.settings.pins.iter().cloned());
        let list = offered(&self.settings.recent_dests, &places, Path::is_dir);
        let key = if op == Op::Move { "move-to" } else { "copy-to" };
        super::menu::request(Anchor::over(&self.ctx, anchor), key, dest_entries(op, count, &list));
    }
    pub(super) fn selected_paths(&self) -> Vec<PathBuf> {
        self.tabs[self.active].selection().into_iter().map(|e| e.path).collect()
    }

    /// Move or copy `items` into `dest` with the shell, and remember the place.
    pub(super) fn send_to(&mut self, op: Op, items: Vec<PathBuf>, dest: PathBuf) {
        if let Err(why) = sensible(op, &items, &dest) {
            self.toast(why);
            return;
        }
        // A copy into the folder the items are in makes copies beside them.
        let here = items.iter().all(|p| p.parent() == Some(dest.as_path()));
        let op = if op == Op::Copy && here { Op::Duplicate } else { op };
        remember(&mut self.settings.recent_dests, &dest, RECENT_MAX);
        self.save_pending = true;
        let what = if op == Op::Move { "Move" } else { "Copy" };
        self.run_op(op, what, items, Some(dest));
    }

    /// "Choose folder…": Files' own picker, in folder mode, as its own window over this one. A
    /// worker waits for it and sends the answer back.
    pub(super) fn choose_dest(&mut self, op: Op) {
        let items = self.selected_paths();
        let Ok(exe) = std::env::current_exe() else { return };
        let verb = if op == Op::Move { "Move" } else { "Copy" };
        let req = shellman_pick::Request {
            mode: shellman_pick::Mode::Folder,
            title: format!("{verb} {} to", super::text::count(items.len(), "item")),
            ok_label: format!("{verb} here"),
            folder: self.tabs[self.active].loc.dir().map(Path::to_path_buf),
            owner: self.hwnd as i64,
            client: "shellman-files.exe|send-to".into(),
            host_pid: std::process::id(),
            ..Default::default()
        };
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = shellman_pick::new_request_path(&std::env::temp_dir(), std::process::id(), n);
        if let Err(e) = shellman_pick::write_json(&path, &req) {
            self.toast(format!("Couldn't open the folder picker: {e}"));
            return;
        }
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let response = shellman_pick::response_path(&path);
            let _ = std::fs::remove_file(&response);
            let status = std::process::Command::new(exe).arg("--pick").arg(&path).status();
            let dest = match status {
                Ok(s) if s.code() == Some(shellman_pick::exit::OK) => shellman_pick::read_response(&response)
                    .filter(|r| r.ok)
                    .and_then(|r| r.paths.into_iter().next()),
                _ => None,
            };
            for p in [&path, &response] {
                let _ = std::fs::remove_file(p);
            }
            if let Some(dest) = dest {
                let _ = tx.send(Msg::Chosen { op, items, dest });
                ctx.request_repaint();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn the_move_to_menu() {
        let list = [(p(r"C:\Work"), true), (p(r"C:\Users\me\Documents"), false)];
        let e = dest_entries(Op::Move, 3, &list);
        let rows: Vec<String> = e
            .iter()
            .map(|e| match e {
                Entry::Heading(t) => format!("({t})"),
                Entry::Item(i) => format!("{} | {}", i.label, i.hint),
                _ => "-".into(),
            })
            .collect();
        assert_eq!(rows, ["(Move 3 items to)", r"Work | C:\", "-", r"Documents | C:\Users\me", "-", "Choose folder… | "]);
        let Entry::Item(first) = &e[1] else { panic!() };
        assert!(matches!(&first.id, Action::SendTo(Op::Move, d) if *d == p(r"C:\Work")));
        // Nothing recent: no line above the places.
        let e = dest_entries(Op::Copy, 1, &list[1..]);
        assert!(matches!(&e[0], Entry::Heading(t) if t == "Copy 1 item to"));
        assert!(matches!(e[1], Entry::Item(_)));
    }
    #[test]
    fn recent_places_are_newest_first_without_repeats_and_few() {
        let mut list = Vec::new();
        for s in [r"C:\a", r"C:\b", r"c:\A\"] {
            remember(&mut list, &p(s), RECENT_MAX);
        }
        assert_eq!(list, [p(r"c:\A\"), p(r"C:\b")]);
        for i in 0..20 {
            remember(&mut list, &p(&format!(r"D:\{i}")), RECENT_MAX);
        }
        assert_eq!(list.len(), RECENT_MAX);
        assert_eq!(list[0], p(r"D:\19"));
    }

    #[test]
    fn the_list_puts_recent_first_and_skips_repeats_and_gone_folders() {
        let recent = [p(r"C:\Work"), p(r"C:\Gone")];
        let places = [p(r"C:\Users\me"), p(r"c:\work")];
        let out = offered(&recent, &places, |q| q != Path::new(r"C:\Gone"));
        assert_eq!(out, [(p(r"C:\Work"), true), (p(r"C:\Users\me"), false)]);
    }

    #[test]
    fn nonsense_moves_are_refused() {
        let items = [p(r"C:\a\x.txt"), p(r"C:\a\sub")];
        assert!(sensible(Op::Move, &items, Path::new(r"C:\a")).is_err());
        assert!(sensible(Op::Copy, &items, Path::new(r"C:\a")).is_ok(), "a copy here makes copies");
        assert!(sensible(Op::Move, &items, Path::new(r"C:\a\sub\deeper")).is_err());
        assert!(sensible(Op::Move, &items, Path::new(r"C:\a\subway")).is_ok());
        assert!(sensible(Op::Move, &items, Path::new(r"D:\")).is_ok());
        assert!(sensible(Op::Move, &[], Path::new(r"D:\")).is_err());
    }
}
