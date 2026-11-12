//! The Fonts page's other tabs: the preview, the face for each weight, tuning, which parts of
//! Windows get the font, and the backups. Each edits the choices in memory and saves them on the
//! change worker; nothing here touches Windows.

use super::fonts_logic::{self, PARTS, WEIGHT_ROWS};
use super::fonts_preview;
use super::fonts_view::{Confirm, FontsView};
use crate::page;
use egui::{Align, Layout, RichText};
use shellman_sysfont::fonts::catalog::Origin;
use shellman_sysfont::machine;
use shellman_sysfont::plan::{self, FaceChoice};
use shellman_sysfont::{settings, system};
use shellman_ui::slider;
use shellman_ui::theme::{self, colors};
use shellman_ui::widgets::switch;

fn need_family(ui: &mut egui::Ui, v: &FontsView) -> bool {
    if v.family().is_some() {
        return true;
    }
    ui.label(theme::muted(if v.page.catalog.is_none() { "Reading the installed fonts..." } else { "Choose a font on the Fonts tab first." }));
    false
}

fn building(ui: &mut egui::Ui, v: &FontsView) {
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(16.0));
        let name = v.family().map(|f| f.name.clone()).unwrap_or_default();
        ui.label(theme::muted(format!("Building {name} the way Windows would get it...")));
    });
}

/// File Explorer, a message box and a Settings page in the faces Windows would get.
pub fn preview(ui: &mut egui::Ui, v: &mut FontsView) {
    if !need_family(ui, v) {
        return;
    }
    ui.horizontal(|ui| {
        let mut segoe = v.compare;
        let name = v.family().map(|f| f.name.clone()).unwrap_or_default();
        if page::segmented(ui, &mut segoe, &[(false, name.as_str()), (true, "Segoe UI now")]) {
            v.compare = segoe;
        }
        if v.preview_busy() && v.preview_ready() {
            ui.add(egui::Spinner::new().size(14.0));
        }
    });
    ui.add_space(10.0);
    if !v.preview_ready() && !v.compare {
        building(ui, v);
        return;
    }
    let faces = v.faces(v.compare);
    if !faces.is_ready() {
        building(ui, v);
        ui.ctx().request_repaint();
        return;
    }
    fonts_preview::explorer(ui, &faces);
    ui.add_space(12.0);
    fonts_preview::settings_page(ui, &faces);
    for problem in &v.preview_problems {
        ui.add_space(6.0);
        ui.label(RichText::new(problem).size(12.0).color(colors().danger_text));
    }
}

/// Which face stands in for each Segoe UI weight, with a choice by hand.
pub fn weights(ui: &mut egui::Ui, v: &mut FontsView) {
    if !need_family(ui, v) {
        return;
    }
    let faces = v.faces(false);
    if faces.is_ready() {
        fonts_preview::weights(ui, &faces);
        ui.add_space(12.0);
    }
    ui.label(theme::muted("Each Segoe UI weight gets the closest face. A variable font is set to the exact weight; a face picked here is used instead.").size(12.0));
    ui.add_space(8.0);
    let Some(family) = v.family().cloned() else { return };
    let mut changed = false;
    for (label, weight, italic) in WEIGHT_ROWS {
        let key = plan::override_key(weight, italic);
        let mapped = plan::map(&family, weight, italic, None, &v.settings.overrides, &v.settings.tune);
        ui.horizontal(|ui| {
            ui.add_sized([130.0, 24.0], egui::Label::new(RichText::new(label).size(13.0).color(colors().text)));
            let now = mapped.as_ref().map(|m| m.describe()).unwrap_or_else(|| "None".into());
            let by_hand = v.settings.overrides.contains_key(&key);
            let shown = if by_hand { now.clone() } else { format!("Automatic: {now}") };
            egui::ComboBox::from_id_salt(("weight-face", &key)).width(260.0).selected_text(shown).show_ui(ui, |ui| {
                if ui.selectable_label(!by_hand, "Automatic").clicked() && by_hand {
                    v.settings.overrides.remove(&key);
                    changed = true;
                }
                for face in &family.faces {
                    let picked = v.settings.overrides.get(&key).is_some_and(|c| c.path == face.path && c.index == face.index);
                    let text = if face.is_variable() { format!("{} (variable)", face.style) } else { face.style.clone() };
                    if ui.selectable_label(picked, text).clicked() {
                        let w = face.is_variable().then_some(weight as f32);
                        v.settings.overrides.insert(key.clone(), FaceChoice { path: face.path.clone(), index: face.index, weight: w });
                        changed = true;
                    }
                }
            });
        });
    }
    if changed {
        v.save();
    }
}

