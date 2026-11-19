//! Terminal: shellman Terminal's own settings (`terminal.json`, which running terminals follow as
//! it changes), which terminal shellman's "Open Terminal here" opens, and whether Windows hands
//! console programs to shellman Terminal (its default terminal).
//!
//! The file is read whole and written whole with only the fields shown here changed, so fields
//! this page does not know are kept. The console switch shows the registry's current choice and
//! runs the installed terminal's own `--register-default-terminal` /
//! `--unregister-default-terminal` (`shellman_core::terminal::handoff`), off the UI thread.

use crate::page;
use crate::state::State;
use shellman_core::terminal::file::File;
use shellman_core::terminal::{handoff, profiles, Choice, Which};
use shellman_ui::theme;
use shellman_ui::widgets::toggle;
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

    page::group(ui, "Profiles", |ui| {
        profile(ui, st);
        colours(ui, st);
    });
    page::group(ui, "Font", |ui| font(ui, st));
    page::group(ui, "History and copying", |ui| history(ui, st));
    page::group(ui, "Opening", |ui| {
        new_instance(ui, st);
        open_with(ui, st);
    });
    page::group(ui, "Console programs", |ui| consoles(ui, st));
}

/// How wide a row's own control (a drop-down, a field, a slider) is, at most.
const CONTROL_W: f32 = 260.0;

