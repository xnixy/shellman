//! Terminal: shellman Terminal's own settings (`terminal.json`, which running terminals follow as
//! it changes), which terminal shellman's "Open Terminal here" opens, and whether Windows hands
//! console programs to shellman Terminal (its default terminal).
//!
//! The file is read whole and written whole with only the fields shown here changed, so fields
//! this page does not know are kept. The console switch shows the registry's current choice and
//! runs the installed terminal's own `--register-default-terminal` /
//! `--unregister-default-terminal` (`shellman_core::terminal::handoff`), off the UI thread.

use crate::page;
use crate::sections::slider_row;
use crate::state::State;
use shellman_core::terminal::file::File;
use shellman_core::terminal::{handoff, profiles, Choice, Which};
use shellman_ui::theme;
use shellman_ui::widgets::switch;
use std::sync::mpsc::{channel, Receiver};

/// History lengths offered; a value set by hand shows beside them.
const SCROLLBACK: [u64; 4] = [1_000, 10_000, 50_000, 100_000];
const FONT_MIN: f32 = 8.0;
const FONT_MAX: f32 = 32.0;

#[derive(Default)]
pub struct TerminalView {
    file: Option<File>,
    profiles: Vec<String>,
    /// The font family as typed, saved when the field is left.
    family: String,
    /// The custom command line as typed, saved when the field is left.
    custom: String,
    handoff: Option<handoff::State>,
    wt_installed: bool,
    /// The console switch's command, running.
    switching: Option<Receiver<Result<String, String>>>,
    note: Option<String>,
}

impl TerminalView {
    pub fn stop(&mut self) {
        // A switch still running keeps its thread; its answer is simply not shown.
        *self = Self::default();
    }

    fn start(&mut self) {
        if self.file.is_some() {
            return;
        }
        let f = File::load();
        self.family = f.font_family();
        self.custom = f.choice().custom_terminal;
        self.file = Some(f);
        self.profiles = profiles::detect().names();
        self.handoff = Some(handoff::current());
        self.wt_installed = handoff::windows_terminal_installed();
    }

    fn poll(&mut self) {
        if let Some(rx) = &self.switching {
            if let Ok(result) = rx.try_recv() {
                self.switching = None;
                self.handoff = Some(handoff::current());
                self.note = match result {
                    Ok(_) => None,
                    Err(e) => Some(format!("It did not change: {e}")),
                };
            }
        }
    }
}

fn save(st: &State, f: &File) {
    let f = f.clone();
    st.saver.run(move || f.save());
}

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Terminal", "shellman Terminal, and the terminal shellman opens.");
    st.terminal.start();
    st.terminal.poll();
    if st.terminal.switching.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }

    page::group(ui, "Default profile", |ui| profile(ui, st));
    page::group(ui, "Font", |ui| font(ui, st));
    page::group(ui, "History and copying", |ui| history(ui, st));
    page::group(ui, "New terminal opens in", |ui| {
        ui.label(theme::muted("Starting the terminal while one is open.").size(12.5));
        ui.add_space(8.0);
        let v = &mut st.terminal;
        let Some(f) = v.file.as_mut() else { return };
        let mut tab = f.new_instance() == "tab";
        if page::segmented(ui, &mut tab, &[(false, "A new window"), (true, "A new tab")]) {
            f.set("new_instance", if tab { "tab" } else { "window" });
            let f = f.clone();
            st.saver.run(move || f.save());
        }
    });
    page::group(ui, "Open Terminal here uses", |ui| open_with(ui, st));
    page::group(ui, "Console programs", |ui| consoles(ui, st));
}

fn profile(ui: &mut egui::Ui, st: &mut State) {
    let v = &mut st.terminal;
    let Some(f) = v.file.as_mut() else { return };
    ui.label(theme::muted("What a new tab runs.").size(12.5));
    ui.add_space(8.0);
    let current = f.default_profile();
    let mut picked = None;
    egui::ComboBox::from_id_salt("terminal-profile").width(300.0).selected_text(&current).show_ui(ui, |ui| {
        for name in &v.profiles {
            if ui.selectable_label(name.eq_ignore_ascii_case(&current), name).clicked() {
                picked = Some(name.clone());
            }
        }
    });
    if let Some(name) = picked {
        f.set("default_profile", name);
        let f = f.clone();
        st.saver.run(move || f.save());
    }
}

