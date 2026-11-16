//! The cards view: each item its own rounded card, with its preview filling the upper part and
//! its name and a muted detail below, laid out in as many columns as fit. The cards have no
//! panel behind them; they float on the window's faint glass.
//!
//! Only the lines on screen are laid out and drawn, and only their previews are asked for, so a
//! folder of 100 000 photos scrolls as smoothly as one of ten.

use super::actions::Action;
use super::interact::{self, ItemEvents};
use super::rows::file_icon;
use super::select;
use super::tab::Loc;
use super::text;
use egui::{Align2, Color32, CornerRadius, FontId, Id, Rect, Stroke, Ui, Vec2};
use shellman_core::fsops::Entry;
use shellman_core::thumbs;
use shellman_ui::glass::islands::shows_as;
use shellman_ui::theme::{self, colors, icon_font, semibold_font, Colors};
use shellman_ui::widgets::{self, draw_text};

/// Smallest card width; cards stretch to fill the line exactly.
const CARD_MIN_W: f32 = 150.0;
/// Between cards (the design's 18 px at its scale).
const GAP: f32 = 14.0;
/// Around the preview, inside the card.
const INSET: f32 = 8.0;
/// Below the preview: the name and the detail lines.
const TEXT_H: f32 = 48.0;
const CARD_RADIUS: u8 = super::RADIUS as u8;
const PREVIEW_RADIUS: u8 = 14;

/// Columns, card width and card height for a line `width` wide.
pub(super) fn fit(width: f32) -> (usize, f32, f32) {
    let cols = (((width + GAP) / (CARD_MIN_W + GAP)) as usize).max(1);
    let card_w = ((width + GAP) / cols as f32 - GAP).max(1.0);
    let preview_h = ((card_w - INSET * 2.0) * 0.7).round();
    (cols, card_w, INSET + preview_h + TEXT_H)
}

/// A card's fill and line, by its state. Selected: the accent at a fifth, ringed by a lighter
/// accent, as the design has it.
fn card_look(c: &Colors, selected: bool, hovered: bool) -> (Color32, Stroke) {
    // High contrast: no tints; every card outlined in the text colour, the selected one in the
    // highlight.
    if c.high_contrast {
        return if selected {
            (c.bg, Stroke::new(2.0_f32, c.accent_fill))
        } else {
            (c.bg, Stroke::new(1.0_f32, c.text))
        };
    }
    // As the design gives them (CSS colours), converted for the window (see `shows_as`).
    let css = |r: u8, g: u8, b: u8, a: u8| shows_as(Color32::from_rgba_unmultiplied(r, g, b, a));
    if selected {
        let [r, g, b, _] = c.accent_fill.to_array();
        let fill = css(r, g, b, if c.light { 40 } else { 51 });
        let ring = if c.light {
            c.accent_fill
        } else {
            theme::mix(c.accent_fill, Color32::WHITE, 0.25)
        };
        return (fill, Stroke::new(2.0_f32, ring));
    }
    let (fill, line) = if c.light {
        (
            css(255, 255, 255, if hovered { 170 } else { 120 }),
            Color32::from_black_alpha(16),
        )
    } else {
        (
            css(255, 255, 255, if hovered { 18 } else { 10 }),
            css(255, 255, 255, 16),
        )
    };
    (fill, Stroke::new(1.0_f32, line))
}

