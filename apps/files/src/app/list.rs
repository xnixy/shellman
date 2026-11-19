//! The details view, straight on the pane: a slim row of column titles (click one to sort by it,
//! again to reverse) over a hairline, and one row per item.

use super::actions::Action;
use super::interact::{self, ItemEvents};
use super::rows::list_icon;
use super::select;
use super::tab::{Loc, SortKey, Tab};
use super::text;
use super::ROW_H;
use egui::{Align2, Color32, FontId, Id, Rect, Sense, Stroke, Ui, Vec2};
use shellman_core::fsops;
use shellman_core::thumbs;
use shellman_ui::theme::{self, colors, glyph, icon_font, shape, Colors};
use shellman_ui::widgets::draw_text;

const HEADER_H: f32 = 26.0;
/// Under the header, before the first row.
const HEADER_GAP: f32 = 4.0;

/// Where each column starts and how wide it is. The name takes what the others leave; the type
/// column goes first when the window is narrow.
pub(super) struct Columns {
    pub(super) name: f32,
    pub(super) modified: f32,
    pub(super) kind: f32,
    pub(super) size: f32,
    /// The Recycle Bin's columns: the type slot holds the original location and comes first, and
    /// the date is the day it was deleted.
    bin: bool,
}

impl Columns {
    pub(super) fn fit(width: f32, bin: bool) -> Self {
        if bin {
            let (size, deleted) = (90.0, 150.0);
            let origin = if width > 560.0 { (width * 0.36).clamp(180.0, 360.0) } else { 0.0 };
            return Self {
                name: (width - size - deleted - origin).max(160.0),
                modified: deleted,
                kind: origin,
                size,
                bin,
            };
        }
        let (size, modified) = (90.0, 150.0);
        let kind = if width > 620.0 { 130.0 } else { 0.0 };
        Self {
            name: (width - size - modified - kind).max(160.0),
            modified,
            kind,
            size,
            bin,
        }
    }

    fn x_modified(&self) -> f32 {
        if self.bin {
            self.name + self.kind
        } else {
            self.name
        }
    }

    fn x_kind(&self) -> f32 {
        if self.bin {
            self.name
        } else {
            self.name + self.modified
        }
    }

