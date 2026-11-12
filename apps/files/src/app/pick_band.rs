//! Picker mode's pills: at the top, the title (where the tabs would be) with New folder; at the
//! bottom, in place of the selection pill, the name field, the file type, and the main button
//! (accent) with Cancel. The bottom pill turns into the question "Replace <name>?" before a
//! save over an existing file, and says so while the app checks a choice.

use super::actions::Action;
use super::tab::Loc;
use crate::pick::button;
use egui::{Align, Align2, CornerRadius, FontId, Id, Key, Rect, RichText, Stroke, Ui, Vec2};
use shellman_pick::Mode;
use shellman_ui::chips::{chip, chip_width, soft_colors, Chip, CHIP_H};
use shellman_ui::theme::{self, colors, glyph, icon_font, semibold_font};
use shellman_ui::widgets::{draw_text, menu_item};
use std::path::Path;

/// Inside the pills, at each end, and between their parts.
const LEAD: f32 = 16.0;
const PAD: f32 = 8.0;
const SPACE: f32 = 8.0;
/// The bottom pill is no wider than this.
const MAX_W: f32 = 900.0;
const FIELD_H: f32 = 30.0;
const TYPE_MAX_W: f32 = 240.0;
/// Segoe's chevron pointing down, for the type list.
const CHEVRON_DOWN: char = '\u{E70D}';

fn text_width(ui: &Ui, text: &str, font: FontId) -> f32 {
    ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE))
        .size()
        .x
        .ceil()
}

/// A chip that can be greyed out; true when clicked.
fn button_chip(ui: &mut Ui, rect: Rect, id: &str, label: &str, kind: Chip, enabled: bool) -> bool {
    ui.scope(|ui| {
        if !enabled {
            ui.disable();
            ui.multiply_opacity(0.45);
        }
        chip(ui, rect, Id::new(("pick", id)), label, None, kind).clicked() && enabled
    })
    .inner
}

impl super::FileMan {
    /// The title pill at the left of `row`; returns it.
    pub(super) fn pick_title(&mut self, ui: &mut Ui, row: Rect, actions: &mut Vec<Action>) -> Rect {
        let c = colors();
        let Some(p) = self.picker() else { return Rect::NOTHING };
        let req = &p.session.req;
        let title = button::title(req);
        // The program's name, without the dialog id some apps add.
        let client = req.client.split('|').next().unwrap_or("").trim().to_owned();
        let g = match req.mode {
            Mode::Save => glyph::FILE,
            Mode::Folder => glyph::FOLDER,
            Mode::Open | Mode::OpenMany => glyph::OPEN_EXTERNAL,
        };
        let title_w = text_width(ui, &title, semibold_font(13.5)).min(360.0);
        let client_w = if client.is_empty() {
            0.0
        } else {
            text_width(ui, &client, FontId::proportional(12.5)).min(200.0) + SPACE * 2.0
        };
        let can_add = self.tabs[self.active].loc.dir().is_some();
        let add_w = if can_add { chip_width(ui, "New folder", true, Chip::Soft) + SPACE } else { 0.0 };
        let wanted = LEAD + 22.0 + title_w + client_w + add_w + PAD + 4.0;
        let pill = super::plan::from_left(row, wanted);
        let y = pill.center().y;
        let mut x = pill.left() + LEAD;
        ui.painter().text(
            egui::pos2(x + 7.0, y),
            Align2::CENTER_CENTER,
            g,
            icon_font(13.0),
            c.accent_text,
        );
        x += 22.0;
        let room = (pill.right() - add_w - PAD - x).max(0.0);
        draw_text(
            ui,
            Rect::from_min_size(egui::pos2(x, pill.top()), Vec2::new(title_w.min(room), pill.height())),
            &title,
            semibold_font(13.5),
            c.text,
            false,
        );
        x += title_w + SPACE * 2.0;
        if !client.is_empty() {
            let w = (pill.right() - add_w - PAD - x).max(0.0);
            draw_text(
                ui,
                Rect::from_min_size(egui::pos2(x, pill.top()), Vec2::new(w, pill.height())),
                &client,
                FontId::proportional(12.5),
                c.muted,
                false,
            );
        }
        if can_add {
            let w = add_w - SPACE;
            let r = Rect::from_min_size(
                egui::pos2(pill.right() - PAD - w, y - CHIP_H / 2.0),
                Vec2::new(w, CHIP_H),
            );
            if chip(ui, r, Id::new("pick-new-folder"), "New folder", Some(glyph::NEW_FOLDER), Chip::Soft)
                .on_hover_text("New folder (Ctrl+Shift+N)")
                .clicked()
            {
                actions.push(Action::NewFolder);
            }
        }
        pill
    }