impl super::FileMan {
    pub(super) fn file_grid(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let c = colors();
        let clip = &self.clip;
        let days = text::today();
        let tab = &mut self.tabs[self.active];
        let rename = &mut self.rename;
        let thumbs = &mut self.thumbs;
        let counts = &mut self.counts;
        let tags = &self.tags;
        let mut ev = ItemEvents::default();

        if interact::error_state(ui, tab) {
            return;
        }
        interact::folder_backdrop(ui, self.content_island, tab, actions);
        if interact::loading_state(ui, tab) || interact::empty_state(ui, tab) {
            return;
        }

        let bin = tab.loc == Loc::Bin;
        let width = ui.available_width();
        let (cols, card_w, card_h) = fit(width);
        let line_h = card_h + GAP;
        self.grid_cols = cols;
        self.grid_line_h = line_h;
        let n = tab.view.len();
        let lines = n.div_ceil(cols);
        let many = tab.selected.len() > 1;
        let take_focus = self.take_focus;
        let mut has_focus = false;
        let mut tabbed_in = None;

        let scroll = interact::scroll_to_cursor(tab, line_h, cols);
        let out = scroll.show_rows(ui, line_h, lines, |ui, line_range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for line in line_range {
                // Space only, not a widget: a line is no control of its own.
                let (_, line_rect) = ui.allocate_space(Vec2::new(width, line_h));
                for col in 0..cols {
                    let r = line * cols + col;
                    if r >= n {
                        break;
                    }
                    let e = &tab.entries[tab.view[r]];
                    let card = Rect::from_min_size(
                        line_rect.min + Vec2::new(col as f32 * (card_w + GAP), 0.0),
                        Vec2::new(card_w, card_h),
                    );
                    // Drags too, so a drag that starts on a card is the card's, never a
                    // rubber band.
                    let resp = ui.interact(card, Id::new(("card", r)), interact::item_sense(tab, r));
                    if !bin {
                        super::dnd::item_zone(ui, card, e, super::RADIUS);
                    }
                    let dim = interact::fade(e.hidden, clip.is_cut(&e.path));
                    let selected = tab.selected.contains(&e.path);
                    let (fill, stroke) = card_look(&c, selected, resp.hovered());
                    ui.painter().rect(
                        card,
                        CornerRadius::same(CARD_RADIUS),
                        fill,
                        stroke,
                        egui::StrokeKind::Inside,
                    );
                    let items = if e.is_dir && !bin { counts.get(&e.path) } else { None };
                    let date = text::friendly_date(&e.modified_text, &days.0, &days.1);
                    let name = text::spoken_item(e, &date, items);
                    has_focus |= interact::item_access(ui, &resp, &name, selected, take_focus);
                    theme::focus_ring(ui, &resp, card, CARD_RADIUS as f32);
                    if resp.gained_focus() && tab.selected.is_empty() {
                        tabbed_in = Some(r);
                    }
                    // The keyboard's place, when it is not plainly the one selected item.
                    if tab.cursor == Some(r) && (many || (!selected && !tab.selected.is_empty())) {
                        ui.painter().rect_stroke(
                            card.shrink(3.0),
                            CornerRadius::same(CARD_RADIUS - 3),
                            Stroke::new(1.0_f32, theme::with_alpha(c.accent_text, 160)),
                            egui::StrokeKind::Inside,
                        );
                    }

                    let preview = Rect::from_min_max(
                        card.min + Vec2::splat(INSET),
                        egui::pos2(card.right() - INSET, card.bottom() - TEXT_H),
                    );
                    let thumb = (!e.is_dir && thumbs::wants_thumb(&e.name))
                        .then(|| thumbs.get(&e.path, true).map(|t| (t.id(), t.size_vec2())))
                        .flatten();
                    paint_preview(ui, preview, e, thumb, &c, &dim);

                    // Name and detail, or the rename box.
                    let dots = super::tag_ui::dots_width(tags.mask(&e.path));
                    let name = Rect::from_min_max(
                        egui::pos2(card.left() + 14.0, preview.bottom() + 6.0),
                        egui::pos2(card.right() - 12.0 - dots, preview.bottom() + 25.0),
                    );
                    match rename.as_mut().filter(|rn| rn.path == e.path) {
                        Some(rn) => {
                            let field = Rect::from_min_size(
                                egui::pos2(card.left() + 8.0, name.top()),
                                Vec2::new(card_w - 16.0, 24.0),
                            );
                            interact::rename_box(ui, field, rn, e.is_dir, &mut ev);
                        }
                        None => {
                            let font = semibold_font(13.0);
                            let shown = text::middle_ellipsis(&e.name, |s| {
                                ui.fonts(|f| f.layout_no_wrap(s.to_owned(), font.clone(), c.text))
                                    .size()
                                    .x
                                    <= name.width()
                            });
                            draw_text(ui, name, &shown, font, dim(c.text), false);
                        }
                    }
                    super::tag_ui::paint_dots(
                        ui.painter(),
                        egui::pos2(card.right() - 14.0, name.center().y),
                        tags.mask(&e.path),
                        true,
                    );
                    let below = Rect::from_min_max(
                        egui::pos2(name.left(), name.bottom()),
                        egui::pos2(card.right() - 12.0, name.bottom() + 17.0),
                    );
                    draw_text(
                        ui,
                        below,
                        &text::card_detail(e, tab.sort, items, &days),
                        FontId::proportional(11.5),
                        dim(c.muted),
                        false,
                    );

                    let resp = if e.name.chars().count() > 18 {
                        resp.on_hover_text_at_pointer(&e.name)
                    } else {
                        resp
                    };
                    ev.record(ui, &resp, r, actions);
                    interact::item_menu(ui, &resp, actions, r, bin);
                }
            }
        });
        shellman_ui::widgets::name_scroll_bar(ui.ctx(), &out, "the folder");
        tab.list_offset = out.state.offset.y;
        tab.list_height = out.inner_rect.height();
        tab.view_rect = out.inner_rect;
        if let Some(r) = tabbed_in {
            tab.select_only(r);
        }
        self.row_focus |= has_focus;
        let tab = &mut self.tabs[self.active];
        let layout = select::Layout {
            count: n,
            cols,
            item: Vec2::new(card_w, card_h),
            step: Vec2::new(card_w + GAP, line_h),
        };
        interact::rubber_band(ui, tab, layout);
        ev.apply(tab);
        self.finish_rename(&ev);
        if let Some(r) = ev.drag_out {
            self.drag_out(r);
        }
    }
}