    /// (sort key, label, x, width) for each column.
    fn each(&self) -> [(SortKey, &'static str, f32, f32); 4] {
        let (modified, kind) = if self.bin {
            ("Date deleted", "Original location")
        } else {
            ("Date modified", "Type")
        };
        [
            (SortKey::Name, "Name", 0.0, self.name),
            (SortKey::Modified, modified, self.x_modified(), self.modified),
            (SortKey::Kind, kind, self.x_kind(), self.kind),
            (
                SortKey::Size,
                "Size",
                self.name + self.modified + self.kind,
                self.size,
            ),
        ]
    }
}

/// A row's fill, by its state: the selection's accent wash when selected, Glasshouse's faint hover
/// when hovered. Given as they should show over the glass (see `shows_as`).
pub(super) fn row_fill(c: &Colors, selected: bool, hovered: bool) -> Option<Color32> {
    // High contrast: no washes; the selected row is outlined where it is drawn.
    if c.high_contrast {
        return None;
    }
    if selected {
        return Some(c.row_selected);
    }
    hovered.then(|| c.row_hover)
}

/// What a click on a column title does to the sort: the same column reverses it; another sorts
/// by it, names and types A to Z, dates and sizes newest and largest first.
pub(super) fn click_sort(sort: SortKey, ascending: bool, clicked: SortKey) -> (SortKey, bool) {
    if sort == clicked {
        (sort, !ascending)
    } else {
        (clicked, matches!(clicked, SortKey::Name | SortKey::Kind))
    }
}

impl super::FileMan {
    pub(super) fn file_list(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let c = colors();
        let clip = &self.clip;
        let days = text::today();
        let tab = &mut self.tabs[self.active];
        let rename = &mut self.rename;
        let thumbs = &mut self.thumbs;
        let tags = &self.tags;
        let mut ev = ItemEvents::default();

        if interact::error_state(ui, tab) {
            return;
        }
        let width = ui.available_width();
        let bin = tab.loc == Loc::Bin;
        let tagged = matches!(tab.loc, Loc::Tag(_));
        let cols = Columns::fit(width, bin);
        header(ui, tab, &cols, width);
        interact::folder_backdrop(ui, self.folder_rect, tab, actions);
        if interact::loading_state(ui, tab) || interact::empty_state(ui, tab) {
            return;
        }
        let many = tab.selected.len() > 1;
        let take_focus = self.take_focus;
        let mut has_focus = false;
        let mut tabbed_in = None;

        let scroll = interact::scroll_to_cursor(tab, ROW_H, 1);
        let out = scroll.show_rows(ui, ROW_H, tab.view.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for r in range {
                let e = &tab.entries[tab.view[r]];
                // Drags too, so a drag that starts on a row is the row's, never a rubber band.
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(width, ROW_H), interact::item_sense(tab, r));
                if !bin {
                    super::dnd::item_zone(ui, rect, e, shape::MENU_ROW);
                }
                let dim = interact::fade(e.hidden, clip.is_cut(&e.path));

                // Selection and hover.
                let selected = tab.selected.contains(&e.path);
                let row_rect = rect.shrink2(Vec2::new(0.0, 1.0));
                let p = ui.painter();
                let radius = shape::corner(shape::MENU_ROW);
                if let Some(fill) = row_fill(&c, selected, resp.hovered()) {
                    p.rect_filled(row_rect, radius, fill);
                }
                if c.high_contrast && selected {
                    // Highlight only under highlight text: the selected row is outlined.
                    p.rect_stroke(row_rect, radius, Stroke::new(2.0_f32, c.accent_fill), egui::StrokeKind::Inside);
                }
                let date = if bin {
                    e.modified_text.clone()
                } else {
                    text::friendly_date(&e.modified_text, &days.0, &days.1)
                };
                let name = text::spoken_item(e, &date, None);
                has_focus |= interact::item_access(ui, &resp, &name, selected, take_focus);
                theme::focus_ring(ui, &resp, row_rect, shape::MENU_ROW);
                if resp.gained_focus() && tab.selected.is_empty() {
                    tabbed_in = Some(r);
                }
                let p = ui.painter();
                if tab.cursor == Some(r) && (many || (!selected && !tab.selected.is_empty())) {
                    p.rect_stroke(
                        row_rect,
                        radius,
                        Stroke::new(1.0_f32, theme::with_alpha(c.accent_text, 150)),
                        egui::StrokeKind::Inside,
                    );
                }

                // Icon: the shell's preview for photos, videos and programs once it has loaded.
                let thumb = (!e.is_dir && thumbs::wants_thumb(&e.name))
                    .then(|| thumbs.get(&e.path, false).map(|t| (t.id(), t.size_vec2())))
                    .flatten();
                let center = egui::pos2(rect.left() + 20.0, rect.center().y);
                list_icon(ui, center, e, thumb, &c, &dim);

                // Name (with its tags' dots after it), or the rename box. A tag place says
                // where each item is, after its name.
                let mask = tags.mask(&e.path);
                let dots = super::tag_ui::dots_width(mask);
                let name_rect = Rect::from_min_max(
                    egui::pos2(rect.left() + 40.0, rect.top()),
                    egui::pos2(rect.left() + cols.name - 8.0 - dots, rect.bottom()),
                );
                match rename.as_mut().filter(|rn| rn.path == e.path) {
                    Some(rn) => interact::rename_box(
                        ui,
                        name_rect.shrink2(Vec2::new(0.0, 3.0)),
                        rn,
                        e.is_dir,
                        &mut ev,
                    ),
                    None => {
                        let font = FontId::proportional(13.5);
                        let room = name_rect.width();
                        let shown = text::middle_ellipsis(&e.name, |s| {
                            ui.fonts(|f| f.layout_no_wrap(s.to_owned(), font.clone(), c.text))
                                .size()
                                .x
                                <= room
                        });
                        draw_text(ui, name_rect, &shown, font.clone(), dim(c.text), false);
                        let used = ui
                            .fonts(|f| f.layout_no_wrap(shown, font, c.text))
                            .size()
                            .x
                            .min(room);
                        super::tag_ui::paint_dots(
                            ui.painter(),
                            egui::pos2(name_rect.left() + used + 8.0, rect.center().y),
                            mask,
                            false,
                        );
                    }
                }

                // The other columns.
                let col = |x: f32, w: f32| {
                    Rect::from_min_size(
                        egui::pos2(rect.left() + x + 10.0, rect.top()),
                        Vec2::new(w - 20.0, ROW_H),
                    )
                };
                let small = FontId::proportional(12.0);
                draw_text(
                    ui,
                    col(cols.x_modified(), cols.modified),
                    &date,
                    small.clone(),
                    dim(c.muted),
                    false,
                );
                if cols.kind > 0.0 {
                    // In a tag place the type column says where the item is instead.
                    let kind = if tagged {
                        e.path
                            .parent()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default()
                    } else {
                        e.kind.clone()
                    };
                    draw_text(
                        ui,
                        col(cols.x_kind(), cols.kind),
                        &kind,
                        small.clone(),
                        dim(c.muted),
                        false,
                    );
                }
                // Folders show no size in a listing, but in the bin they have one worth knowing.
                if !e.is_dir || (bin && e.size > 0) {
                    let size = fsops::format_size(e.size);
                    draw_text(
                        ui,
                        col(cols.name + cols.modified + cols.kind, cols.size),
                        &size,
                        small,
                        dim(c.muted),
                        true,
                    );
                }

                ev.record(ui, &resp, r, actions);
                interact::item_menu(ui, &resp, actions, r, bin);
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
            count: tab.view.len(),
            cols: 1,
            item: Vec2::new(width, ROW_H),
            step: Vec2::new(width, ROW_H),
        };
        interact::rubber_band(ui, tab, layout);
        ev.apply(tab);
        self.finish_rename(&ev);
        if let Some(r) = ev.drag_out {
            self.drag_out(r);
        }
    }
}

/// Column titles, on a slim row with a hairline under it; clicking one sorts by it, clicking it
/// again reverses.
fn header(ui: &mut Ui, tab: &mut Tab, cols: &Columns, width: f32) {
    let c = colors();
    let (_, head) = ui.allocate_space(Vec2::new(width, HEADER_H + HEADER_GAP));
    let head = Rect::from_min_size(head.min, Vec2::new(width, HEADER_H));
    ui.painter().hline(
        head.x_range().shrink(6.0),
        head.bottom() + HEADER_GAP / 2.0,
        Stroke::new(1.0_f32, c.hairline),
    );
    let tagged = matches!(tab.loc, Loc::Tag(_));
    for (key, label, x, w) in cols.each() {
        if w <= 0.0 {
            continue;
        }
        let label = if tagged && key == SortKey::Kind { "Where" } else { label };
        let r = Rect::from_min_size(head.min + Vec2::new(x, 0.0), Vec2::new(w, HEADER_H));
        let resp = ui.interact(r, Id::new(("col", label)), Sense::click());
        let spoken = match (tab.sort == key, tab.ascending) {
            (false, _) => format!("{label}, sort by"),
            (true, true) => format!("{label}, sorted ascending"),
            (true, false) => format!("{label}, sorted descending"),
        };
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &spoken));
        theme::focus_ring(ui, &resp, r.shrink(2.0), shape::MENU_ROW);
        if resp.hovered() {
            ui.painter()
                .rect_filled(r.shrink2(Vec2::new(2.0, 2.0)), shape::MENU_ROW, row_fill(&c, false, true).unwrap_or_default());
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
            (tab.sort, tab.ascending) = click_sort(tab.sort, tab.ascending, key);
            tab.dirty = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicking_a_title_sorts_and_clicking_again_reverses() {
        assert_eq!(click_sort(SortKey::Name, true, SortKey::Name), (SortKey::Name, false));
        assert_eq!(click_sort(SortKey::Name, true, SortKey::Size), (SortKey::Size, false));
        assert_eq!(click_sort(SortKey::Size, false, SortKey::Kind), (SortKey::Kind, true));
        assert_eq!(
            click_sort(SortKey::Kind, true, SortKey::Modified),
            (SortKey::Modified, false)
        );
    }

    #[test]
    fn selected_rows_are_the_selection_wash_converted_for_glass() {
        let c = Colors::new(false, Color32::from_rgb(59, 130, 246));
        let sel = row_fill(&c, true, false).unwrap();
        // The theme's selection, converted: brighter than the raw premultiplied colour would be.
        assert_eq!(sel, c.row_selected);
        assert!(sel.a() > 0);
        assert!(row_fill(&c, false, false).is_none());
        assert_eq!(row_fill(&c, false, true), Some(c.row_hover));
        // High contrast: no wash at all (the selected row is outlined where it is drawn).
        let hc = Colors::high_contrast();
        assert!(row_fill(&hc, true, true).is_none() && row_fill(&hc, false, true).is_none());
    }

    #[test]
    fn columns_share_the_width() {
        let wide = Columns::fit(900.0, false);
        assert_eq!(wide.name + wide.modified + wide.kind + wide.size, 900.0);
        let narrow = Columns::fit(500.0, false);
        assert_eq!(narrow.kind, 0.0, "the type column goes first");
        assert!(narrow.name >= 160.0);
    }
}
