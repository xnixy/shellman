//! The file list for a folder: header, rows, selection, and renaming.

use super::actions::Action;
use super::rows::{file_icon, row_menu};
use super::tab::{Loc, SortKey};
use super::widgets::{draw_text, menu_item, select_range};
use super::ROW_H;
use crate::fsops::{self, Op};
use crate::theme::{self, colors, glyph, icon_font};
use crate::thumbs;
use egui::{
    Align2, Color32, CornerRadius, FontId, Id, Key, Modifiers, Rect, RichText, ScrollArea, Sense,
    Stroke, Ui, Vec2,
};
use std::path::Path;

const HEADER_H: f32 = 28.0;

impl super::FileMan {
    pub(super) fn file_list(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let c = colors();
        let has_clip = self.clipboard.is_some();
        let tab = &mut self.tabs[self.active];
        let rename = &mut self.rename;
        let thumbs = &mut self.thumbs;
        let mut commit = false;
        let mut cancel = false;

        if let Some(err) = &tab.error {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Can't open this folder")
                        .size(16.0)
                        .color(c.text),
                );
                ui.label(theme::muted(err.clone()));
            });
            return;
        }

        let width = ui.available_width();
        // Columns: name takes what the others leave. Type goes first when space is short.
        let size_w = 90.0;
        let mod_w = 140.0;
        let kind_w = if width > 620.0 { 130.0 } else { 0.0 };
        let name_w = (width - size_w - mod_w - kind_w).max(160.0);
        let cols = [
            (SortKey::Name, "Name", 0.0, name_w),
            (SortKey::Modified, "Date modified", name_w, mod_w),
            (SortKey::Kind, "Type", name_w + mod_w, kind_w),
            (SortKey::Size, "Size", name_w + mod_w + kind_w, size_w),
        ];

        // Header.
        let (head, _) = ui.allocate_exact_size(Vec2::new(width, HEADER_H), Sense::hover());
        for (key, label, x, w) in cols {
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
            let text_rect = r.shrink2(Vec2::new(10.0, 0.0));
            let sorted = tab.sort == key;
            draw_text(
                ui,
                text_rect,
                label,
                FontId::proportional(12.5),
                if sorted { c.text } else { c.muted },
                right,
            );
            if sorted {
                let g = if tab.ascending {
                    glyph::SORT_UP
                } else {
                    glyph::SORT_DOWN
                };
                let pos = if right {
                    egui::pos2(r.left() + 8.0, r.center().y)
                } else {
                    egui::pos2(r.right() - 10.0, r.center().y)
                };
                let align = if right {
                    Align2::LEFT_CENTER
                } else {
                    Align2::RIGHT_CENTER
                };
                ui.painter()
                    .text(pos, align, g, icon_font(9.0), c.accent_text);
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
        ui.painter().hline(
            head.x_range(),
            head.bottom(),
            Stroke::new(1.0_f32, c.hairline),
        );

        // Empty space behind the rows: clicks clear the selection, right-click acts on the folder.
        let bg_rect = ui.available_rect_before_wrap();
        let bg = ui.interact(bg_rect, Id::new("list-bg"), Sense::click());
        if bg.clicked() {
            tab.selected.clear();
            tab.cursor = None;
        }
        bg.context_menu(|ui| {
            if menu_item(ui, glyph::NEW_FOLDER, "New folder", "Ctrl+Shift+N") {
                actions.push(Action::NewFolder);
                ui.close_menu();
            }
            if has_clip && menu_item(ui, glyph::PASTE, "Paste", "Ctrl+V") {
                actions.push(Action::Paste);
                ui.close_menu();
            }
            ui.separator();
            if menu_item(ui, glyph::REFRESH, "Refresh", "F5") {
                actions.push(Action::Refresh);
                ui.close_menu();
            }
            if menu_item(ui, glyph::COPY, "Copy folder path", "") {
                tab.selected.clear();
                actions.push(Action::CopyPath);
                ui.close_menu();
            }
            if menu_item(ui, glyph::OPEN_EXTERNAL, "Open in Explorer", "") {
                actions.push(Action::ExplorerHere);
                ui.close_menu();
            }
        });

        if tab.view.is_empty() && !tab.loading {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                let msg = if tab.filter.is_empty() {
                    "This folder is empty"
                } else {
                    "Nothing matches"
                };
                ui.label(theme::muted(msg));
            });
            return;
        }

        let mut scroll = ScrollArea::vertical().auto_shrink([false, false]);
        if std::mem::take(&mut tab.scroll_to_cursor) {
            if let Some(cur) = tab.cursor {
                let y = cur as f32 * ROW_H;
                let mut off = tab.list_offset;
                if y < off {
                    off = y;
                } else if y + ROW_H > off + tab.list_height {
                    off = y + ROW_H - tab.list_height;
                }
                scroll = scroll.vertical_scroll_offset(off.max(0.0));
            }
        }
        let n = tab.view.len();
        let mut clicked: Option<(usize, Modifiers)> = None;
        let mut secondary: Option<usize> = None;
        let out = scroll.show_rows(ui, ROW_H, n, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for r in range {
                let e = &tab.entries[tab.view[r]];
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::click());
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
                let dim = |col: Color32| {
                    if e.hidden {
                        col.gamma_multiply(0.55)
                    } else {
                        col
                    }
                };
                let cut = matches!(&self.clipboard, Some((Op::Move, ps)) if ps.contains(&e.path));
                let dim = |col: Color32| {
                    if cut {
                        dim(col).gamma_multiply(0.5)
                    } else {
                        dim(col)
                    }
                };
                // A soft tile in the file type's color, with its glyph on it.
                // Photos, videos and programs show the shell's own preview or icon; everything
                // else, or anything still loading, gets a soft tile in its type's color.
                let tile = Rect::from_center_size(
                    egui::pos2(rect.left() + 22.0, rect.center().y),
                    Vec2::splat(24.0),
                );
                let thumb = if !e.is_dir && thumbs::wants_thumb(&e.name) {
                    thumbs.get(&e.path).map(|t| (t.id(), t.size_vec2()))
                } else {
                    None
                };
                if let Some((id, size)) = thumb {
                    // Fit inside the tile, keeping the aspect ratio.
                    let scale = (tile.width() / size.x).min(tile.height() / size.y);
                    let fit = Rect::from_center_size(tile.center(), size * scale);
                    egui::Image::from_texture((id, size))
                        .corner_radius(CornerRadius::same(5))
                        .tint(dim(Color32::WHITE))
                        .paint_at(ui, fit);
                } else {
                    let (g, gcol) = file_icon(e, &c);
                    let p = ui.painter();
                    p.rect_filled(
                        tile,
                        CornerRadius::same(7),
                        dim(theme::with_alpha(gcol, if c.light { 36 } else { 46 })),
                    );
                    p.text(
                        tile.center(),
                        Align2::CENTER_CENTER,
                        g,
                        icon_font(12.5),
                        dim(gcol),
                    );
                }
                let name_rect = Rect::from_min_max(
                    egui::pos2(rect.left() + 46.0, rect.top()),
                    egui::pos2(rect.left() + name_w - 8.0, rect.bottom()),
                );
                let renaming = rename.as_ref().is_some_and(|rn| rn.path == e.path);
                if renaming {
                    let rn = rename.as_mut().unwrap();
                    let id = Id::new("rename");
                    let te = ui.put(
                        name_rect.shrink2(Vec2::new(0.0, 2.0)),
                        egui::TextEdit::singleline(&mut rn.text)
                            .id(id)
                            .margin(Vec2::new(6.0, 3.0)),
                    );
                    if std::mem::take(&mut rn.focus) {
                        te.request_focus();
                        let stem = if e.is_dir {
                            rn.text.chars().count()
                        } else {
                            Path::new(&rn.text)
                                .file_stem()
                                .map_or(0, |s| s.to_string_lossy().chars().count())
                        };
                        select_range(ui.ctx(), id, 0, stem);
                    }
                    if te.lost_focus() {
                        if ui.input(|i| i.key_pressed(Key::Escape)) {
                            cancel = true;
                        } else {
                            commit = true;
                        }
                    }
                } else {
                    draw_text(
                        ui,
                        name_rect,
                        &e.name,
                        FontId::proportional(13.5),
                        dim(c.text),
                        false,
                    );
                }
                let col = |x: f32, w: f32| {
                    Rect::from_min_size(
                        egui::pos2(rect.left() + x + 10.0, rect.top()),
                        Vec2::new(w - 20.0, ROW_H),
                    )
                };
                let small = FontId::proportional(12.5);
                draw_text(
                    ui,
                    col(name_w, mod_w),
                    &e.modified_text,
                    small.clone(),
                    dim(c.muted),
                    false,
                );
                if kind_w > 0.0 {
                    draw_text(
                        ui,
                        col(name_w + mod_w, kind_w),
                        &e.kind,
                        small.clone(),
                        dim(c.muted),
                        false,
                    );
                }
                if !e.is_dir {
                    draw_text(
                        ui,
                        col(name_w + mod_w + kind_w, size_w),
                        &fsops::format_size(e.size),
                        small,
                        dim(c.muted),
                        true,
                    );
                }

                if resp.double_clicked() {
                    actions.push(Action::OpenRow(r));
                } else if resp.clicked() {
                    clicked = Some((r, ui.input(|i| i.modifiers)));
                }
                if resp.secondary_clicked() {
                    secondary = Some(r);
                }
                let is_dir = e.is_dir;
                let loc = Loc::Dir(e.path.clone());
                resp.context_menu(|ui| row_menu(ui, actions, is_dir, &loc));
            }
        });
        tab.list_offset = out.state.offset.y;
        tab.list_height = out.inner_rect.height();

        if let Some((r, m)) = clicked {
            if m.command {
                let p = tab.entry(r).path.clone();
                if !tab.selected.remove(&p) {
                    tab.selected.insert(p);
                }
                tab.cursor = Some(r);
                tab.anchor = Some(r);
            } else if m.shift {
                tab.select_range(r);
            } else {
                tab.select_only(r);
            }
        }
        if let Some(r) = secondary {
            if !tab.selected.contains(&tab.entry(r).path) {
                tab.select_only(r);
            }
        }
        if cancel {
            self.rename = None;
        } else if commit {
            self.commit_rename();
        }
    }
}
