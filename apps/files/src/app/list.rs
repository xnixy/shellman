//! The details view: a sortable header and one row per item.

use super::actions::Action;
use super::interact::{self, ItemEvents};
use super::rows::list_icon;
use super::tab::{Loc, SortKey, Tab};
use super::ROW_H;
use egui::{Align2, CornerRadius, FontId, Id, Rect, Sense, Stroke, Ui, Vec2};
use shellman_core::fsops::{self, Op};
use shellman_core::thumbs;
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::draw_text;

const HEADER_H: f32 = 28.0;

/// Where each column starts and how wide it is. The name takes what the others leave; the type
/// column goes first when the window is narrow.
struct Columns {
    name: f32,
    modified: f32,
    kind: f32,
    size: f32,
}

impl Columns {
    fn fit(width: f32) -> Self {
        let (size, modified) = (90.0, 140.0);
        let kind = if width > 620.0 { 130.0 } else { 0.0 };
        Self {
            name: (width - size - modified - kind).max(160.0),
            modified,
            kind,
            size,
        }
    }

    /// (sort key, label, x, width) for each column.
    fn each(&self) -> [(SortKey, &'static str, f32, f32); 4] {
        [
            (SortKey::Name, "Name", 0.0, self.name),
            (SortKey::Modified, "Date modified", self.name, self.modified),
            (SortKey::Kind, "Type", self.name + self.modified, self.kind),
            (
                SortKey::Size,
                "Size",
                self.name + self.modified + self.kind,
                self.size,
            ),
        ]
    }
}

impl super::FileMan {
    pub(super) fn file_list(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let c = colors();
        let has_clip = self.clipboard.is_some();
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
        let width = ui.available_width();
        let cols = Columns::fit(width);
        header(ui, tab, &cols, width);
        interact::folder_backdrop(ui, tab, has_clip, actions);
        if interact::empty_state(ui, tab) {
            return;
        }

        let scroll = interact::scroll_to_cursor(tab, ROW_H, 1);
        let out = scroll.show_rows(ui, ROW_H, tab.view.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for r in range {
                let e = &tab.entries[tab.view[r]];
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::click());
                let dim = interact::fade(e.hidden, cut.contains(&e.path));

                // Selection and hover.
                let selected = tab.selected.contains(&e.path);
                let row_rect = rect.shrink2(Vec2::new(2.0, 1.0));
                let p = ui.painter();
                if selected {
                    p.rect_filled(row_rect, CornerRadius::same(9), c.row_selected);
                    if tab.cursor == Some(r) && tab.selected.len() > 1 {
                        p.rect_stroke(
                            row_rect,
                            CornerRadius::same(9),
                            Stroke::new(1.0_f32, theme::with_alpha(c.accent, 150)),
                            egui::StrokeKind::Inside,
                        );
                    }
                } else if resp.hovered() {
                    p.rect_filled(row_rect, CornerRadius::same(9), c.control);
                }

                // Icon: the shell's preview for photos, videos and programs once it has loaded.
                let thumb = (!e.is_dir && thumbs::wants_thumb(&e.name))
                    .then(|| thumbs.get(&e.path, false).map(|t| (t.id(), t.size_vec2())))
                    .flatten();
                let center = egui::pos2(rect.left() + 20.0, rect.center().y);
                list_icon(ui, center, e, thumb, &c, &dim);

                // Name, or the rename box.
                let name_rect = Rect::from_min_max(
                    egui::pos2(rect.left() + 40.0, rect.top()),
                    egui::pos2(rect.left() + cols.name - 8.0, rect.bottom()),
                );
                match rename.as_mut().filter(|rn| rn.path == e.path) {
                    Some(rn) => interact::rename_box(
                        ui,
                        name_rect.shrink2(Vec2::new(0.0, 2.0)),
                        rn,
                        e.is_dir,
                        &mut ev,
                    ),
                    None => draw_text(
                        ui,
                        name_rect,
                        &e.name,
                        FontId::proportional(13.5),
                        dim(c.text),
                        false,
                    ),
                }

                // The other columns.
                let col = |x: f32, w: f32| {
                    Rect::from_min_size(
                        egui::pos2(rect.left() + x + 10.0, rect.top()),
                        Vec2::new(w - 20.0, ROW_H),
                    )
                };
                let small = FontId::proportional(12.0);
                let x_kind = cols.name + cols.modified;
                draw_text(
                    ui,
                    col(cols.name, cols.modified),
                    &e.modified_text,
                    small.clone(),
                    dim(c.muted),
                    false,
                );
                if cols.kind > 0.0 {
                    draw_text(
                        ui,
                        col(x_kind, cols.kind),
                        &e.kind,
                        small.clone(),
                        dim(c.muted),
                        false,
                    );
                }
                if !e.is_dir {
                    let size = fsops::format_size(e.size);
                    draw_text(
                        ui,
                        col(x_kind + cols.kind, cols.size),
                        &size,
                        small,
                        dim(c.muted),
                        true,
                    );
                }

                ev.record(ui, &resp, r, actions);
                let (is_dir, loc) = (e.is_dir, Loc::Dir(e.path.clone()));
                interact::item_menu(ui, &resp, actions, is_dir, &loc);
            }
        });
        tab.list_offset = out.state.offset.y;
        tab.list_height = out.inner_rect.height();
        ev.apply(tab);
        self.finish_rename(&ev);
    }
}

/// Column titles; clicking one sorts by it, clicking it again reverses.
fn header(ui: &mut Ui, tab: &mut Tab, cols: &Columns, width: f32) {
    let c = colors();
    let (head, _) = ui.allocate_exact_size(Vec2::new(width, HEADER_H), Sense::hover());
    for (key, label, x, w) in cols.each() {
        if w <= 0.0 {
            continue;
        }
        let r = Rect::from_min_size(head.min + Vec2::new(x, 0.0), Vec2::new(w, HEADER_H));
        let resp = ui.interact(r, Id::new(("col", label)), Sense::click());
        if resp.hovered() {
            ui.painter()
                .rect_filled(r.shrink(2.0), CornerRadius::same(6), c.row_hover);
        }
        let right = key == SortKey::Size;
        let sorted = tab.sort == key;
        let color = if sorted { c.text } else { c.muted };
        let font = FontId::proportional(11.5);
        draw_text(
            ui,
            r.shrink2(Vec2::new(10.0, 0.0)),
            label,
            font.clone(),
            color,
            right,
        );
        if sorted {
            let g = if tab.ascending {
                glyph::SORT_UP
            } else {
                glyph::SORT_DOWN
            };
            // Hug the label: after it, or before it for the right-aligned Size column.
            let w = ui
                .fonts(|f| f.layout_no_wrap(label.to_owned(), font, color))
                .size()
                .x;
            let (pos, align) = if right {
                (
                    egui::pos2(r.right() - 10.0 - w - 6.0, r.center().y),
                    Align2::RIGHT_CENTER,
                )
            } else {
                (
                    egui::pos2(r.left() + 10.0 + w + 6.0, r.center().y),
                    Align2::LEFT_CENTER,
                )
            };
            ui.painter().text(pos, align, g, icon_font(8.0), c.muted);
        }
        if resp.clicked() {
            if tab.sort == key {
                tab.ascending = !tab.ascending;
            } else {
                tab.sort = key;
                tab.ascending = key == SortKey::Name || key == SortKey::Kind;
            }
            tab.dirty = true;
        }
    }
}
