//! The Fonts page's other tabs: the preview, the face for each weight, tuning, which parts of
//! Windows get the font, and the backups. Each edits the choices in memory and saves them on the
//! change worker; nothing here touches Windows.

use shellman_ui::menu::dropdown::dropdown;
use super::fonts_logic::{self, PARTS, WEIGHT_ROWS};
use super::fonts_preview;
use super::fonts_view::{Confirm, FontsView};
use crate::page;
use egui::RichText;
use shellman_sysfont::fonts::catalog::Origin;
use shellman_sysfont::machine;
use shellman_sysfont::plan::{self, FaceChoice};
use shellman_sysfont::{settings, system};
use shellman_ui::slider;
use shellman_ui::theme::{self, colors};
use shellman_ui::widgets::toggle;

fn need_family(ui: &mut egui::Ui, v: &FontsView) -> bool {
    if v.family().is_some() {
        return true;
    }
    page::group(ui, "", |ui| {
        page::row_with(ui, |ui| {
            ui.label(theme::muted(if v.page.catalog.is_none() { "Reading the installed fonts..." } else { "Choose a font on the Fonts tab first." }));
        });
    });
    false
}

fn building(ui: &mut egui::Ui, v: &FontsView) {
    page::row_with(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(16.0));
            let name = v.family().map(|f| f.name.clone()).unwrap_or_default();
            ui.label(theme::muted(format!("Building {name} the way Windows would get it...")));
        });
    });
}

/// File Explorer, a message box and a Settings page in the faces Windows would get. The mocks are
/// deliberate stand-ins for other apps, so they keep their own look inside their rows.
pub fn preview(ui: &mut egui::Ui, v: &mut FontsView) {
    if !need_family(ui, v) {
        return;
    }
    page::group(ui, "Preview", |ui| {
        let mut segoe = v.compare;
        let name = v.family().map(|f| f.name.clone()).unwrap_or_default();
        let picked = page::row(ui, "Show text in", "", |ui| {
            // Right to left: the spinner at the row's end, the choice before it.
            if v.preview_busy() && v.preview_ready() {
                ui.add(egui::Spinner::new().size(14.0));
            }
            page::segmented(ui, &mut segoe, &[(false, name.as_str()), (true, "Segoe UI now")])
        });
        if picked {
            v.compare = segoe;
        }
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
        page::row_with(ui, |ui| fonts_preview::explorer(ui, &faces));
        page::row_with(ui, |ui| fonts_preview::settings_page(ui, &faces));
        for problem in &v.preview_problems {
            page::row_with(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(problem).size(12.5).color(colors().danger_text)).wrap());
            });
        }
    });
}

/// Which face stands in for each Segoe UI weight, with a choice by hand.
pub fn weights(ui: &mut egui::Ui, v: &mut FontsView) {
    if !need_family(ui, v) {
        return;
    }
    let faces = v.faces(false);
    if faces.is_ready() {
        page::group(ui, "Preview", |ui| page::row_with(ui, |ui| fonts_preview::weights(ui, &faces)));
    }
    let Some(family) = v.family().cloned() else { return };
    let mut changed = false;
    page::group(ui, "Weights", |ui| {
        page::row_with(ui, |ui| {
            ui.add(egui::Label::new(theme::muted("Each Segoe UI weight gets the closest face. A variable font is set to the exact weight; a face picked here is used instead.").size(12.5)).wrap());
        });
        for (label, weight, italic) in WEIGHT_ROWS {
            let key = plan::override_key(weight, italic);
            let mapped = plan::map(&family, weight, italic, None, &v.settings.overrides, &v.settings.tune);
            let now = mapped.as_ref().map(|m| m.describe()).unwrap_or_else(|| "None".into());
            let by_hand = v.settings.overrides.contains_key(&key);
            let shown = if by_hand { now.clone() } else { format!("Automatic: {now}") };
            let mut rows: Vec<(String, bool)> = vec![("Automatic".to_string(), !by_hand)];
            rows.extend(family.faces.iter().map(|face| {
                let picked = v.settings.overrides.get(&key).is_some_and(|c| c.path == face.path && c.index == face.index);
                let text = if face.is_variable() { format!("{} (variable)", face.style) } else { face.style.clone() };
                (text, picked)
            }));
            let width = 260.0_f32.min(ui.available_width() * 0.6);
            let picked = page::row(ui, label, "", |ui| dropdown(ui, ("weight-face", &key), width, &shown, &rows));
            match picked {
                Some(0) if by_hand => {
                    v.settings.overrides.remove(&key);
                    changed = true;
                }
                Some(k) if k > 0 => {
                    let face = &family.faces[k - 1];
                    let w = face.is_variable().then_some(weight as f32);
                    v.settings.overrides.insert(key.clone(), FaceChoice { path: face.path.clone(), index: face.index, weight: w });
                    changed = true;
                }
                _ => {}
            }
        }
    });
    if changed {
        v.save();
    }
}