fn profile(ui: &mut egui::Ui, st: &mut State) {
    let v = &mut st.terminal;
    let Some(f) = v.file.as_mut() else { return };
    let current = f.default_profile();
    let mut picked = None;
    page::row(ui, "Default profile", "What a new tab runs.", |ui| {
        let resp = egui::ComboBox::from_id_salt("terminal-profile").width(CONTROL_W.min(ui.available_width() * 0.5)).selected_text(&current).show_ui(ui, |ui| {
            for name in &v.profiles {
                if ui.selectable_label(name.eq_ignore_ascii_case(&current), name).clicked() {
                    picked = Some(name.clone());
                }
            }
        });
        resp.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Default profile, {current}")));
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
    let mut changed = false;
    let edit = page::row(ui, "Family", "Empty for Cascadia Mono (then Cascadia Code, then Consolas).", |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut v.family)
                .hint_text(theme::muted("Cascadia Mono"))
                .desired_width(CONTROL_W.min(ui.available_width() * 0.5)),
        )
    });
    shellman_ui::widgets::name_field(&edit, "Font family", Some(&v.family));
    let done = edit.lost_focus() || (edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
    if done && v.family.trim() != f.font_family() {
        f.set("font_family", v.family.trim());
        changed = true;
    }
    let pt = f.font_size().clamp(FONT_MIN, FONT_MAX);
    let at = (pt - FONT_MIN) / (FONT_MAX - FONT_MIN);
    let moved = page::row(ui, "Size", "", |ui| {
        // Right to left: the value at the row's end, the slider before it.
        ui.add_sized(egui::vec2(44.0, 20.0), egui::Label::new(theme::muted(format!("{pt:.0} pt")).size(12.5)));
        ui.add_space(6.0);
        shellman_ui::slider::thin(ui, egui::Id::new("terminal-font-size"), at, Some(CONTROL_W.min(ui.available_width() * 0.5)), true).named("Size")
    });
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

fn colours(ui: &mut egui::Ui, st: &mut State) {
    use shellman_core::terminal::file::COLOR_SCHEMES;
    let v = &mut st.terminal;
    let Some(f) = v.file.as_mut() else { return };
    let current = f.color_scheme();
    let shown = COLOR_SCHEMES.iter().find(|(n, _)| *n == current).map_or(COLOR_SCHEMES[0].1, |(_, label)| label);
    let mut picked = None;
    page::row(ui, "Colours", "The 16 colours programs use. Text in them is kept readable on the glass.", |ui| {
        let resp = egui::ComboBox::from_id_salt("terminal-colours").width(CONTROL_W.min(ui.available_width() * 0.5)).selected_text(shown).show_ui(ui, |ui| {
            for (name, label) in COLOR_SCHEMES {
                if ui.selectable_label(name == current, label).clicked() {
                    picked = Some(name);
                }
            }
        });
        resp.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Colour scheme, {shown}")));
    });
    if let Some(name) = picked.filter(|n| *n != current) {
        f.set("color_scheme", name);
        let f = f.clone();
        st.saver.run(move || f.save());
    }
}

fn history(ui: &mut egui::Ui, st: &mut State) {
    let v = &mut st.terminal;
    let Some(f) = v.file.as_mut() else { return };
    let mut lines = f.scrollback();
    let mut options: Vec<(u64, String)> = SCROLLBACK.iter().map(|&n| (n, group_digits(n))).collect();
    if !SCROLLBACK.contains(&lines) {
        options.push((lines, group_digits(lines)));
        options.sort_by_key(|o| o.0);
    }
    let shown: Vec<(u64, &str)> = options.iter().map(|(n, s)| (*n, s.as_str())).collect();
    let mut changed = false;
    if page::row(ui, "Lines of history kept", "", |ui| page::segmented(ui, &mut lines, &shown)) {
        f.set("scrollback", lines);
        changed = true;
    }
    let mut copy = f.copy_on_select();
    let title = "Copy text as soon as it is selected";
    if page::row(ui, title, "", |ui| toggle(ui, &mut copy, title)).changed() {
        f.set("copy_on_select", copy);
        changed = true;
    }
    if changed {
        save(st, st.terminal.file.as_ref().unwrap());
    }
}

fn new_instance(ui: &mut egui::Ui, st: &mut State) {
    let v = &mut st.terminal;
    let Some(f) = v.file.as_mut() else { return };
    let mut tab = f.new_instance() == "tab";
    let picked = page::row(ui, "New terminal opens in", "Starting the terminal while one is open.", |ui| {
        page::segmented(ui, &mut tab, &[(false, "A new window"), (true, "A new tab")])
    });
    if picked {
        f.set("new_instance", if tab { "tab" } else { "window" });
        let f = f.clone();
        st.saver.run(move || f.save());
    }
}

fn open_with(ui: &mut egui::Ui, st: &mut State) {
    let v = &mut st.terminal;
    let Some(f) = v.file.as_mut() else { return };
    let mut choice: Choice = f.choice();
    let mut changed = page::row(ui, "Open Terminal here uses", "What the desktop's and Files' \u{201C}Open Terminal here\u{201D} start.", |ui| {
        page::segmented(
            ui,
            &mut choice.default_terminal,
            &[(Which::Shellman, "shellman Terminal"), (Which::WindowsTerminal, "Windows Terminal"), (Which::Custom, "A command")],
        )
    });
    if choice.default_terminal == Which::Custom {
        let edit = page::row(ui, "Command line", "{dir} is the folder.", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut v.custom)
                    .hint_text(theme::muted("\"C:\\Tools\\wezterm.exe\" start --cwd {dir}"))
                    .font(egui::TextStyle::Monospace)
                    .desired_width((ui.available_width() * 0.6).min(420.0)),
            )
        });
        shellman_ui::widgets::name_field(&edit, "Command line", Some(&v.custom));
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
    let title = "Use shellman Terminal for all console programs";
    let resp = page::row(ui, title, "Needs Windows Terminal installed: its console host hands each console over.", |ui| {
        ui.add_enabled_ui(can, |ui| toggle(ui, &mut on, title)).inner
    });
    let line = if busy {
        "Changing it\u{2026}".to_owned()
    } else if exe.is_none() {
        "shellman-terminal.exe is not beside Settings, so this cannot change here.".to_owned()
    } else if !v.wt_installed && !on {
        format!("{} Windows Terminal is not installed.", state.describe())
    } else {
        state.describe()
    };
    page::row_with(ui, |ui| {
        ui.add(egui::Label::new(theme::muted(line).size(12.5)).wrap());
        if let Some(note) = &v.note {
            ui.add(egui::Label::new(egui::RichText::new(note).size(12.5).color(theme::colors().danger_text)).wrap());
        }
    });
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