/// A slider row: label, the slider and its value. Returns the new 0..1 value while it moves.
fn slider_row(ui: &mut egui::Ui, id: &str, label: &str, t: f32, value: &str) -> (Option<f32>, bool) {
    let mut out = (None, false);
    ui.horizontal(|ui| {
        ui.add_sized([150.0, 26.0], egui::Label::new(RichText::new(label).size(13.0).color(colors().text)));
        let width = (ui.available_width() - 70.0).max(120.0);
        let s = slider::thin(ui, egui::Id::new(("fonts-slider", id)), t, Some(width), true);
        out = (s.value, s.dragging());
        ui.label(theme::muted(value).size(12.5));
    });
    out
}

/// Glyph size, line height, slanted italics, and the size of each kind of window text.
pub fn tune(ui: &mut egui::Ui, v: &mut FontsView) {
    let mut changed = false;
    let mut moving = false;
    page::label(ui, "Letters");
    let size = v.settings.tune.size;
    let (to, drag) = slider_row(ui, "size", "Glyph size", fonts_logic::slider_from_size(size), &format!("{size}%"));
    if let Some(t) = to {
        let new = fonts_logic::size_from_slider(t);
        changed |= new != size;
        v.settings.tune.size = new;
    }
    moving |= drag;
    ui.label(theme::muted("For fonts that look small next to Segoe UI.").size(12.0));
    ui.add_space(10.0);
    changed |= switch(ui, &mut v.settings.tune.match_line_height, "Match Segoe UI's line height").changed();
    ui.label(theme::muted("So text sits right in buttons and menus made for Segoe UI.").size(12.0));
    ui.add_space(8.0);
    changed |= switch(ui, &mut v.settings.tune.synth_italic, "Slant the upright face for missing italics").changed();
    page::gap(ui);

    page::label(ui, "Window text, in points");
    let sizes = &mut v.settings.text_sizes;
    for (id, label, value) in [
        ("title", "Title bars", &mut sizes.title),
        ("menu", "Menus", &mut sizes.menu),
        ("message", "Message boxes", &mut sizes.message),
        ("status", "Tooltips and status bars", &mut sizes.status),
        ("icon", "Icon labels", &mut sizes.icon),
    ] {
        let (to, drag) = slider_row(ui, id, label, fonts_logic::slider_from_points(*value), &format!("{value:.1} pt"));
        if let Some(t) = to {
            let new = fonts_logic::points_from_slider(t);
            changed |= new != *value;
            *value = new;
        }
        moving |= drag;
    }
    ui.label(theme::muted("Windows 11 has no setting for these; 9 pt is Windows' own size.").size(12.0));
    // Saved once the slider is let go, not on every step of a drag.
    if changed {
        v.edited = true;
    }
    if v.edited && !moving {
        v.edited = false;
        v.save();
    }
}

