//! Fonts: the font Windows uses everywhere, Fontman's window as one Settings page. A status
//! group on top says what Windows uses now; the tabs below (Fontman's pages: the fonts, the
//! preview, weights, tuning, which parts of Windows, and backups) scroll inside themselves, each
//! as boxed groups; and a command group at the bottom applies the choice. Reading and changing
//! are done by the workers (`fonts_worker`); this file only draws what they last published.

use super::fonts_logic::{self, Offer, Tab};
use super::fonts_view::{Confirm, FontsView};
use super::{fonts_list, fonts_tabs, Section};
use crate::page;
use crate::state::State;
use egui::{Align, Layout, RichText};
use shellman_sysfont::{settings, system};
use shellman_ui::theme::{self, colors};

/// The command group's height: one row of 32, a row's padding and the box's.
const PILL_H: f32 = 32.0 + 2.0 * 14.0 + 2.0 * 6.0 + 2.0;
/// The tab choice's height and the space under it.
const TABS_H: f32 = 32.0 + 12.0;
/// The least the tab's own scrolling area is given.
const TAB_MIN_H: f32 = 200.0;

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Fonts", "The font Windows uses everywhere: File Explorer, Settings, Start, the taskbar, title bars and menus.");
    let v = &mut st.fonts;
    v.start(ui.ctx());
    v.poll(ui.ctx());
    page::group(ui, "", |ui| status(ui, v, &mut st.goto));

    // The tabs fill what the status and the command group leave, and scroll inside themselves,
    // so the command group stays at the bottom of the window.
    let left = ui.clip_rect().bottom() - ui.cursor().top() - TABS_H - PILL_H - page::GROUP_GAP;
    let inner = left.max(TAB_MIN_H);
    page::segmented(ui, &mut v.tab, &Tab::ALL.map(|t| (t, t.label())));
    ui.add_space(12.0);
    let out = egui::ScrollArea::vertical()
        .id_salt("fonts-tab")
        .max_height(inner)
        .min_scrolled_height(inner)
        .auto_shrink([false, false])
        .show(ui, |ui| match v.tab {
            Tab::Fonts => fonts_list::show(ui, v),
            Tab::Preview => fonts_tabs::preview(ui, v),
            Tab::Weights => fonts_tabs::weights(ui, v),
            Tab::Tune => fonts_tabs::tune(ui, v),
            Tab::Coverage => fonts_tabs::coverage(ui, v),
            Tab::Backups => fonts_tabs::backups(ui, v),
        });
    shellman_ui::widgets::name_scroll_bar(ui.ctx(), &out, "the fonts");
    page::group(ui, "", |ui| page::row_with(ui, |ui| command_pill(ui, v)));
}

/// What Windows uses now, whether a restart is due, and the way back to Segoe UI.
fn status(ui: &mut egui::Ui, v: &mut FontsView, goto: &mut Option<Section>) {
    let c = colors();
    let s = v.page.status.clone();
    let mut lines = Vec::new();
    if let Some(current) = &s.current {
        lines.push(format!("Since {}.", system::local_stamp(current.applied_at)));
    }
    if let Some(text) = &v.page.window_text {
        lines.push(format!("Window text uses {}.", text.faces().join(", ")));
    }
    page::row(ui, &fonts_logic::status_title(&s), &lines.join("  "), |ui| {
        if s.restart_pending {
            chip(ui, "Restart needed", c.warn);
        }
        if settings::dry_run() {
            let note = format!("Dry run: nothing on this PC changes. The registry, fonts folder and window text are stand-ins in {}.", settings::dry_root().display());
            chip(ui, "Dry run", c.accent_text).on_hover_text(note);
        }
    });
    if s.restart_pending {
        page::row_with(ui, |ui| {
            ui.add(egui::Label::new(theme::muted("Restart later: Windows shows the change everywhere after its next restart. Until then some parts keep the font they had.").size(12.5)).wrap());
        });
    }
    if s.drift {
        page::row_with(ui, |ui| {
            ui.add(egui::Label::new(RichText::new("Windows put Segoe UI back since the font was last changed, probably during an update.").color(c.warn).size(12.5)).wrap());
        });
    }

    let can_restore = s.current.is_some() || v.page.window_text_ours;
    if !(can_restore || s.drift || s.current.is_some()) {
        return;
    }
    if v.confirm == Some(Confirm::Restore) {
        let needs = if settings::dry_run() { "In this dry run only the stand-ins change." } else { "Windows asks for administrator rights; restart to finish." };
        page::row(ui, "Put Segoe UI back everywhere?", needs, |ui| {
            // Right to left: Restore is the row's last button.
            if ui.button("Cancel").clicked() {
                v.confirm = None;
            }
            if ui.add(theme::danger_button("Restore")).clicked() {
                v.run(Confirm::Restore);
            }
        });
        return;
    }
    page::row_with(ui, |ui| {
        ui.horizontal(|ui| {
            let busy = v.busy();
            if can_restore && ui.add_enabled(!busy, theme::primary_button("Restore Segoe UI")).clicked() {
                v.confirm = Some(Confirm::Restore);
            }
            if s.drift && ui.add_enabled(!busy, egui::Button::new("Apply again")).clicked() {
                v.apply_again();
            }
            if s.current.is_some() && ui.button("Use this font for shellman too").on_hover_text("Shellman's own font is set on the Appearance page.").clicked() {
                *goto = Some(Section::Appearance);
            }
        });
    });
}

