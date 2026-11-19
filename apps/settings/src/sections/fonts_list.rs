//! The Fonts tab: every installed family, each written in its own letters, the one in use first.
//! Only the rows in view are drawn, and only their samples are loaded (by the reader, never in
//! the frame); a row whose sample is not loaded yet is written in the window's font meanwhile.

use super::fonts_logic::{self, visible_range};
use super::fonts_view::FontsView;
use egui::{Align2, CornerRadius, FontFamily, FontId, Sense, Vec2};
use shellman_sysfont::fonts::catalog::{Family, Origin};
use crate::page;
use shellman_ui::theme::{self, colors, glyph, icon_font, shape};
use shellman_ui::widgets::draw_text;

/// The height of a family item's content: the sample tile. The list adds its own padding.
const ROW_H: f32 = 40.0;

pub fn show(ui: &mut egui::Ui, v: &mut FontsView) {
    page::group(ui, "", |ui| {
        controls(ui, v);
        let Some(catalog) = v.page.catalog.clone() else {
            page::row_with(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(16.0));
                    ui.label(theme::muted("Reading the installed fonts..."));
                });
            });
            return;
        };
        let shown = fonts_logic::order(&catalog.families, &v.query, v.in_use());
        if shown.is_empty() {
            page::row_with(ui, |ui| {
                ui.label(theme::muted("No font matches the search."));
            });
            return;
        }
        let stride = page::item_stride(ROW_H);
        // Items sit flush: each is exactly one stride, which the spacers count on.
        ui.spacing_mut().item_spacing.y = 0.0;
        let top = ui.cursor().top();
        let clip = ui.clip_rect();
        let range = visible_range(clip.top(), clip.bottom(), top, stride, shown.len());
        let spacer = |ui: &mut egui::Ui, rows: usize| {
            if rows > 0 {
                ui.allocate_space(Vec2::new(1.0, rows as f32 * stride));
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
    });
}

/// The search field, the count and "Add font file...", as the first rows of the list's box.
fn controls(ui: &mut egui::Ui, v: &mut FontsView) {
    let c = colors();
    page::row_with(ui, |ui| {
        ui.horizontal(|ui| {
            let add_w = 140.0;
            let (rect, _) = ui.allocate_exact_size(Vec2::new((ui.available_width() - add_w - 8.0).max(120.0), 34.0), Sense::hover());
            page::field_fill(ui, rect);
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
            let resp = ui.put(edit_rect, edit);
            shellman_ui::widgets::name_field(&resp, "Search fonts", Some(&v.query));
            let label = if v.picking() { "Choosing a file..." } else { "Add font file..." };
            let button = egui::Button::new(egui::RichText::new(label).size(13.0)).corner_radius(CornerRadius::same(17)).min_size(Vec2::new(add_w, 34.0));
            if ui.add_enabled(!v.picking() && !v.busy(), button).on_hover_text("Try a .ttf, .otf or .ttc file without installing it. A copy is kept with the fonts you added.").clicked() {
                v.pick_font_file(ui.ctx());
            }
        });
    });
    if let Some(catalog) = &v.page.catalog {
        page::row_with(ui, |ui| {
            ui.add(egui::Label::new(theme::muted(format!("{} fonts. Icons, emoji and symbols always keep Windows' own fonts.", catalog.families.len())).size(12.5)).wrap());
        });
    }
}

/// One family, an item of the list: a sample, its name in its own letters, what it has, and
/// whether it is in use. True when clicked.
fn row(ui: &mut egui::Ui, v: &FontsView, family: &Family) -> bool {
    let c = colors();
    let selected = v.selected.as_ref().is_some_and(|(n, o)| *n == family.name && *o == family.origin);
    // High contrast: the item draws a highlight outline when selected, never a fill under text.
    page::item(ui, selected, |ui| {
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &family.name));
        theme::focus_ring(ui, &resp, rect.expand2(Vec2::new(6.0, 4.0)), shape::TILE);
        let own = v.fonts.family(&fonts_logic::tile_key(family)).unwrap_or(FontFamily::Proportional);
        let tile = egui::Rect::from_center_size(egui::pos2(rect.left() + 20.0, rect.center().y), Vec2::new(40.0, 40.0));
        // The sample's tile: the field fill, which the theme has already corrected for the glass.
        if !c.high_contrast {
            ui.painter().rect_filled(tile, CornerRadius::same(10), c.field);
        }
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
        let text_right = (rect.right() - tag_w).max(text_left + 60.0);
        let line = |top: f32, bottom: f32| egui::Rect::from_min_max(egui::pos2(text_left, top), egui::pos2(text_right, bottom));
        draw_text(ui, line(rect.top(), rect.center().y + 2.0), &family.name, FontId::new(15.0, own), c.text, false);
        draw_text(ui, line(rect.center().y + 2.0, rect.bottom()), &family.summary(), FontId::proportional(12.0), c.muted, false);
        let mut x = rect.right();
        for tag in tags.iter().rev() {
            let w = tag.len() as f32 * 6.8 + 16.0;
            let r = egui::Rect::from_min_max(egui::pos2(x - w, rect.center().y - 11.0), egui::pos2(x, rect.center().y + 11.0));
            let color = if *tag == "In use" { c.accent_text } else { c.muted };
            ui.painter().rect_filled(r, CornerRadius::same(11), theme::with_alpha(color, 30));
            ui.painter().text(r.center(), Align2::CENTER_CENTER, *tag, FontId::proportional(11.5), color);
            x -= w + 6.0;
        }
        resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    })
}