    /// The bottom pill in `band`; returns it.
    pub(super) fn pick_band(&mut self, ui: &mut Ui, band: Rect) -> Option<Rect> {
        let pill = super::plan::centred(band, MAX_W);
        let p = self.picker()?;
        if p.waiting.is_some() {
            self.pick_waiting(ui, pill);
        } else if let Some(path) = p.replace.clone() {
            self.pick_replace(ui, pill, &path);
        } else {
            self.pick_fields(ui, pill);
        }
        Some(pill)
    }

    /// The name field, the type list, and the buttons.
    fn pick_fields(&mut self, ui: &mut Ui, pill: Rect) {
        let c = colors();
        let selected = self.pick_selection();
        let in_folder = self.tabs[self.active].loc.dir().is_some();
        let p = self.pick.as_ref().unwrap();
        let req = &p.session.req;
        let label = button::name_label(req);
        let main = button::main_label(req, &selected, p.typed_since);
        let enabled = button::main_enabled(req.mode, &p.name, &selected, p.typed_since, in_folder);
        let cancel = "Cancel";
        let show_types = req.mode != Mode::Folder && !req.types.is_empty();
        let type_name = match &p.typed_filter {
            Some(f) => f.clone(),
            None => req
                .types
                .get(p.type_index)
                .map_or_else(String::new, |t| t.name.clone()),
        };
        let y = pill.center().y;
        let main_w = chip_width(ui, &main, false, Chip::Accent);
        let cancel_w = chip_width(ui, cancel, false, Chip::Soft);
        let type_w = if show_types {
            (chip_width(ui, &type_name, true, Chip::Soft)).min(TYPE_MAX_W)
        } else {
            0.0
        };
        let label_w = text_width(ui, &label, FontId::proportional(12.5));

        // From the right: Cancel, the main button, the type list.
        let mut right = pill.right() - PAD;
        let cancel_r = Rect::from_min_size(egui::pos2(right - cancel_w, y - CHIP_H / 2.0), Vec2::new(cancel_w, CHIP_H));
        right -= cancel_w + SPACE;
        let main_r = Rect::from_min_size(egui::pos2(right - main_w, y - CHIP_H / 2.0), Vec2::new(main_w, CHIP_H));
        right -= main_w + SPACE;
        let type_r = Rect::from_min_size(egui::pos2(right - type_w, y - CHIP_H / 2.0), Vec2::new(type_w, CHIP_H));
        if show_types {
            right -= type_w + SPACE;
        }
        let mut x = pill.left() + LEAD;
        ui.painter().text(
            egui::pos2(x, y),
            Align2::LEFT_CENTER,
            &label,
            FontId::proportional(12.5),
            c.muted,
        );
        x += label_w + SPACE + 2.0;
        let field = Rect::from_min_max(
            egui::pos2(x, y - FIELD_H / 2.0),
            egui::pos2(right.max(x + 40.0), y + FIELD_H / 2.0),
        );
        let (submit, cancelled) = self.pick_name_field(ui, field);

        if show_types {
            self.pick_type_list(ui, type_r, &type_name);
        }
        let main_clicked = button_chip(ui, main_r, "main", &main, Chip::Accent, enabled);
        let cancel_clicked = button_chip(ui, cancel_r, "cancel", cancel, Chip::Soft, true);
        if cancel_clicked || cancelled {
            self.pick_cancel();
        } else if main_clicked || (submit && enabled) {
            self.pick_submit();
        }
    }

    /// The name field. Returns (Enter pressed in it, Esc pressed in it).
    fn pick_name_field(&mut self, ui: &mut Ui, rect: Rect) -> (bool, bool) {
        let c = colors();
        let id = Id::new("pick-name");
        let focused = ui.memory(|m| m.has_focus(id));
        let hovered = ui.rect_contains_pointer(rect);
        let (soft, soft_hover, line) = soft_colors(c.light);
        let fill = if focused || hovered { soft_hover } else { soft };
        let stroke = if focused {
            Stroke::new(1.0_f32, theme::with_alpha(c.accent_fill, 190))
        } else {
            line
        };
        ui.painter().rect(rect, CornerRadius::same((FIELD_H / 2.0) as u8), fill, stroke, egui::StrokeKind::Inside);
        let p = self.pick.as_mut().unwrap();
        let hint = match p.session.req.mode {
            Mode::Save => "Name to save as",
            Mode::Folder => "Folder name or path",
            _ => "Name, path, or *.ext to filter",
        };
        let resp = ui.put(
            rect.shrink2(Vec2::new(6.0, 0.0)),
            egui::TextEdit::singleline(&mut p.name)
                .id(id)
                .frame(false)
                .text_color(c.text)
                .hint_text(RichText::new(hint).color(c.muted))
                .vertical_align(Align::Center)
                .margin(Vec2::new(8.0, 0.0))
                .desired_width(rect.width() - 12.0),
        );
        if std::mem::take(&mut p.focus_name) {
            resp.request_focus();
            // Select the name without its extension, ready to be typed over, as Windows does.
            let stem = Path::new(&p.name)
                .file_stem()
                .map_or(0, |s| s.to_string_lossy().chars().count());
            shellman_ui::widgets::select_range(ui.ctx(), id, 0, stem);
        }
        if resp.changed() {
            p.typed_since = true;
        }
        let lost = resp.lost_focus();
        let (enter, esc) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
        (lost && enter, lost && esc)
    }

