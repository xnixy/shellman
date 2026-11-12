//! The bottom of the panel: the battery, when the machine has one, and a row of round buttons
//! (Settings, Lock, Sleep, Power). Power opens Restart / Shut down / Sign out inline, each asking
//! for a second click.

use super::kit::{self, chip, clipped_text, label_font, small_font, CHIP, GAP};
use crate::power_menu::{self, PowerMenu, CHOICES};
use egui::{Align2, CornerRadius, Pos2, Rect, Sense, Ui, Vec2};
use shellman_core::system::power::Action;
use shellman_core::system::status::Battery;
use shellman_ui::theme::{colors, glyph};
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
    });
}

pub fn show(ui: &mut Ui, menu: &mut PowerMenu) -> Option<Asked> {
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
        if let Some(action) = choices(ui, menu) {
            asked = Some(Asked::Power(action));
        }
    }
    asked
}

/// Restart, Shut down, Sign out, side by side. Returns a confirmed choice.
fn choices(ui: &mut Ui, menu: &mut PowerMenu) -> Option<Action> {
    let c = colors();
    let now = Instant::now();
    menu.expire(now);
    if let Some(deadline) = menu.deadline() {
        // One wake-up to disarm on time; no repeating timer.
        ui.ctx().request_repaint_after(deadline.saturating_duration_since(now));
    }
    let width = ui.available_width();
    let (row, _) = ui.allocate_exact_size(Vec2::new(width, CHOICE_HEIGHT), Sense::hover());
    let w = (width - GAP * 2.0) / 3.0;
    let mut confirmed = None;
    for (i, action) in CHOICES.into_iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(row.left() + i as f32 * (w + GAP), row.top()), Vec2::new(w, CHOICE_HEIGHT));
        let resp = ui.interact(r, egui::Id::new(("power", i)), Sense::click());
        let armed = menu.armed() == Some(action);
        let fill = match (armed, resp.hovered()) {
            (true, _) => c.danger,
            (false, true) => kit::well(true),
            (false, false) => kit::tile_fill(false),
        };
        let fg = if armed { egui::Color32::WHITE } else { c.text };
        ui.painter().rect_filled(r, CornerRadius::same((CHOICE_HEIGHT / 2.0) as u8), fill);
        let text = power_menu::label(action, armed);
        ui.painter().text(r.center(), Align2::CENTER_CENTER, text, small_font(), fg);
        if resp.clicked() {
            confirmed = menu.press(action, now);
        }
    }
    confirmed
}
