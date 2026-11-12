//! The Fonts tab: every installed family, each written in its own letters, the one in use first.
//! Only the rows in view are drawn, and only their samples are loaded (by the reader, never in
//! the frame); a row whose sample is not loaded yet is written in the window's font meanwhile.

use super::fonts_logic::{self, visible_range};
use super::fonts_view::FontsView;
use egui::{Align2, CornerRadius, FontFamily, FontId, Sense, Vec2};
use shellman_sysfont::fonts::catalog::{Family, Origin};
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::draw_text;

const ROW_H: f32 = 52.0;
const GAP: f32 = 4.0;

pub fn show(ui: &mut egui::Ui, v: &mut FontsView) {
    controls(ui, v);
    ui.add_space(10.0);
    let Some(catalog) = v.page.catalog.clone() else {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(16.0));
            ui.label(theme::muted("Reading the installed fonts..."));
        });
        return;
    };
    let shown = fonts_logic::order(&catalog.families, &v.query, v.in_use());
    if shown.is_empty() {
        ui.label(theme::muted("No font matches the search."));
        return;
    }
    let stride = ROW_H + GAP;
    ui.spacing_mut().item_spacing.y = GAP;
    let top = ui.cursor().top();
    let clip = ui.clip_rect();
    let range = visible_range(clip.top(), clip.bottom(), top, stride, shown.len());
    let spacer = |ui: &mut egui::Ui, rows: usize| {
        if rows > 0 {
            ui.allocate_space(Vec2::new(1.0, rows as f32 * stride - GAP));
        }
    };
    let in_view: Vec<&Family> = range.clone().map(|i| &catalog.families[shown[i]]).collect();
    v.want_tiles(&in_view);
    spacer(ui, range.start);
    let mut clicked = None;
    for family in in_view {
        if row(ui, v, family) {
            clicked = Some(family);
        }
    }
    spacer(ui, shown.len() - range.end);
    if let Some(family) = clicked {
        v.select(family);
    }
}

/// The search field, the count and "Add font file...".
fn controls(ui: &mut egui::Ui, v: &mut FontsView) {
    let c = colors();
    ui.horizontal(|ui| {
        let add_w = 140.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new((ui.available_width() - add_w - 8.0).max(120.0), 34.0), Sense::hover());
        ui.painter().rect_filled(rect, CornerRadius::same(10), c.control);
        ui.painter().text(egui::pos2(rect.left() + 18.0, rect.center().y), Align2::CENTER_CENTER, glyph::SEARCH, icon_font(14.0), c.muted);
        let edit_rect = egui::Rect::from_min_max(egui::pos2(rect.left() + 36.0, rect.top()), egui::pos2(rect.right() - 10.0, rect.bottom()));
        let edit = egui::TextEdit::singleline(&mut v.query)
            .id(egui::Id::new("fonts_search"))
            .font(FontId::proportional(14.0))
            .frame(false)
            .margin(egui::Margin::ZERO)
            .vertical_align(egui::Align::Center)
            .hint_text(egui::RichText::new("Search fonts").color(c.muted))
            .desired_width(edit_rect.width());
        ui.put(edit_rect, edit);
        let label = if v.picking() { "Choosing a file..." } else { "Add font file..." };
        let button = egui::Button::new(egui::RichText::new(label).size(13.0)).corner_radius(CornerRadius::same(10)).min_size(Vec2::new(add_w, 34.0));
        if ui.add_enabled(!v.picking() && !v.busy(), button).on_hover_text("Try a .ttf, .otf or .ttc file without installing it. A copy is kept with the fonts you added.").clicked() {
            v.pick_font_file(ui.ctx());
        }
    });
    if let Some(catalog) = &v.page.catalog {
        ui.add_space(6.0);
        ui.label(theme::muted(format!("{} fonts. Icons, emoji and symbols always keep Windows' own fonts.", catalog.families.len())).size(12.0));
    }
}

/// One family: a sample, its name in its own letters, what it has, and whether it is in use.
/// True when clicked.
fn row(ui: &mut egui::Ui, v: &FontsView, family: &Family) -> bool {
    let c = colors();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
    let selected = v.selected.as_ref().is_some_and(|(n, o)| *n == family.name && *o == family.origin);
    let fill = if selected {
        c.row_selected
    } else if resp.hovered() {
        c.control_hover
    } else {
        c.control
    };
    ui.painter().rect_filled(rect, CornerRadius::same(12), fill);
    if selected {
        ui.painter().rect_stroke(rect, CornerRadius::same(12), egui::Stroke::new(1.5_f32, c.accent_fill), egui::StrokeKind::Inside);
    }
    let own = v.fonts.family(&fonts_logic::tile_key(family)).unwrap_or(FontFamily::Proportional);
    let tile = egui::Rect::from_center_size(egui::pos2(rect.left() + 32.0, rect.center().y), Vec2::new(40.0, 40.0));
    ui.painter().rect_filled(tile, CornerRadius::same(10), theme::with_alpha(c.text, 12));
    ui.painter().text(tile.center(), Align2::CENTER_CENTER, "Aa", FontId::new(20.0, own.clone()), c.text);

    let using = v.in_use().is_some_and(|n| n.eq_ignore_ascii_case(&family.name)) && family.origin == Origin::Installed;
    let mut tags: Vec<&str> = Vec::new();
    if using {
        tags.push("In use");
    }
    if family.origin == Origin::Added {
        tags.push("Added");
    }
    if family.restricted() {
        tags.push("License limits copies");
    }
    let tag_w = tags.iter().map(|t| t.len() as f32 * 6.8 + 22.0).sum::<f32>();
    let text_left = tile.right() + 12.0;
    let text_right = (rect.right() - tag_w - 12.0).max(text_left + 60.0);
    let line = |top: f32, bottom: f32| egui::Rect::from_min_max(egui::pos2(text_left, top), egui::pos2(text_right, bottom));
    draw_text(ui, line(rect.top() + 6.0, rect.center().y + 2.0), &family.name, FontId::new(15.0, own), c.text, false);
    draw_text(ui, line(rect.center().y + 2.0, rect.bottom() - 6.0), &family.summary(), FontId::proportional(11.5), c.muted, false);
    let mut x = rect.right() - 12.0;
    for tag in tags.iter().rev() {
        let w = tag.len() as f32 * 6.8 + 16.0;
        let r = egui::Rect::from_min_max(egui::pos2(x - w, rect.center().y - 11.0), egui::pos2(x, rect.center().y + 11.0));
        let color = if *tag == "In use" { c.accent_text } else { c.muted };
        ui.painter().rect_filled(r, CornerRadius::same(11), theme::with_alpha(color, 30));
        ui.painter().text(r.center(), Align2::CENTER_CENTER, *tag, FontId::proportional(11.5), color);
        x -= w + 6.0;
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}