/// A slider row: label, a detail line, the slider and its value. Returns the new 0..1 value while
/// it moves, and whether the knob is held.
fn slider_row(ui: &mut egui::Ui, id: &str, label: &str, detail: &str, t: f32, value: &str) -> (Option<f32>, bool) {
    page::row(ui, label, detail, |ui| {
        // Right to left: the value at the row's end, the slider before it.
        ui.label(theme::muted(value).size(12.5));
        ui.add_space(6.0);
        let width = (ui.available_width() * 0.5).clamp(120.0, 260.0);
        let s = slider::thin(ui, egui::Id::new(("fonts-slider", id)), t, Some(width), true).named(label);
        (s.value, s.dragging())
    })
}

/// Glyph size, line height, slanted italics, and the size of each kind of window text.
pub fn tune(ui: &mut egui::Ui, v: &mut FontsView) {
    let mut changed = false;
    let mut moving = false;
    page::group(ui, "Letters", |ui| {
        let size = v.settings.tune.size;
        let (to, drag) = slider_row(ui, "size", "Glyph size", "For fonts that look small next to Segoe UI.", fonts_logic::slider_from_size(size), &format!("{size}%"));
        if let Some(t) = to {
            let new = fonts_logic::size_from_slider(t);
            changed |= new != size;
            v.settings.tune.size = new;
        }
        moving |= drag;
        let title = "Match Segoe UI's line height";
        changed |= page::row(ui, title, "So text sits right in buttons and menus made for Segoe UI.", |ui| toggle(ui, &mut v.settings.tune.match_line_height, title)).changed();
        let title = "Slant the upright face for missing italics";
        changed |= page::row(ui, title, "", |ui| toggle(ui, &mut v.settings.tune.synth_italic, title)).changed();
    });

    page::group(ui, "Window text, in points", |ui| {
        let sizes = &mut v.settings.text_sizes;
        for (id, label, value) in [
            ("title", "Title bars", &mut sizes.title),
            ("menu", "Menus", &mut sizes.menu),
            ("message", "Message boxes", &mut sizes.message),
            ("status", "Tooltips and status bars", &mut sizes.status),
            ("icon", "Icon labels", &mut sizes.icon),
        ] {
            let (to, drag) = slider_row(ui, id, label, "", fonts_logic::slider_from_points(*value), &format!("{value:.1} pt"));
            if let Some(t) = to {
                let new = fonts_logic::points_from_slider(t);
                changed |= new != *value;
                *value = new;
            }
            moving |= drag;
        }
        page::row_with(ui, |ui| {
            ui.add(egui::Label::new(theme::muted("Windows 11 has no setting for these; 9 pt is Windows' own size.").size(12.5)).wrap());
        });
    });
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
    page::group(ui, "Parts of Windows", |ui| {
        for part in &PARTS {
            let mut on = (part.get)(&v.settings.layers);
            let detail = format!("{}\nNeeds: {}", part.what, part.needs);
            if page::row(ui, part.label, &detail, |ui| toggle(ui, &mut on, part.label)).changed() {
                (part.set)(&mut v.settings.layers, on);
                changed = true;
            }
        }
        if let Some(family) = v.family() {
            if family.origin == Origin::Added && v.settings.layers.window_text && !v.settings.layers.segoe {
                page::row_with(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(format!("{} is not installed, so window text can use it only with Apps and File Explorer on.", family.name)).size(12.5).color(c.warn)).wrap());
                });
            }
        }
    });
    page::group(ui, "Kept as they are", |ui| {
        page::row(ui, "Icons, emoji and symbols keep Windows' fonts", "Segoe Fluent Icons, Segoe MDL2 Assets, Segoe UI Emoji, Segoe UI Symbol and Segoe UI Historic are left alone, so buttons and emoji never turn into boxes. Letters the new font lacks, like Chinese or Arabic, still come from Windows' own fonts.", |_| {});
    });
    if changed {
        v.save();
    }
}

