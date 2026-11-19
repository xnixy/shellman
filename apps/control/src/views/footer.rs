//! The bottom of the panel: the battery, when the machine has one, and a row of round buttons
//! (Settings, Lock, Sleep, Power). Power opens Restart / Shut down / Sign out inline, each asking
//! for a second click.

use super::kit::{self, chip, clipped_text, label_font, small_font, CHIP, GAP};
use crate::power_menu::{self, PowerMenu, CHOICES};
use egui::{Align2, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use shellman_core::system::power::Action;
use shellman_core::system::status::Battery;
use shellman_ui::chips::soft_colors;
use shellman_ui::glass::islands::shows_as;
use shellman_ui::theme::{colors, glyph, with_alpha};
use std::time::Instant;

const BUTTON: f32 = 38.0;
const CHOICE_HEIGHT: f32 = 32.0;

/// What a footer click asks for.
pub enum Asked {
    Settings,
    Power(Action),
}

pub fn battery(ui: &mut Ui, b: Battery) {
    let c = colors();
    kit::tile(ui, |ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), CHIP), Sense::hover());
        let center = Pos2::new(rect.left() + CHIP / 2.0, rect.center().y);
        chip(ui, center, if b.charging { glyph::CHARGING } else { glyph::BATTERY }, b.charging, false);
        let left = rect.left() + CHIP + 10.0;
        let width = rect.right() - left;
        clipped_text(ui, Pos2::new(left, rect.center().y - 1.0), Align2::LEFT_BOTTOM, "Battery", label_font(), c.text, width);
        let detail = format!("{}%{}", b.percent, if b.charging { " · Charging" } else { "" });
        clipped_text(ui, Pos2::new(left, rect.center().y + 1.0), Align2::LEFT_TOP, &detail, small_font(), c.muted, width);
        let resp = ui.interact(rect, egui::Id::new("battery"), Sense::hover());
        shellman_ui::theme::speak(&resp, egui::WidgetType::Label, || format!("Battery, {}%{}", b.percent, if b.charging { ", charging" } else { "" }));
    });
}

pub fn show(ui: &mut Ui, menu: &mut PowerMenu, restart_pending: bool) -> Option<Asked> {
    let mut asked = None;
    let width = ui.available_width();
    let (row, _) = ui.allocate_exact_size(Vec2::new(width, BUTTON), Sense::hover());
    let buttons: [(char, &str, Option<Asked>); 4] = [
        (glyph::SETTINGS, "shellman Settings", Some(Asked::Settings)),
        (glyph::LOCK, "Lock", Some(Asked::Power(Action::Lock))),
        (glyph::MOON, "Sleep", Some(Asked::Power(Action::Sleep))),
        (glyph::POWER, "Restart, shut down or sign out", None),
    ];
    // Spread evenly across the row.
    let step = (width - BUTTON) / (buttons.len() - 1) as f32;
    for (i, (g, tip, what)) in buttons.into_iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(row.left() + i as f32 * step, row.top()), Vec2::splat(BUTTON));
        let power = what.is_none();
        let resp = ui.put(r, |ui: &mut Ui| kit::round_button(ui, g, BUTTON, power && menu.is_open(), tip));
        if resp.clicked() {
            match what {
                Some(a) => asked = Some(a),
                None => menu.toggle(),
            }
        }
    }
    if menu.is_open() {
        ui.add_space(GAP - ui.spacing().item_spacing.y);
        if let Some(action) = choices(ui, menu, restart_pending) {
            asked = Some(Asked::Power(action));
        }
    }
    asked
}

/// Restart, Shut down, Sign out, side by side, with "Update and restart" on a row of its own above
/// them while updates wait for a restart. Returns a confirmed choice.
fn choices(ui: &mut Ui, menu: &mut PowerMenu, restart_pending: bool) -> Option<Action> {
    let now = Instant::now();
    menu.expire(now);
    if let Some(deadline) = menu.deadline() {
        // One wake-up to disarm on time; no repeating timer.
        ui.ctx().request_repaint_after(deadline.saturating_duration_since(now));
    }
    let width = ui.available_width();
    let mut confirmed = None;
    if let Some(action) = power_menu::extra_choice(restart_pending) {
        let (row, _) = ui.allocate_exact_size(Vec2::new(width, CHOICE_HEIGHT), Sense::hover());
        confirmed = choice(ui, menu, action, row, 0, now);
        ui.add_space(GAP - ui.spacing().item_spacing.y);
    }
    let (row, _) = ui.allocate_exact_size(Vec2::new(width, CHOICE_HEIGHT), Sense::hover());
    let w = (width - GAP * 2.0) / 3.0;
    for (i, action) in CHOICES.into_iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(row.left() + i as f32 * (w + GAP), row.top()), Vec2::new(w, CHOICE_HEIGHT));
        confirmed = choice(ui, menu, action, r, i + 1, now).or(confirmed);
    }
    confirmed
}

/// One pill in `r`; a click is `PowerMenu::press`, which says whether it confirms.
fn choice(ui: &mut Ui, menu: &mut PowerMenu, action: Action, r: Rect, slot: usize, now: Instant) -> Option<Action> {
    let c = colors();
    let resp = ui.interact(r, egui::Id::new(("power", slot)), Sense::click());
    let armed = menu.armed() == Some(action);
    // The label says what a click does now ("Click again to restart" once armed).
    shellman_ui::theme::speak(&resp, egui::WidgetType::Button, || power_menu::label(action, armed).to_string());
    let radius = CHOICE_HEIGHT / 2.0;
    shellman_ui::theme::focus_ring(ui, &resp, r, radius);
    // Armed, it is Glasshouse's destructive action: red text on a faint red wash inside a
    // hairline. High contrast: highlight under highlight text (`danger` is highlight there).
    let (_, _, line) = soft_colors(c.light);
    let (fill, fg, stroke) = match (armed, resp.hovered()) {
        (true, _) if c.high_contrast => (c.danger, c.on_accent, Stroke::NONE),
        (true, _) => (shows_as(with_alpha(c.danger_text, 46)), c.danger_text, line),
        (false, hovered) => (kit::field(hovered), c.text, Stroke::NONE),
    };
    ui.painter().rect(r, CornerRadius::same(radius as u8), fill, stroke, egui::StrokeKind::Inside);
    if !armed {
        shellman_ui::theme::contrast_outline(ui, r, radius, true, false);
        if c.high_contrast && !resp.hovered() {
            ui.painter().rect_stroke(r, radius, egui::Stroke::new(1.0_f32, c.text), egui::StrokeKind::Inside);
        }
    }
    ui.painter().text(r.center(), Align2::CENTER_CENTER, power_menu::label(action, armed), small_font(), fg);
    if resp.clicked() {
        return menu.press(action, now);
    }
    None
}