/// The upper part of a card: a folder drawing, a photo filling it, a program's icon, or the
/// type's glyph, on a faint rounded well.
fn paint_preview(
    ui: &Ui,
    rect: Rect,
    e: &Entry,
    thumb: Option<(egui::TextureId, Vec2)>,
    c: &Colors,
    dim: &impl Fn(Color32) -> Color32,
) {
    let well = if c.light {
        Color32::from_black_alpha(10)
    } else {
        shows_as(Color32::from_rgba_unmultiplied(255, 255, 255, 8))
    };
    let radius = CornerRadius::same(PREVIEW_RADIUS);
    let p = ui.painter();
    if e.is_dir {
        p.rect_filled(rect, radius, well);
        let w = (rect.width() * 0.36).min(rect.height() * 0.56 / 0.74);
        let icon = Rect::from_center_size(rect.center(), Vec2::new(w, w * 0.74));
        widgets::folder_icon(p, icon, c.accent_text, dim);
        return;
    }
    if let Some((id, size)) = thumb {
        if thumbs::is_program(&e.name) {
            p.rect_filled(rect, radius, well);
            let max = Vec2::splat(56.0_f32.min(rect.height() * 0.6));
            let fitted = size * (max.x / size.x).min(max.y / size.y);
            egui::Image::from_texture((id, fitted))
                .tint(dim(Color32::WHITE))
                .paint_at(ui, Rect::from_center_size(rect.center(), fitted));
        } else {
            // A photo fills the well, cropped to it.
            let uv = cover_uv(size, rect.size());
            egui::Image::from_texture((id, size))
                .uv(uv)
                .corner_radius(radius)
                .tint(dim(Color32::WHITE))
                .paint_at(ui, rect);
        }
        return;
    }
    let (g, gcol) = file_icon(e, c);
    p.rect_filled(
        rect,
        radius,
        dim(shows_as(theme::with_alpha(
            gcol,
            if c.light { 34 } else { 40 },
        ))),
    );
    let size = (rect.height() * 0.36).clamp(20.0, 40.0);
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        g,
        icon_font(size),
        dim(gcol),
    );
}

/// The part of an image of `image` size that covers a box of `target` size, centred, in texture
/// coordinates.
pub(super) fn cover_uv(image: Vec2, target: Vec2) -> Rect {
    if image.x <= 0.0 || image.y <= 0.0 || target.x <= 0.0 || target.y <= 0.0 {
        return Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    }
    let (ia, ta) = (image.x / image.y, target.x / target.y);
    if ia > ta {
        let w = ta / ia;
        Rect::from_min_max(
            egui::pos2((1.0 - w) / 2.0, 0.0),
            egui::pos2((1.0 + w) / 2.0, 1.0),
        )
    } else {
        let h = ia / ta;
        Rect::from_min_max(
            egui::pos2(0.0, (1.0 - h) / 2.0),
            egui::pos2(1.0, (1.0 + h) / 2.0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_fill_the_line_exactly() {
        for width in [150.0, 400.0, 777.0, 1600.0] {
            let (cols, w, h) = fit(width);
            assert!(w >= CARD_MIN_W || cols == 1);
            assert!((cols as f32 * w + (cols - 1) as f32 * GAP - width).abs() < 0.01);
            assert!(h > w * 0.6, "a card is taller than a strip");
        }
        assert_eq!(fit(100.0).0, 1);
    }

    #[test]
    fn a_photo_is_cropped_to_cover_its_well() {
        // A wide photo in a squarer well: its middle, full height.
        let uv = cover_uv(Vec2::new(400.0, 100.0), Vec2::new(200.0, 100.0));
        assert!((uv.width() - 0.5).abs() < 1e-5 && uv.height() == 1.0);
        assert!((uv.center().x - 0.5).abs() < 1e-5);
        // A tall one: its middle, full width.
        let uv = cover_uv(Vec2::new(100.0, 400.0), Vec2::new(100.0, 100.0));
        assert!((uv.height() - 0.25).abs() < 1e-5 && uv.width() == 1.0);
        assert_eq!(cover_uv(Vec2::ZERO, Vec2::splat(10.0)).width(), 1.0);
    }

    #[test]
    fn selected_cards_are_ringed_and_tinted() {
        let c = Colors::new(false, Color32::from_rgb(59, 130, 246));
        let (fill, stroke) = card_look(&c, true, false);
        assert_eq!(stroke.width, 2.0);
        assert!(fill.a() > 0 && fill.a() < 80);
        let (_, plain) = card_look(&c, false, false);
        assert_eq!(plain.width, 1.0);
    }
}
