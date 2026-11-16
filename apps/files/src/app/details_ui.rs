//! The details island on the right (shown with the path bar's details button): a large preview
//! of the selection, its name and what it is, its tags, the facts (dates, where it is, its size),
//! and Open. For several items, how many and how big they are together. What takes reading is
//! read by `details`' worker; this only draws what has arrived.

use super::actions::Action;
use super::details::Total;
use super::rows::file_icon;
use super::tab::Loc;
use super::text;
use crate::tags::COUNT;
use egui::{Align2, Color32, CornerRadius, FontId, Id, Rect, Sense, Ui, UiBuilder, Vec2};
use shellman_core::fsops::{self, Entry};
use shellman_core::thumbs;
use shellman_ui::chips::{chip, chip_width, Chip, CHIP_H};
use shellman_ui::glass::islands::shows_as;
use shellman_ui::theme::{self, colors, glyph, icon_font, semibold_font};
use shellman_ui::widgets::{self, draw_text};
use std::path::PathBuf;

/// Inside the island.
const PAD: f32 = 18.0;
/// The facts' label column.
const LABEL_W: f32 = 84.0;
const FACT_H: f32 = 24.0;

/// "12 files, 3 folders" and their size, for a count (with "so far" while it runs).
pub(super) fn total_text(t: &Total) -> (String, String) {
    let mut parts = Vec::new();
    if t.files > 0 || t.folders == 0 {
        parts.push(text::count(t.files as usize, "file"));
    }
    if t.folders > 0 {
        parts.push(text::count(t.folders as usize, "folder"));
    }
    let mut size = fsops::format_size(t.bytes);
    if t.partial {
        size += " so far…";
    }
    (parts.join(", "), size)
}

/// The line under the name: "PNG image · 3840 × 2160 · 4.2 MB", or for a folder its type. A
/// cloud item whose content is not on this disk says so (its pixels and lines were not read).
pub(super) fn subline(e: &Entry, pixels: Option<(u32, u32)>, online_only: bool) -> String {
    let mut parts = vec![if e.is_dir {
        "Folder".to_owned()
    } else {
        e.kind.replace(" file", "")
    }];
    if let Some((w, h)) = pixels {
        parts.push(format!("{w} × {h}"));
    }
    if !e.is_dir {
        parts.push(fsops::format_size(e.size));
    }
    if online_only {
        parts.push("Available online only".to_owned());
    }
    parts.join(" · ")
}

impl super::FileMan {
    /// Whether the details island shows: the setting, or in a picker only when asked for there.
    pub(super) fn details_on(&self) -> bool {
        match &self.pick {
            Some(p) => p.details,
            None => self.settings.details,
        }
    }

    pub(super) fn toggle_details(&mut self) {
        // A narrow window shows one pane at a time: the button goes between the folder and the
        // details (turning them on if they were off) rather than squeezing both in.
        if self.narrow {
            if self.details_on() && self.details_alone {
                self.details_alone = false;
                return;
            }
            self.details_alone = true;
            if self.details_on() {
                return;
            }
        }
        match &mut self.pick {
            Some(p) => p.details = !p.details,
            None => {
                self.settings.details = !self.settings.details;
                self.save_pending = true;
            }
        }
        if !self.details_on() {
            self.details.stop();
        }
    }

