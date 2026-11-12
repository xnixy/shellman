//! The icons view: tiles with large previews, laid out in as many columns as fit.
//!
//! Only the lines on screen are laid out and drawn, and only their previews are asked for, so a
//! folder of 100 000 photos scrolls as smoothly as one of ten.

use super::actions::Action;
use super::interact::{self, ItemEvents};
use super::rows::file_icon;
use super::tab::Loc;
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align, Align2, Color32, CornerRadius, FontId, Id, Rect, Sense, Ui, Vec2};
use shellman_core::fsops::Op;
use shellman_core::thumbs;
use shellman_ui::theme::{self, colors, icon_font};
use shellman_ui::widgets;

/// Smallest tile width; tiles stretch a little to fill the line exactly.
const TILE_W: f32 = 112.0;
const TILE_H: f32 = 128.0;
const GAP: f32 = 4.0;
/// Height of one line of tiles, used for scrolling and Page Up/Down.
pub(super) const GRID_LINE_H: f32 = TILE_H + GAP;
const PREVIEW: f32 = 80.0;

impl super::FileMan {
    pub(super) fn file_grid(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let c = colors();
        let cut: &[std::path::PathBuf] = match &self.clipboard {
            Some((Op::Move, paths)) => paths,
            _ => &[],
        };
        let tab = &mut self.tabs[self.active];
        let rename = &mut self.rename;
        let thumbs = &mut self.thumbs;
        let mut ev = ItemEvents::default();

        if interact::error_state(ui, tab) {
            return;
        }
        ui.add_space(6.0);
        interact::folder_backdrop(ui, tab, actions);
        if interact::empty_state(ui, tab) {
            return;
        }

        let bin = tab.loc == Loc::Bin;
        let width = ui.available_width();
        let cols = (((width + GAP) / (TILE_W + GAP)) as usize).max(1);
        let tile_w = (width + GAP) / cols as f32 - GAP;
        self.grid_cols = cols;
        let n = tab.view.len();
        let lines = n.div_ceil(cols);

        let scroll = interact::scroll_to_cursor(tab, GRID_LINE_H, cols);
        let out = scroll.show_rows(ui, GRID_LINE_H, lines, |ui, line_range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for line in line_range {
                let (line_rect, _) =
                    ui.allocate_exact_size(Vec2::new(width, GRID_LINE_H), Sense::hover());
                for col in 0..cols {
                    let r = line * cols + col;
                    if r >= n {
                        break;
                    }
                    let e = &tab.entries[tab.view[r]];
                    let tile = Rect::from_min_size(
                        line_rect.min + Vec2::new(col as f32 * (tile_w + GAP), 0.0),
                        Vec2::new(tile_w, TILE_H),
                    );
                    let resp = ui.interact(tile, Id::new(("tile", r)), Sense::click());
                    let dim = interact::fade(e.hidden, cut.contains(&e.path));

                    // Selection and hover.
                    let selected = tab.selected.contains(&e.path);
                    if selected {
                        ui.painter()
                            .rect_filled(tile, CornerRadius::same(10), c.row_selected);
                    } else if resp.hovered() {
                        ui.painter()
                            .rect_filled(tile, CornerRadius::same(10), c.control);
                    }

                    // Preview: a folder glyph, the shell's preview, or a large type tile.
                    let preview = Rect::from_center_size(
                        egui::pos2(tile.center().x, tile.top() + 10.0 + PREVIEW / 2.0),
                        Vec2::splat(PREVIEW),
                    );
                    let thumb = (!e.is_dir && thumbs::wants_thumb(&e.name))
                        .then(|| thumbs.get(&e.path, true).map(|t| (t.id(), t.size_vec2())))
                        .flatten();
                    if e.is_dir {
                        let icon = Rect::from_center_size(preview.center(), Vec2::new(62.0, 46.0));
                        widgets::folder_icon(ui.painter(), icon, c.accent_text, &dim);
                    } else if let Some((id, size)) = thumb {
                        let program = thumbs::is_program(&e.name);
                        // Program icons are small and unframed; photos are fitted into a wider
                        // box and sit on its bottom edge so every name hangs at the same distance.
                        let max = if program {
                            Vec2::splat(56.0)
                        } else {
                            Vec2::new(80.0, 60.0)
                        };
                        let size = size * (max.x / size.x).min(max.y / size.y);
                        let at = if program {
                            Rect::from_center_size(preview.center(), size)
                        } else {
                            Rect::from_min_size(
                                egui::pos2(
                                    preview.center().x - size.x / 2.0,
                                    preview.bottom() - size.y,
                                ),
                                size,
                            )
                        };
                        let radius = if program { 0 } else { 8 };
                        egui::Image::from_texture((id, size))
                            .corner_radius(CornerRadius::same(radius))
                            .tint(dim(Color32::WHITE))
                            .paint_at(ui, at);
                    } else {
                        let (g, gcol) = file_icon(e, &c);
                        let square = Rect::from_center_size(preview.center(), Vec2::splat(64.0));
                        let alpha = if c.light { 36 } else { 46 };
                        let p = ui.painter();
                        p.rect_filled(
                            square,
                            CornerRadius::same(16),
                            dim(theme::with_alpha(gcol, alpha)),
                        );
                        p.text(
                            square.center(),
                            Align2::CENTER_CENTER,
                            g,
                            icon_font(26.0),
                            dim(gcol),
                        );
                    }

                    // Name, up to two lines, or the rename box.
                    let name_rect = Rect::from_min_max(
                        egui::pos2(tile.left() + 6.0, preview.bottom() + 8.0),
                        egui::pos2(tile.right() - 6.0, tile.bottom() - 4.0),
                    );
                    match rename.as_mut().filter(|rn| rn.path == e.path) {
                        Some(rn) => {
                            let field = Rect::from_min_size(
                                name_rect.min,
                                Vec2::new(name_rect.width(), 24.0),
                            );
                            interact::rename_box(ui, field, rn, e.is_dir, &mut ev);
                        }
                        None => centered_name(ui, name_rect, &e.name, dim(c.text)),
                    }

                    ev.record(ui, &resp, r, actions);
                    interact::item_menu(ui, &resp, actions, r, bin);
                }
            }
        });
        tab.list_offset = out.state.offset.y;
        tab.list_height = out.inner_rect.height();
        ev.apply(tab);
        self.finish_rename(&ev);
    }
}

/// A name centered under its preview, wrapped to two lines and cut with an ellipsis after that.
fn centered_name(ui: &Ui, rect: Rect, name: &str, color: Color32) {
    let mut job = LayoutJob::single_section(
        name.to_owned(),
        TextFormat {
            font_id: FontId::proportional(12.0),
            color,
            ..Default::default()
        },
    );
    job.wrap = TextWrapping {
        max_width: rect.width(),
        max_rows: 2,
        // Prefer breaking between words; egui still splits a word longer than a line.
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    job.halign = Align::Center;
    let galley = ui.fonts(|f| f.layout_job(job));
    ui.painter()
        .galley(egui::pos2(rect.center().x, rect.top()), galley, color);
}