/// Restore, fonts used before, the restore file, and the check at sign-in.
pub fn backups(ui: &mut egui::Ui, v: &mut FontsView) {
    let c = colors();
    let s = v.page.status.clone();
    page::group(ui, "Windows' own font", |ui| {
        let can = s.current.is_some() || v.page.window_text_ours;
        let detail = if s.backup { "Windows' original entries are saved, and are what Restore puts back." } else { "Saved before the first change to apps or the shell." };
        if page::row(ui, "Restore Segoe UI", detail, |ui| ui.add_enabled(can && !v.busy(), theme::primary_button("Restore Segoe UI")).clicked()) {
            v.confirm = Some(Confirm::Restore);
        }
    });

    page::group(ui, "Use again", |ui| {
        let current = s.current.as_ref().map(|a| a.generation.clone());
        let earlier: Vec<_> = s.history.iter().filter(|a| Some(&a.generation) != current.as_ref()).collect();
        if earlier.is_empty() {
            page::row_with(ui, |ui| {
                ui.add(egui::Label::new(theme::muted("Fonts used for apps and the shell show up here, ready to use again.").size(12.5)).wrap());
            });
        }
        for applied in earlier {
            let detail = format!("{}, {}", applied.summary, system::local_stamp(applied.applied_at));
            let saved = v.page.reusable.contains(&applied.generation);
            let again = page::row(ui, &applied.family, &detail, |ui| {
                ui.add_enabled(saved && !v.busy(), egui::Button::new("Use again")).on_disabled_hover_text("Its files are no longer saved; choose it on the Fonts tab instead.").clicked()
            });
            if again {
                v.reuse(&applied.generation);
            }
        }
    });

    page::group(ui, "Restore file", |ui| {
        let reg = machine::restore_reg_path();
        let detail = format!("{}\nIf Windows ever looks wrong, double-click it and restart: it puts Windows' fonts back without shellman, even from Safe Mode.", reg.display());
        let open = page::row(ui, "Restore file", &detail, |ui| ui.add_enabled(reg.parent().is_some_and(|d| d.is_dir()), egui::Button::new("Open in Files")).clicked());
        if open {
            if let Some(dir) = reg.parent() {
                open_in_files(dir);
            }
        }
    });

    page::group(ui, "At sign-in", |ui| {
        let mut on = v.page.check_on;
        let title = "Check the font when I sign in";
        if page::row(ui, title, "Sets window text again if a theme change reset it, and opens this page if a Windows update put Segoe UI back. It runs for a moment and ends.", |ui| toggle(ui, &mut on, title)).changed() {
            v.set_check(on);
        }
        if v.page.fontman_check_on {
            page::row_with(ui, |ui| {
                ui.add(egui::Label::new(RichText::new("Fontman's own check is still on (the Fontman value under Run). Once this one is on, it can be turned off in Fontman.").size(12.5).color(c.warn)).wrap());
            });
        }
        if settings::dry_run() {
            page::row_with(ui, |ui| {
                ui.add(egui::Label::new(theme::muted("Dry run: this switch changes a stand-in, not the Run key.").size(12.5)).wrap());
            });
        }
    });
}

/// Open a folder in shellman's Files, beside this exe.
fn open_in_files(dir: &std::path::Path) {
    let files = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("shellman-files.exe")));
    if let Some(files) = files.filter(|f| f.is_file()) {
        let _ = std::process::Command::new(files).arg(dir).spawn();
    }
}
