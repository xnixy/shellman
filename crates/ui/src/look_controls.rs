//! The controls that edit the shared look: the accent color swatches and the glass slider. Files'
//! Appearance dialog and the settings app's Appearance page both use them.

use crate::theme::{self, colors, glyph, icon_font, AccentChoice};
use egui::widgets::color_picker::{color_picker_color32, Alpha};
use egui::{Align2, Color32, Id, RichText, Sense, Stroke, Ui, Vec2};

/// The accent swatches (Windows' own, the presets, a custom color) with a color picker that opens
/// under them, and a line naming the choice. Returns the new choice in the frame it is made.
pub fn accent_picker(ui: &mut Ui, current: AccentChoice) -> Option<AccentChoice> {
    let c = colors();
    let picker_id = Id::new("custom-picker");
    let mut open = ui.data(|d| d.get_temp::<bool>(picker_id)).unwrap_or(false);
    let mut pick = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
        let sys = theme::system_accent().unwrap_or(c.accent);
        if swatch(
            ui,
            Swatch::System(sys),
            current == AccentChoice::System,
            "Windows accent",
        )
        .clicked()
        {
            pick = Some(AccentChoice::System);
        }
        ui.add_space(6.0);
        for (i, (name, rgb)) in theme::PRESETS.iter().enumerate() {
            let col = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            if swatch(
                ui,
                Swatch::Solid(col),
                current == AccentChoice::Preset(i),
                name,
            )
            .clicked()
            {
                pick = Some(AccentChoice::Preset(i));
            }
        }
        // The eleventh swatch opens the picker, and shows the custom color once chosen.
        let kind = match current {
            AccentChoice::Custom(v) => Swatch::Solid(Color32::from_rgb(v[0], v[1], v[2])),
            _ => Swatch::Add,
        };
        let custom = matches!(current, AccentChoice::Custom(_));
        if swatch(ui, kind, custom, "Custom color").clicked() {
            open = !open;
        }
    });
    ui.data_mut(|d| d.insert_temp(picker_id, open));
    ui.add_space(2.0);
    let label = match current {
        AccentChoice::System => "Following Windows",
        AccentChoice::Preset(i) => theme::PRESETS[i.min(theme::PRESETS.len() - 1)].0,
        AccentChoice::Custom(_) => "Custom",
    };
    ui.label(theme::muted(label).size(12.0));
    if open {
        ui.add_space(6.0);
        // Live: every change restyles the app as a custom accent.
        let mut col = c.accent;
        if color_picker_color32(ui, &mut col, Alpha::Opaque) {
            pick = Some(AccentChoice::Custom([col.r(), col.g(), col.b()]));
        }
    }
    pick
}

/// How much of the acrylic shows through, from solid to clear. Without system backdrops
/// (Windows 10) there is no glass to show, so `enabled` is false and the slider says why. The
/// caller applies `changed()` as the slider moves, and shares the look once the drag is over.
pub fn glass_slider(ui: &mut Ui, glass: &mut f32, enabled: bool) -> egui::Response {
    let c = colors();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Solid").size(11.5).color(c.muted));
        ui.spacing_mut().slider_width = 240.0;
        let slider = egui::Slider::new(glass, 0.0..=1.0)
            .show_value(false)
            .trailing_fill(true);
        let mut resp = ui.add_enabled(enabled, slider);
        ui.label(RichText::new("Clear").size(11.5).color(c.muted));
        if !enabled {
            resp = resp.on_disabled_hover_text("Needs Windows 11");
        }
        resp
    })
    .inner
}

/// What a swatch is filled with.
enum Swatch {
    Solid(Color32),
    /// Half the system accent, half neutral: it stands for "whatever Windows uses".
    System(Color32),
    /// The custom-color button, before a custom color exists.
    Add,
}

fn swatch(ui: &mut Ui, kind: Swatch, selected: bool, tip: &str) -> egui::Response {
    let c = colors();
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
    let center = rect.center();
    let radius = if resp.hovered() { 13.5 } else { 12.5 };
    let p = ui.painter();
    let under = match kind {
        Swatch::Solid(col) => {
            p.circle_filled(center, radius, col);
            col
        }
        Swatch::System(col) => {
            p.add(half_disc(center, radius, true, col));
            p.add(half_disc(
                center,
                radius,
                false,
                theme::with_alpha(c.text, 40),
            ));
            col
        }
        Swatch::Add => {
            p.circle_filled(center, radius, c.control);
            p.text(
                center,
                Align2::CENTER_CENTER,
                glyph::ADD,
                icon_font(12.0),
                c.text,
            );
            c.control
        }
    };
    if selected {
        p.circle_stroke(center, 15.0, Stroke::new(2.0_f32, c.text));
        let on = theme::Colors::new(c.light, under).on_accent;
        p.text(
            center,
            Align2::CENTER_CENTER,
            glyph::CHECK,
            icon_font(11.0),
            on,
        );
    }
    resp.on_hover_text(tip)
}

/// One half of a disc as a convex polygon, so two of them make a split swatch.
fn half_disc(center: egui::Pos2, radius: f32, left: bool, fill: Color32) -> egui::Shape {
    use std::f32::consts::PI;
    let start = if left { PI / 2.0 } else { -PI / 2.0 };
    let points = (0..=16)
        .map(|i| {
            let a = start + PI * i as f32 / 16.0;
            center + Vec2::new(a.cos(), a.sin()) * radius
        })
        .collect();
    egui::Shape::convex_polygon(points, fill, Stroke::NONE)
}