fn chip(ui: &mut egui::Ui, text: &str, color: egui::Color32) -> egui::Response {
    let fill = theme::with_alpha(color, 36);
    egui::Frame::new()
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(11))
        .inner_margin(egui::Margin::symmetric(10, 3))
        .show(ui, |ui| ui.label(RichText::new(text).size(12.0).color(color)))
        .response
}

/// The bottom group's row: what is going on (or went wrong), and Apply.
fn command_pill(ui: &mut egui::Ui, v: &mut FontsView) {
    let c = colors();
    let differs = v.family().is_some_and(|f| v.settings.differs_from_applied(&f.name));
    let offer = fonts_logic::offer(v.family().map(|f| f.name.as_str()), &v.page.status, differs);
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 32.0), Layout::left_to_right(Align::Center), |ui| {
        let right_w = if v.confirm == Some(Confirm::Apply) { 220.0 } else { 170.0 };
        let text_w = (ui.available_width() - right_w).max(80.0);
        ui.allocate_ui_with_layout(egui::vec2(text_w, 32.0), Layout::left_to_right(Align::Center), |ui| {
            ui.set_max_width(text_w);
            if let Some(job) = &v.page.job {
                ui.add(egui::Spinner::new().size(16.0).color(c.accent_text));
                let mut line = format!("{}: {}", job.what, job.stage);
                if let Some((done, total)) = job.progress {
                    line.push_str(&format!(" {done} of {total}"));
                }
                ui.add(egui::Label::new(RichText::new(line).size(12.5).color(c.text)).truncate());
            } else if v.confirm == Some(Confirm::Apply) {
                let needs = if settings::dry_run() {
                    "Dry run: builds and checks every face, then changes only the stand-ins."
                } else if v.settings.layers.machine() {
                    "Builds and checks every face, then Windows asks for administrator rights. Restart to finish."
                } else {
                    "Window text changes at once."
                };
                ui.add(egui::Label::new(RichText::new(needs).size(12.5).color(c.text)).wrap());
            } else if let Some(message) = &v.message {
                let (text, color) = match message {
                    Ok(text) => (text.as_str(), c.text),
                    Err(text) => (text.as_str(), c.danger_text),
                };
                ui.add(egui::Label::new(RichText::new(text).size(12.5).color(color)).truncate()).on_hover_text(text);
            } else if let Some(err) = &v.page.catalog_error {
                ui.add(egui::Label::new(RichText::new(err).size(12.5).color(c.danger_text)).truncate());
            } else if v.page.catalog.is_none() {
                ui.add(egui::Spinner::new().size(16.0).color(c.muted));
                ui.label(theme::muted("Reading the installed fonts...").size(12.5));
            } else {
                let what = match v.family() {
                    Some(f) => format!("{} for {}.", f.name, shellman_sysfont::plan::layers_summary(&v.settings.layers)),
                    None => "Choose a font on the Fonts tab.".into(),
                };
                ui.add(egui::Label::new(theme::muted(what).size(12.5)).truncate());
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if v.confirm == Some(Confirm::Apply) {
                if ui.add(theme::primary_button("Apply now")).clicked() {
                    v.run(Confirm::Apply);
                }
                if ui.button("Cancel").clicked() {
                    v.confirm = None;
                }
                return;
            }
            let enabled = offer.enabled() && !v.busy() && v.page.catalog.is_some();
            if ui.add_enabled(enabled, theme::primary_button(&offer.label())).clicked() {
                match offer {
                    Offer::Again(_) => v.apply_again(),
                    _ => v.confirm = Some(Confirm::Apply),
                }
            }
        });
    });
}