    /// Draw the island's contents in `rect`; `back`: they show in the folder's place (a narrow
    /// window), with a back arrow to the folder.
    pub(super) fn details_island(&mut self, ui: &mut Ui, rect: Rect, back: bool, actions: &mut Vec<Action>) {
        let mut rect = rect;
        if back {
            let b = Rect::from_min_size(rect.min + Vec2::new(12.0, 10.0), Vec2::splat(30.0));
            let resp = shellman_ui::chips::round_icon(ui, b, Id::new("details-back"), glyph::BACK, true, false, "Back to the folder");
            if resp.clicked() {
                self.details_alone = false;
            }
            ui.painter().text(
                egui::pos2(b.right() + 8.0, b.center().y),
                Align2::LEFT_CENTER,
                "Details",
                semibold_font(14.0),
                colors().text,
            );
            rect.min.y = b.bottom() - 6.0;
        }
        // The whole island read as one piece; its buttons are named on their own.
        let resp = ui.interact(rect, Id::new("details-island"), Sense::hover());
        let sel = self.details_contents(ui, rect, actions);
        let spoken = self.details_spoken(&sel);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &spoken));
        self.details_sel.1 = sel;
    }

    /// The details as a screen reader says them: "Details. dock.png, PNG · 4.2 MB".
    fn details_spoken(&self, sel: &[Entry]) -> String {
        let reading = &self.details.reading;
        match sel {
            [] => format!("Details. {}", self.tabs[self.active].loc.title()),
            [e] => format!("Details. {}, {}", e.name, subline(e, reading.pixels, reading.online_only)),
            many => format!("Details. {} selected", many.len()),
        }
    }

    /// The island's contents; hands back the selection it was given, to keep for next frame.
    fn details_contents(&mut self, ui: &mut Ui, rect: Rect, actions: &mut Vec<Action>) -> Vec<Entry> {
        let c = colors();
        let tab = &self.tabs[self.active];
        // The selection, worked out again only when its fingerprint changes: Ctrl+A in a huge
        // folder must not cost a copy of every entry each frame.
        let sig = (tab.id, tab.gen, tab.selected.len(), tab.cursor, tab.anchor);
        if self.details_sel.0 != Some(sig) {
            let mut sel: Vec<Entry> = tab
                .selected
                .iter()
                .filter_map(|p| tab.row_of(p).map(|r| tab.entry(r).clone()))
                .collect();
            sel.sort_by(|a, b| a.path.cmp(&b.path));
            self.details_sel = (Some(sig), sel);
        }
        let tab = &self.tabs[self.active];
        let sel = std::mem::take(&mut self.details_sel.1);
        let loc = tab.loc.clone();
        let view_len = tab.view.len();
        let items: Vec<(PathBuf, bool)> = sel.iter().map(|e| (e.path.clone(), e.is_dir)).collect();
        self.details.want(items, &self.tx, &self.ctx);
        let reading = self.details.reading.clone();

        let inner = rect.shrink(PAD);
        let mut ui = ui.new_child(UiBuilder::new().id_salt("details").max_rect(inner));
        ui.set_clip_rect(rect);
        let ui = &mut ui;
        // The preview gives way on a short window, so the facts keep their room.
        let preview_h = (inner.width() * 0.6).min(inner.height() * 0.34).max(60.0).round();
        let preview = Rect::from_min_size(inner.min, Vec2::new(inner.width(), preview_h));
        let mut y = preview.bottom() + 16.0;

        // The preview.
        let well = CornerRadius::same(14);
        let soft = if c.high_contrast {
            c.bg
        } else if c.light {
            Color32::from_black_alpha(10)
        } else {
            shows_as(Color32::from_rgba_unmultiplied(255, 255, 255, 10))
        };

        match sel.as_slice() {
            [] => {
                ui.painter().rect_filled(preview, well, soft);
                let g = match loc {
                    Loc::Pc => glyph::PC,
                    Loc::Bin => glyph::DELETE,
                    _ => glyph::FOLDER,
                };
                if matches!(loc, Loc::Dir(_)) {
                    // A third of the well's width, but never taller than the well.
                    let w = (preview.width() * 0.32).min(preview.height() * 0.7 / 0.74);
                    widgets::folder_icon(ui.painter(), Rect::from_center_size(preview.center(), Vec2::new(w, w * 0.74)), c.accent_text, |x| x);
                } else {
                    ui.painter().text(preview.center(), Align2::CENTER_CENTER, g, icon_font(42.0), c.muted);
                }
            }
            [e] => self.details_preview(ui, preview, e, reading.lines.as_deref()),
            many => {
                ui.painter().rect_filled(preview, well, soft);
                ui.painter().text(
                    preview.center() - Vec2::new(0.0, 10.0),
                    Align2::CENTER_CENTER,
                    many.len().to_string(),
                    semibold_font(40.0),
                    c.text,
                );
                ui.painter().text(
                    preview.center() + Vec2::new(0.0, 26.0),
                    Align2::CENTER_CENTER,
                    "selected",
                    FontId::proportional(12.5),
                    c.muted,
                );
            }
        }

        if c.high_contrast {
            ui.painter().rect_stroke(preview, well, egui::Stroke::new(1.0_f32, c.text), egui::StrokeKind::Inside);
        }

        // The name and what it is.
        let (name, sub) = match sel.as_slice() {
            [] => (loc.title(), text::count(view_len, "item")),
            [e] => (e.name.clone(), subline(e, reading.pixels, reading.online_only)),
            many => {
                let files: u64 = many.iter().filter(|e| !e.is_dir).map(|e| e.size).sum();
                let quick = text::count(many.len(), "item");
                let sub = match &reading.total {
                    Some(t) => total_text(t).1,
                    None if many.iter().any(|e| e.is_dir) => "Counting…".to_owned(),
                    None => fsops::format_size(files),
                };
                (quick, sub)
            }
        };
        let font = semibold_font(16.5);
        let room = inner.width();
        let shown = text::middle_ellipsis(&name, |s| {
            ui.fonts(|f| f.layout_no_wrap(s.to_owned(), font.clone(), c.text)).size().x <= room
        });
        draw_text(ui, Rect::from_min_size(egui::pos2(inner.left(), y), Vec2::new(room, 22.0)), &shown, font, c.text, false);
        y += 24.0;
        draw_text(ui, Rect::from_min_size(egui::pos2(inner.left(), y), Vec2::new(room, 18.0)), &sub, FontId::proportional(12.5), c.muted, false);
        y += 30.0;

        // Tags: the item's, as chips, and "+ tag" to change them.
        if !sel.is_empty() && !matches!(loc, Loc::Bin) {
            y = self.details_tags(ui, &sel, inner, y);
        }

        // The facts.
        let mut facts: Vec<(&str, String, bool)> = Vec::new();
        let (today, yesterday) = text::today();
        match sel.as_slice() {
            [e] => {
                facts.push(("Modified", text::friendly_date(&e.modified_text, &today, &yesterday), false));
                if let Some(cr) = &reading.created {
                    facts.push(("Created", text::friendly_date(cr, &today, &yesterday), false));
                }
                if let Some(parent) = e.path.parent() {
                    facts.push(("Where", parent.display().to_string(), true));
                }
                if e.is_dir {
                    let v = match &reading.total {
                        Some(t) => {
                            let (n, size) = total_text(t);
                            format!("{n} · {size}")
                        }
                        None => "Counting…".into(),
                    };
                    facts.push(("Contains", v, false));
                }
            }
            [] => {
                if let Some(d) = loc.dir() {
                    facts.push(("Where", d.display().to_string(), true));
                }
            }
            many => {
                if let Some(t) = &reading.total {
                    let (n, size) = total_text(t);
                    facts.push(("Contains", n, false));
                    facts.push(("Size", size, false));
                }
                let parents: std::collections::HashSet<_> = many.iter().filter_map(|e| e.path.parent()).collect();
                if parents.len() == 1 {
                    if let Some(p) = parents.into_iter().next() {
                        facts.push(("Where", p.display().to_string(), true));
                    }
                }
            }
        }
        for (label, value, copy) in facts {
            let row = Rect::from_min_size(egui::pos2(inner.left(), y), Vec2::new(room, FACT_H));
            draw_text(ui, Rect::from_min_size(row.min, Vec2::new(LABEL_W, FACT_H)), label, FontId::proportional(12.5), c.muted, false);
            let value_w = room - LABEL_W - if copy { 28.0 } else { 0.0 };
            let value_rect = Rect::from_min_size(egui::pos2(row.left() + LABEL_W, row.top()), Vec2::new(value_w, FACT_H));
            draw_text(ui, value_rect, &value, FontId::proportional(12.5), c.text, false);
            if copy {
                let b = Rect::from_center_size(egui::pos2(row.right() - 12.0, row.center().y), Vec2::splat(24.0));
                let full = match sel.as_slice() {
                    [e] => e.path.display().to_string(),
                    _ => value.clone(),
                };
                if shellman_ui::chips::round_icon(ui, b, Id::new(("details-copy", label)), glyph::COPY, true, false, "Copy the path")
                    .on_hover_text("Copy the path")
                    .clicked()
                {
                    ui.ctx().copy_text(full);
                    actions.push(Action::Toast("Path copied".into()));
                }
            }
            y += FACT_H + 4.0;
        }

        // Open and Properties at the bottom.
        // At the bottom, or under the facts when the window is short; left out when even that
        // has no room (the command bar has both).
        let bottom = inner.bottom().max(y + 8.0 + 34.0);
        if !sel.is_empty() && !matches!(loc, Loc::Bin) && bottom <= inner.bottom() + 0.5 {
            let props_w = chip_width(ui, "Properties", false, Chip::Soft);
            let open = Rect::from_min_max(egui::pos2(inner.left(), bottom - 34.0), egui::pos2(inner.right() - props_w - 8.0, bottom));
            let props = Rect::from_min_max(egui::pos2(open.right() + 8.0, bottom - 34.0), egui::pos2(inner.right(), bottom));
            if chip(ui, open, Id::new("details-open"), "Open", Some(glyph::OPEN_EXTERNAL), Chip::Accent).clicked() {
                actions.push(Action::OpenSelected);
            }
            if chip(ui, props, Id::new("details-props"), "Properties", None, Chip::Soft).clicked() {
                actions.push(Action::Properties);
            }
        }
        sel
    }

    /// One item's preview: its picture, its first lines, a folder, or its type's glyph.
    fn details_preview(&mut self, ui: &Ui, r: Rect, e: &Entry, lines: Option<&[String]>) {
        let c = colors();
        let well = CornerRadius::same(14);
        let p = ui.painter();
        if e.is_dir {
            p.rect_filled(r, well, shows_as(Color32::from_rgba_unmultiplied(255, 255, 255, if c.light { 120 } else { 10 })));
            let w = r.width() * 0.34;
            widgets::folder_icon(p, Rect::from_center_size(r.center(), Vec2::new(w, w * 0.74)), c.accent_text, |x| x);
            return;
        }
        let thumb = thumbs::wants_thumb(&e.name)
            .then(|| self.thumbs.get(&e.path, true).map(|t| (t.id(), t.size_vec2())))
            .flatten();
        if let (Some((id, size)), false) = (thumb, thumbs::is_program(&e.name)) {
            // The whole picture, fitted, on a faint well.
            p.rect_filled(r, well, shows_as(Color32::from_rgba_unmultiplied(255, 255, 255, 8)));
            let fit = (r.width() / size.x).min(r.height() / size.y);
            let at = Rect::from_center_size(r.center(), size * fit);
            egui::Image::from_texture((id, size)).corner_radius(well).paint_at(ui, at);
            return;
        }
        if let Some(lines) = lines {
            // A page: light, with the first lines in small monospace, as the design's documents.
            let page = Color32::from_rgb(244, 245, 249);
            p.rect_filled(r, well, page);
            let font = FontId::monospace(10.5);
            let mut y = r.top() + 12.0;
            for line in lines {
                if y + 13.0 > r.bottom() - 8.0 {
                    break;
                }
                draw_text(ui, Rect::from_min_size(egui::pos2(r.left() + 14.0, y), Vec2::new(r.width() - 28.0, 13.0)), line, font.clone(), Color32::from_rgb(70, 78, 96), false);
                y += 13.5;
            }
            return;
        }
        let (g, col) = file_icon(e, &c);
        p.rect_filled(r, well, shows_as(theme::with_alpha(col, 40)));
        if let Some((id, size)) = thumb {
            let max = 64.0_f32.min(r.height() * 0.5);
            let fitted = size * (max / size.x).min(max / size.y);
            egui::Image::from_texture((id, fitted)).paint_at(ui, Rect::from_center_size(r.center(), fitted));
        } else {
            p.text(r.center(), Align2::CENTER_CENTER, g, icon_font(44.0), col);
        }
    }

    /// The tags of the selection as chips, then "+ tag", which opens the list of tags (to put
    /// on or take off, and to rename). Returns where the next part starts.
    fn details_tags(&mut self, ui: &mut Ui, sel: &[Entry], inner: Rect, y: f32) -> f32 {
        let on: Vec<usize> = (0..COUNT)
            .filter(|&i| sel.iter().all(|e| self.tags.mask(&e.path) & (1 << i) != 0))
            .collect();
        let mut x = inner.left();
        let mut y = y;
        for &i in &on {
            let name = self.tags.store.name(i).to_owned();
            let w = chip_width(ui, &name, true, Chip::Soft);
            if x + w > inner.right() {
                x = inner.left();
                y += CHIP_H + 6.0;
            }
            let r = Rect::from_min_size(egui::pos2(x, y), Vec2::new(w, CHIP_H));
            chip(ui, r, Id::new(("details-tag", i)), &name, Some(' '), Chip::Soft);
            ui.painter().circle_filled(egui::pos2(r.left() + 19.0, r.center().y), 4.5, super::tag_ui::colour(i));
            x += w + 6.0;
        }
        let add = "+ tag";
        let w = chip_width(ui, add, false, Chip::Soft);
        if x + w > inner.right() {
            x = inner.left();
            y += CHIP_H + 6.0;
        }
        let r = Rect::from_min_size(egui::pos2(x, y), Vec2::new(w, CHIP_H));
        let resp = ui.scope(|ui| {
            ui.multiply_opacity(0.75);
            chip(ui, r, Id::new("details-add-tag"), add, None, Chip::Soft)
        }).inner;
        let popup = Id::new("details-tag-menu");
        if resp.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup));
        }
        let paths: Vec<PathBuf> = sel.iter().map(|e| e.path.clone()).collect();
        egui::popup::popup_below_widget(ui, popup, &resp, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            self.tag_menu(ui, &paths);
        });
        y + CHIP_H + 18.0
    }

    /// The list of tags for `paths`: click one to put it on all of them or take it off; "Rename
    /// tags" turns the names into fields.
    pub(super) fn tag_menu(&mut self, ui: &mut Ui, paths: &[PathBuf]) {
        let c = colors();
        ui.set_min_width(220.0);
        shellman_ui::menu::flat_popup(ui);
        let mut changed = false;
        for i in 0..COUNT {
            let on = paths.iter().all(|p| self.tags.mask(p) & (1 << i) != 0);
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width().max(220.0), 32.0), Sense::click());
            let tag_name = self.tags.store.name(i).to_owned();
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, &tag_name));
            theme::focus_ring(ui, &resp, rect, 6.0);
            if resp.hovered() && !self.tag_edit {
                ui.painter().rect_filled(rect, CornerRadius::same(6), theme::with_alpha(c.text, if c.light { 18 } else { 12 }));
            }
            ui.painter().circle_filled(egui::pos2(rect.left() + 19.0, rect.center().y), 5.5, super::tag_ui::colour(i));
            let text_rect = Rect::from_min_max(egui::pos2(rect.left() + 38.0, rect.top() + 3.0), egui::pos2(rect.right() - 34.0, rect.bottom() - 3.0));
            if self.tag_edit {
                let mut name = self.tags.store.name(i).to_owned();
                let te = ui.put(text_rect, egui::TextEdit::singleline(&mut name).margin(Vec2::new(6.0, 2.0)));
                shellman_ui::widgets::name_field(&te, &format!("Name of tag {}", i + 1), Some(&name));
                if te.changed() && self.tags.store.rename(i, &name) {
                    changed = true;
                }
            } else {
                draw_text(ui, text_rect, self.tags.store.name(i), FontId::proportional(14.0), c.text, false);
                if resp.clicked() {
                    for p in paths {
                        changed |= self.tags.store.set(p, i, !on);
                    }
                }
            }
            if on {
                ui.painter().text(egui::pos2(rect.right() - 16.0, rect.center().y), Align2::CENTER_CENTER, glyph::CHECK, icon_font(12.0), c.text);
            }
        }
        shellman_ui::menu::flat_separator(ui);
        let label = if self.tag_edit { "Done renaming" } else { "Rename tags…" };
        if shellman_ui::menu::flat_item(ui, shellman_ui::menu::Icon::Glyph(glyph::RENAME), label, "", false, true) {
            self.tag_edit = !self.tag_edit;
        }
        if changed {
            self.tags_changed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_read_naturally() {
        let t = Total { files: 12, folders: 3, bytes: 2048, partial: false };
        assert_eq!(total_text(&t), ("12 files, 3 folders".into(), fsops::format_size(2048)));
        let empty = Total::default();
        assert_eq!(total_text(&empty).0, "0 files");
        let running = Total { files: 1, folders: 0, bytes: 10, partial: true };
        assert!(total_text(&running).1.ends_with("so far…"));
    }

    #[test]
    fn the_line_under_the_name_says_type_size_and_pixels() {
        let e = Entry {
            name: "dock.png".into(),
            key: "dock.png".into(),
            path: PathBuf::from(r"C:\dock.png"),
            is_dir: false,
            size: 4_400_000,
            modified: None,
            modified_text: String::new(),
            kind: "PNG file".into(),
            hidden: false,
        };
        let s = subline(&e, Some((3840, 2160)), false);
        assert!(s.starts_with("PNG · 3840 × 2160 · "), "{s}");
        let dir = Entry { is_dir: true, kind: "Folder".into(), ..e };
        assert_eq!(subline(&dir, None, false), "Folder");
        assert_eq!(subline(&dir, None, true), "Folder · Available online only");
    }
}