/// A switch for each part of Windows, with what it covers and what it needs.
pub fn coverage(ui: &mut egui::Ui, v: &mut FontsView) {
    let c = colors();
    let mut changed = false;
    for part in &PARTS {
        let mut on = (part.get)(&v.settings.layers);
        if switch(ui, &mut on, part.label).changed() {
            (part.set)(&mut v.settings.layers, on);
            changed = true;
        }
        ui.label(RichText::new(part.what).size(12.5).color(c.text));
        ui.label(theme::muted(format!("Needs: {}", part.needs)).size(12.0));
        ui.add_space(12.0);
    }
    if let Some(family) = v.family() {
        if family.origin == Origin::Added && v.settings.layers.window_text && !v.settings.layers.segoe {
            ui.label(RichText::new(format!("{} is not installed, so window text can use it only with Apps and File Explorer on.", family.name)).size(12.5).color(c.warn));
            ui.add_space(8.0);
        }
    }
    ui.label(RichText::new("Icons, emoji and symbols keep Windows' fonts").size(13.0).color(c.text));
    ui.label(theme::muted("Segoe Fluent Icons, Segoe MDL2 Assets, Segoe UI Emoji, Segoe UI Symbol and Segoe UI Historic are left alone, so buttons and emoji never turn into boxes. Letters the new font lacks, like Chinese or Arabic, still come from Windows' own fonts.").size(12.0));
    if changed {
        v.save();
    }
}

/// Restore, fonts used before, the restore file, and the check at sign-in.
pub fn backups(ui: &mut egui::Ui, v: &mut FontsView) {
    let c = colors();
    let s = v.page.status.clone();
    page::label(ui, "Windows' own font");
    ui.horizontal(|ui| {
        let can = s.current.is_some() || v.page.window_text_ours;
        if ui.add_enabled(can && !v.busy(), theme::primary_button("Restore Segoe UI")).clicked() {
            v.confirm = Some(Confirm::Restore);
        }
        ui.label(theme::muted(if s.backup { "Windows' original entries are saved, and are what Restore puts back." } else { "Saved before the first change to apps or the shell." }).size(12.0));
    });
    page::gap(ui);

    page::label(ui, "Use again");
    let current = s.current.as_ref().map(|a| a.generation.clone());
    let earlier: Vec<_> = s.history.iter().filter(|a| Some(&a.generation) != current.as_ref()).collect();
    if earlier.is_empty() {
        ui.label(theme::muted("Fonts used for apps and the shell show up here, ready to use again.").size(12.0));
    }
    for applied in earlier {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&applied.family).size(13.5).color(c.text));
            ui.label(theme::muted(format!("{}, {}", applied.summary, system::local_stamp(applied.applied_at))).size(12.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let saved = v.page.reusable.contains(&applied.generation);
                if ui.add_enabled(saved && !v.busy(), egui::Button::new("Use again")).on_disabled_hover_text("Its files are no longer saved; choose it on the Fonts tab instead.").clicked() {
                    v.reuse(&applied.generation);
                }
            });
        });
    }
    page::gap(ui);

    page::label(ui, "Restore file");
    let reg = machine::restore_reg_path();
    ui.label(RichText::new(reg.display().to_string()).size(12.5).color(c.text));
    ui.label(theme::muted("If Windows ever looks wrong, double-click it and restart: it puts Windows' fonts back without shellman, even from Safe Mode.").size(12.0));
    ui.add_space(4.0);
    if ui.add_enabled(reg.parent().is_some_and(|d| d.is_dir()), egui::Button::new("Open in Files")).clicked() {
        if let Some(dir) = reg.parent() {
            open_in_files(dir);
        }
    }
    page::gap(ui);

    page::label(ui, "At sign-in");
    let mut on = v.page.check_on;
    if switch(ui, &mut on, "Check the font when I sign in").changed() {
        v.set_check(on);
    }
    ui.label(theme::muted("Sets window text again if a theme change reset it, and opens this page if a Windows update put Segoe UI back. It runs for a moment and ends.").size(12.0));
    if v.page.fontman_check_on {
        ui.add_space(4.0);
        ui.label(RichText::new("Fontman's own check is still on (the Fontman value under Run). Once this one is on, it can be turned off in Fontman.").size(12.0).color(c.warn));
    }
    if settings::dry_run() {
        ui.add_space(4.0);
        ui.label(theme::muted("Dry run: this switch changes a stand-in, not the Run key.").size(11.5));
    }
}

/// Open a folder in shellman's Files, beside this exe.
fn open_in_files(dir: &std::path::Path) {
    let files = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("shellman-files.exe")));
    if let Some(files) = files.filter(|f| f.is_file()) {
        let _ = std::process::Command::new(files).arg(dir).spawn();
    }
}
