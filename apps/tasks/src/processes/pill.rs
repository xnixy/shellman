//! The command pill under the list: the selection's commands, and the inline questions ending
//! asks ("End <name>?"), instead of dialogs.

use crate::actions::{self, Act, Ask};
use crate::app::State;
use egui::{pos2, vec2, Align2, Color32, FontId, Rect};
use shellman_core::procs::policy::Verdict;
use shellman_ui::chips::{self, Chip};
use shellman_ui::theme::{colors, glyph};

const INFO: char = '\u{E946}';
const TREE: char = '\u{E9F5}';

/// Lays chips and text out from the left of the pill.
struct Row<'a> {
    ui: &'a mut egui::Ui,
    x: f32,
    y: f32,
    right: f32,
}

impl Row<'_> {
    /// A chip; true when clicked. A disabled one says why when hovered.
    fn chip(&mut self, label: &str, icon: Option<char>, kind: Chip, enabled: bool, tip: Option<&str>) -> bool {
        let w = chips::chip_width(self.ui, label, icon.is_some(), kind);
        if self.x + w > self.right {
            return false;
        }
        let rect = Rect::from_min_size(pos2(self.x, self.y - chips::CHIP_H / 2.0), vec2(w, chips::CHIP_H));
        self.x += w + 8.0;
        self.ui
            .scope(|ui| {
                if !enabled {
                    ui.disable();
                }
                let resp = chips::chip(ui, rect, egui::Id::new(("pill", label)), label, icon, kind);
                let resp = match tip {
                    Some(t) if enabled => resp.on_hover_text(t),
                    Some(t) => resp.on_disabled_hover_text(t),
                    None => resp,
                };
                enabled && resp.clicked()
            })
            .inner
    }

    fn text(&mut self, s: &str, color: Color32) {
        let room = (self.right - self.x).max(0.0);
        let painter = self.ui.painter().with_clip_rect(Rect::from_min_max(pos2(self.x, self.y - 14.0), pos2(self.x + room, self.y + 14.0)));
        let r = painter.text(pos2(self.x, self.y), Align2::LEFT_CENTER, s, FontId::proportional(13.0), color);
        self.x = r.right() + 12.0;
    }
}

pub fn show(ui: &mut egui::Ui, rect: Rect, st: &mut State) {
    let c = colors();
    let ask = st.actions.ask.clone();
    let mut row = Row { ui, x: rect.left() + 12.0, y: rect.center().y, right: rect.right() - 14.0 };
    match ask {
        Ask::Confirm { act, keys, name, warning } => {
            let q = match act {
                Act::End if keys.len() > 1 => format!("End {name}, all {} of its processes?", keys.len()),
                Act::End => format!("End {name}?"),
                Act::Tree if keys.len() > 1 => format!("End {name} and the {} processes it started?", keys.len() - 1),
                Act::Tree => format!("End {name}? It started no other processes."),
            };
            row.x += 6.0;
            row.text(&q, c.text);
            let yes = if warning.is_some() { "End anyway" } else { "End" };
            if row.chip(yes, None, Chip::Danger, true, None) {
                st.actions.confirm(&st.list, &st.control);
            }
            if row.chip("Cancel", None, Chip::Soft, true, None) {
                st.actions.cancel();
            }
            if let Some(w) = warning {
                row.text(w, c.warn);
            }
        }
        Ask::Closing { name, .. } => {
            row.x += 6.0;
            row.text(&format!("Asked {name} to close."), c.text);
            if row.chip("End now", None, Chip::Danger, true, Some("End it at once, without waiting; unsaved work is lost")) {
                st.actions.end_now(&st.control);
            }
            if row.chip("Done", None, Chip::Plain, true, None) {
                st.actions.cancel();
            }
        }
        Ask::Waiting { name } => {
            row.x += 6.0;
            row.text(&format!("Waiting for Windows' permission to end {name}..."), c.text);
        }
        Ask::Note(note) => {
            row.x += 6.0;
            row.text(&note, c.warn);
            if row.chip("OK", None, Chip::Soft, true, None) {
                st.actions.cancel();
            }
        }
        Ask::Nothing => commands(&mut row, st),
    }
}

/// The commands for the selection, or a line about the list when nothing is selected.
fn commands(row: &mut Row, st: &mut State) {
    let c = colors();
    let Some(t) = st.list.target() else {
        let apps = st.list.groups.iter().filter(|g| g.app).count();
        row.x += 6.0;
        row.text(&format!("{} processes, {apps} apps. Select one to end it or see its details.", st.list.procs.len()), c.muted);
        // What reading them all took, for keeping an eye on this window's own cost.
        let took = format!("Read in {:.1} ms", st.took.as_secs_f64() * 1000.0);
        let p = row.ui.painter();
        p.text(pos2(row.right, row.y), Align2::RIGHT_CENTER, took, FontId::proportional(12.0), c.muted);
        return;
    };
    let end = actions::verdict(&st.list, &actions::affected(&st.list, t, Act::End), st.own_pid);
    let tree = actions::verdict(&st.list, &actions::affected(&st.list, t, Act::Tree), st.own_pid);
    let tip = |v: &Verdict, fallback: &'static str| v.reason().unwrap_or(fallback).to_owned();
    let has_windows = st.list.target_procs(t).iter().any(|&i| !st.list.procs[i].windows.is_empty());
    let end_tip = tip(&end, if has_windows { "Ask its windows to close (Delete)" } else { "End it at once (Delete)" });
    if row.chip("End task", Some(glyph::CLOSE), Chip::Danger, end.can_end(), Some(&end_tip)) {
        st.actions.start(&st.list, Act::End, &st.control, st.own_pid);
    }
    let tree_tip = tip(&tree, "End it and every process it started");
    if row.chip("End tree", Some(TREE), Chip::Soft, tree.can_end(), Some(&tree_tip)) {
        st.actions.start(&st.list, Act::Tree, &st.control, st.own_pid);
    }
    let p = &st.list.procs[t.proc];
    let exe = p.exe.clone();
    let folder = exe.as_ref().and_then(|e| e.parent().map(|d| d.to_path_buf()));
    if row.chip("Open location", Some(glyph::FOLDER), Chip::Soft, folder.is_some(), Some(if folder.is_some() { "Show it in Files" } else { "Its path cannot be read" })) {
        if let Some(exe) = exe.as_ref().filter(|_| folder.is_some()) {
            if !shellman_core::sibling::start_with("files", &["--select", &exe.to_string_lossy()]) {
                st.actions.ask = Ask::Note("Files (shellman-files.exe) was not found next to Task Manager.".into());
            }
        }
    }
    if row.chip("Copy path", Some(glyph::COPY), Chip::Soft, exe.is_some(), Some("Copy the program's full path")) {
        if let Some(e) = &exe {
            row.ui.ctx().copy_text(e.display().to_string());
        }
    }
    let pid = p.key.pid;
    if row.chip("Copy PID", None, Chip::Soft, true, Some("Copy the process ID")) {
        row.ui.ctx().copy_text(pid.to_string());
    }
    let details = if st.list.details { "Hide details" } else { "Details" };
    if row.chip(details, Some(INFO), Chip::Plain, true, Some("Command line, user, start time, threads, handles... (Enter)")) {
        st.list.toggle_details();
    }
    if let Some(reason) = end.reason() {
        row.text(reason, c.muted);
    }
}