    /// The chosen file type, as a chip that opens the list of types.
    fn pick_type_list(&mut self, ui: &mut Ui, rect: Rect, name: &str) {
        let c = colors();
        let resp = chip(ui, rect, Id::new("pick-type"), name, None, Chip::Soft);
        ui.painter().text(
            egui::pos2(rect.right() - 16.0, rect.center().y),
            Align2::CENTER_CENTER,
            CHEVRON_DOWN,
            icon_font(9.0),
            c.muted,
        );
        let types = self.picker().map(|p| p.session.req.types.clone()).unwrap_or_default();
        let current = self.picker().map_or(0, |p| p.type_index);
        let tip = types
            .get(current)
            .map(|t| t.patterns.join("; "))
            .unwrap_or_default();
        let resp = resp.on_hover_text(tip);
        let popup = Id::new("pick-type-menu");
        if resp.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup));
        }
        let mut chosen = None;
        egui::popup::popup_above_or_below_widget(
            ui,
            popup,
            &resp,
            egui::AboveOrBelow::Above,
            egui::PopupCloseBehavior::CloseOnClick,
            |ui| {
                ui.set_min_width(rect.width().max(220.0));
                super::controls::flat_items(ui);
                for (i, t) in types.iter().enumerate() {
                    let g = if i == current { glyph::CHECK } else { ' ' };
                    if menu_item(ui, g, &t.name, "") {
                        chosen = Some(i);
                    }
                }
            },
        );
        if let Some(i) = chosen {
            self.pick_set_type(i);
        }
    }

    /// "Replace <name>?" with Replace (red) and Back.
    fn pick_replace(&mut self, ui: &mut Ui, pill: Rect, path: &Path) {
        let c = colors();
        let name = path
            .file_name()
            .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let question = format!("Replace {name}?");
        let note = "It's already there; saving will write over it.";
        let y = pill.center().y;
        let back = "Back";
        let replace = "Replace";
        let back_w = chip_width(ui, back, false, Chip::Soft);
        let replace_w = chip_width(ui, replace, false, Chip::Danger);
        let back_r = Rect::from_min_size(egui::pos2(pill.right() - PAD - back_w, y - CHIP_H / 2.0), Vec2::new(back_w, CHIP_H));
        let replace_r = Rect::from_min_size(
            egui::pos2(back_r.left() - SPACE - replace_w, y - CHIP_H / 2.0),
            Vec2::new(replace_w, CHIP_H),
        );
        let mut x = pill.left() + LEAD;
        ui.painter().text(egui::pos2(x + 7.0, y), Align2::CENTER_CENTER, glyph::INFO, icon_font(13.0), c.warn);
        x += 24.0;
        let room = replace_r.left() - SPACE - x;
        let q_w = text_width(ui, &question, semibold_font(13.5)).min(room);
        draw_text(ui, Rect::from_min_size(egui::pos2(x, pill.top()), Vec2::new(q_w, pill.height())), &question, semibold_font(13.5), c.text, false);
        x += q_w + SPACE * 1.5;
        draw_text(
            ui,
            Rect::from_min_max(egui::pos2(x, pill.top()), egui::pos2(replace_r.left() - SPACE, pill.bottom())),
            note,
            FontId::proportional(12.5),
            c.muted,
            false,
        );
        // Enter does not replace: writing over a file is left to a deliberate click.
        if button_chip(ui, replace_r, "replace", replace, Chip::Danger, true) {
            self.pick_finish(vec![path.to_path_buf()]);
        } else if button_chip(ui, back_r, "back", back, Chip::Soft, true) {
            if let Some(p) = &mut self.pick {
                p.replace = None;
                p.focus_name = true;
            }
        }
    }

    /// While the app checks the choice: a note, and Cancel.
    fn pick_waiting(&mut self, ui: &mut Ui, pill: Rect) {
        let c = colors();
        let y = pill.center().y;
        let cancel_w = chip_width(ui, "Cancel", false, Chip::Soft);
        let cancel_r = Rect::from_min_size(egui::pos2(pill.right() - PAD - cancel_w, y - CHIP_H / 2.0), Vec2::new(cancel_w, CHIP_H));
        ui.painter().text(
            egui::pos2(pill.left() + LEAD, y),
            Align2::LEFT_CENTER,
            "Checking with the app…",
            FontId::proportional(12.5),
            c.muted,
        );
        if button_chip(ui, cancel_r, "wait-cancel", "Cancel", Chip::Soft, true) {
            self.pick_cancel();
        }
    }

    /// The picker's bin: there is none. Places on the rail it leaves out.
    pub(super) fn pick_hides(&self, loc: &Loc) -> bool {
        self.pick.is_some() && *loc == Loc::Bin
    }
}