fn font(ui: &mut egui::Ui, st: &mut State) {
    let v = &mut st.terminal;
    let Some(f) = v.file.as_mut() else { return };
    page::label(ui, "Family");
    let edit = ui.add(
        egui::TextEdit::singleline(&mut v.family)
            .hint_text(theme::muted("Cascadia Mono"))
            .desired_width(300.0),
    );
    let done = edit.lost_focus() || (edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
    let mut changed = false;
    if done && v.family.trim() != f.font_family() {
        f.set("font_family", v.family.trim());
        changed = true;
    }
    ui.label(theme::muted("Empty for Cascadia Mono (then Cascadia Code, then Consolas).").size(12.0));
    page::gap(ui);
    let pt = f.font_size().clamp(FONT_MIN, FONT_MAX);
    let at = (pt - FONT_MIN) / (FONT_MAX - FONT_MIN);
    let moved = slider_row::show(ui, egui::Id::new("terminal-font-size"), "Size", at, &format!("{pt:.0} pt"));
    if let Some(a) = moved.value {
        let new = (FONT_MIN + a * (FONT_MAX - FONT_MIN)).round();
        if new != pt {
            f.set("font_size", new);
            changed = true;
        }
    }
    if changed {
        let f = f.clone();
        st.saver.run(move || f.save());
    }
}

fn history(ui: &mut egui::Ui, st: &mut State) {
    let v = &mut st.terminal;
    let Some(f) = v.file.as_mut() else { return };
    page::label(ui, "Lines of history kept");
    let mut lines = f.scrollback();
    let mut options: Vec<(u64, String)> = SCROLLBACK.iter().map(|&n| (n, group_digits(n))).collect();
    if !SCROLLBACK.contains(&lines) {
        options.push((lines, group_digits(lines)));
        options.sort_by_key(|o| o.0);
    }
    let shown: Vec<(u64, &str)> = options.iter().map(|(n, s)| (*n, s.as_str())).collect();
    let mut changed = false;
    if page::segmented(ui, &mut lines, &shown) {
        f.set("scrollback", lines);
        changed = true;
    }
    page::gap(ui);
    let mut copy = f.copy_on_select();
    if switch(ui, &mut copy, "Copy text as soon as it is selected").changed() {
        f.set("copy_on_select", copy);
        changed = true;
    }
    if changed {
        save(st, st.terminal.file.as_ref().unwrap());
    }
}

fn open_with(ui: &mut egui::Ui, st: &mut State) {
    let v = &mut st.terminal;
    let Some(f) = v.file.as_mut() else { return };
    ui.label(theme::muted("What the desktop's and Files' \u{201C}Open Terminal here\u{201D} start.").size(12.5));
    ui.add_space(8.0);
    let mut choice: Choice = f.choice();
    let mut changed = page::segmented(
        ui,
        &mut choice.default_terminal,
        &[(Which::Shellman, "shellman Terminal"), (Which::WindowsTerminal, "Windows Terminal"), (Which::Custom, "A command")],
    );
    if choice.default_terminal == Which::Custom {
        page::gap(ui);
        page::label(ui, "Command line ({dir} is the folder)");
        let edit = ui.add(
            egui::TextEdit::singleline(&mut v.custom)
                .hint_text(theme::muted("\"C:\\Tools\\wezterm.exe\" start --cwd {dir}"))
                .desired_width(ui.available_width().min(520.0)),
        );
        if (edit.lost_focus() || (edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && v.custom.trim() != choice.custom_terminal {
            choice.custom_terminal = v.custom.trim().to_owned();
            changed = true;
        }
    }
    if changed {
        f.set_choice(&choice);
        let f = f.clone();
        st.saver.run(move || f.save());
    }
}

fn consoles(ui: &mut egui::Ui, st: &mut State) {
    let v = &mut st.terminal;
    let state = v.handoff.clone().unwrap_or(handoff::State::WindowsDecides);
    let busy = v.switching.is_some();
    let exe = shellman_core::terminal::shellman_exe();
    let mut on = state.is_shellman();
    let can = !busy && exe.is_some() && (v.wt_installed || on);
    let resp = ui.add_enabled_ui(can, |ui| switch(ui, &mut on, "Use shellman Terminal for all console programs")).inner;
    ui.label(theme::muted("Needs Windows Terminal installed: its console host hands each console over.").size(12.0));
    ui.add_space(6.0);
    let line = if busy {
        "Changing it\u{2026}".to_owned()
    } else if exe.is_none() {
        "shellman-terminal.exe is not beside Settings, so this cannot change here.".to_owned()
    } else if !v.wt_installed && !on {
        format!("{} Windows Terminal is not installed.", state.describe())
    } else {
        state.describe()
    };
    ui.label(theme::muted(line).size(12.5));
    if let Some(note) = &v.note {
        ui.label(egui::RichText::new(note).size(12.5).color(theme::colors().danger_text));
    }
    if resp.changed() {
        if let Some(exe) = exe {
            let (tx, rx) = channel();
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let _ = tx.send(handoff::switch(&exe, on));
                ctx.request_repaint();
            });
            v.switching = Some(rx);
            v.note = None;
        }
    }
}

/// 100000 as "100 000".
fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('\u{2009}');
        }
        out.push(ch);
    }
    out
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_are_grouped() {
        assert_eq!(group_digits(1_000), "1\u{2009}000");
        assert_eq!(group_digits(100_000), "100\u{2009}000");
        assert_eq!(group_digits(500), "500");
    }

    #[test]
    fn the_size_slider_stays_within_what_the_terminal_takes() {
        use shellman_core::terminal::file;
        assert!(file::MIN_PT <= FONT_MIN && FONT_MAX <= file::MAX_PT);
    }
}
