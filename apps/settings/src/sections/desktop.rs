//! Desktop: the desktop's style (Shelf + widgets, or Clean) as two preview tiles, and the shelf's
//! rows: on or off, which displays, which edge, which stacks, and where dropped files go; then the
//! widgets, their calendar and weather (`desktop_widgets`). The shelf follows the file as it changes.

use crate::page;
use crate::pick::Pick;
use crate::state::State;
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui};
use shellman_core::settings::desktop::{DropTo, Edge, ShelfOn, Stack, Style};
use shellman_ui::chips::{self, Chip};
use shellman_ui::glass::islands::shows_as;
use shellman_ui::menu::{self, Icon};
use shellman_ui::theme::{self, colors, glyph, icon_font, semibold_font};
use shellman_ui::widgets::toggle;

const TILE_H: f32 = 150.0;
const ROW_H: f32 = 46.0;

pub fn show(ui: &mut Ui, st: &mut State) {
    page::heading(ui, "Desktop", "What sits on the wallpaper. Right-click the wallpaper for its menu, in either style.");
    let mut changed = false;
    page::group(ui, "Desktop style", |ui| {
        ui.add_space(4.0);
        let gap = 16.0;
        let w = ((ui.available_width() - gap) / 2.0).max(120.0);
        let top = ui.cursor().min;
        for (i, style) in Style::ALL.into_iter().enumerate() {
            let rect = Rect::from_min_size(pos2(top.x + i as f32 * (w + gap), top.y), vec2(w, TILE_H + 70.0));
            if tile(ui, rect, style, st.desktop.style == style).clicked() && st.desktop.style != style {
                st.desktop.style = style;
                changed = true;
            }
        }
        ui.allocate_rect(Rect::from_min_size(top, vec2(2.0 * w + gap, TILE_H + 70.0)), Sense::hover());
    });
    let clean = st.desktop.style == Style::Clean;
    page::group(ui, "Shelf", |ui| {
        if clean {
            ui.label(theme::muted("Clean shows no shelf. Pick Shelf + widgets to use it.").size(12.0));
            ui.add_space(6.0);
        }
        ui.add_enabled_ui(!clean, |ui| {
            changed |= rows(ui, st);
        });
    });
    if changed {
        st.save_desktop();
    }
    super::desktop_widgets::show(ui, st, clean);
    // A folder chosen for dropped files.
    if let Some(pick) = &mut st.desktop_pick {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        if let Some(chosen) = pick.poll() {
            st.desktop_pick = None;
            if let Some(dir) = chosen {
                st.desktop.shelf.drop_to = DropTo::Folder(dir);
                st.save_desktop();
            }
        }
    }
}

/// One row: the label on the left, `add` on the right, a hairline above.
pub(super) fn row(ui: &mut Ui, label: &str, first: bool, add: impl FnOnce(&mut Ui) -> bool) -> bool {
    let c = colors();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    if !first {
        ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0_f32, c.hairline));
    }
    let text = if ui.is_enabled() { c.text } else { theme::with_alpha(c.muted, 150) };
    ui.painter().text(pos2(rect.left(), rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(14.0), text);
    let right = Rect::from_min_max(pos2(rect.left() + 160.0, rect.top()), rect.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(right).layout(egui::Layout::right_to_left(egui::Align::Center)));
    add(&mut child)
}

fn rows(ui: &mut Ui, st: &mut State) -> bool {
    let mut changed = false;
    changed |= row(ui, "Show shelf", true, |ui| toggle(ui, &mut st.desktop.shelf.show, "Show shelf").changed());
    changed |= row(ui, "Show shelf on", false, |ui| {
        let options = ShelfOn::ALL.map(|o| (o, o.label()));
        page::segmented(ui, &mut st.desktop.shelf.on, &options)
    });
    changed |= row(ui, "Edge", false, |ui| {
        let options = Edge::ALL.map(|e| (e, e.label()));
        page::segmented(ui, &mut st.desktop.shelf.edge, &options)
    });
    changed |= row(ui, "Stacks", false, |ui| {
        let mut toggled = None;
        ui.spacing_mut().item_spacing.x = 6.0;
        for stack in Stack::ALL.into_iter().rev() {
            let on = st.desktop.shelf.stacks.contains(&stack);
            let (kind, icon) = if on { (Chip::Soft, Some(glyph::CHECK)) } else { (Chip::Plain, None) };
            let w = chips::chip_width(ui, stack.label(), icon.is_some(), kind);
            let (rect, _) = ui.allocate_exact_size(vec2(w, chips::CHIP_H), Sense::hover());
            if chips::chip(ui, rect, ui.id().with(("stack", stack.label())), stack.label(), icon, kind).clicked() && ui.is_enabled() {
                toggled = Some(stack);
            }
        }
        if let Some(s) = toggled {
            st.desktop.toggle_stack(s);
        }
        toggled.is_some()
    });
    changed |= row(ui, "Dropped files go to", false, |ui| drop_to(ui, st));
    changed
}

/// The chip that says where dropped files go, with its list of places.
fn drop_to(ui: &mut Ui, st: &mut State) -> bool {
    let label = format!("{}  \u{25BE}", st.desktop.shelf.drop_to.label());
    let w = chips::chip_width(ui, &label, false, Chip::Soft);
    let (rect, _) = ui.allocate_exact_size(vec2(w, chips::CHIP_H), Sense::hover());
    let resp = chips::chip(ui, rect, ui.id().with("drop-to"), &label, None, Chip::Soft);
    let mut changed = false;
    let popup = ui.id().with("drop-to-menu");
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup));
    }
    egui::popup_below_widget(ui, popup, &resp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        menu::flat_popup(ui);
        ui.set_min_width(200.0);
        for to in DropTo::KNOWN {
            let on = st.desktop.shelf.drop_to == to;
            if menu::flat_item(ui, Icon::None, &to.label(), "", on, true) && !on {
                st.desktop.shelf.drop_to = to;
                changed = true;
            }
        }
        menu::flat_separator(ui);
        let custom = matches!(st.desktop.shelf.drop_to, DropTo::Folder(_));
        if menu::flat_item(ui, Icon::Glyph(glyph::FOLDER), "Choose a folder…", "", custom, st.desktop_pick.is_none()) {
            match Pick::start(shellman_pick::Mode::Folder, "Where dropped files go", Vec::new()) {
                Ok(p) => st.desktop_pick = Some(p),
                Err(e) => eprintln!("{e}"),
            }
        }
    });
    changed
}

/// A preview tile of one style: a small desktop, its name, a line about it, and a round mark.
fn tile(ui: &mut Ui, rect: Rect, style: Style, on: bool) -> egui::Response {
    let c = colors();
    let resp = ui.interact(rect, ui.id().with(("style", style.label())), Sense::click());
    let p = ui.painter();
    let r = CornerRadius::same(20);
    let (soft, soft_hover, line) = chips::soft_colors(c.light);
    if on {
        p.rect_filled(rect, r, shows_as(theme::with_alpha(c.text, if c.light { 15 } else { 23 })));
        p.rect_stroke(rect, r, Stroke::new(2.0_f32, c.text), egui::StrokeKind::Inside);
    } else {
        p.rect(rect, r, if resp.hovered() { soft_hover } else { soft }, line, egui::StrokeKind::Inside);
    }
    let shot = Rect::from_min_size(rect.min + vec2(10.0, 10.0), vec2(rect.width() - 20.0, TILE_H));
    mini_desktop(p, shot, style);
    let name_y = shot.bottom() + 24.0;
    p.text(pos2(rect.left() + 14.0, name_y), Align2::LEFT_CENTER, style.label(), semibold_font(14.0), c.text);
    let mark = pos2(rect.right() - 25.0, name_y);
    if on {
        p.circle_filled(mark, 11.0, c.text);
        p.text(mark, Align2::CENTER_CENTER, glyph::CHECK, icon_font(11.0), c.bg);
    } else {
        p.circle_stroke(mark, 10.0, Stroke::new(1.5_f32, c.muted));
    }
    p.text(pos2(rect.left() + 14.0, name_y + 21.0), Align2::LEFT_CENTER, style.about(), FontId::proportional(12.0), c.muted);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// A little desktop: a dusk wallpaper, the top bar's three islands, the dock, and for Shelf +
/// widgets the shelf on the left and two widgets top right.
fn mini_desktop(p: &egui::Painter, r: Rect, style: Style) {
    let mut mesh = egui::Mesh::default();
    let (a, b) = (Color32::from_rgb(14, 20, 40), Color32::from_rgb(58, 28, 64));
    let (tl, tr) = (Color32::from_rgb(10, 13, 24), Color32::from_rgb(30, 60, 140));
    mesh.colored_vertex(r.left_top(), tl);
    mesh.colored_vertex(r.right_top(), tr);
    mesh.colored_vertex(r.right_bottom(), a);
    mesh.colored_vertex(r.left_bottom(), b);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(mesh));
    p.rect_stroke(r, CornerRadius::same(12), Stroke::new(1.0_f32, Color32::from_white_alpha(20)), egui::StrokeKind::Inside);
    let glass = |x: f32, y: f32, w: f32, h: f32, rad: u8| {
        let g = Rect::from_min_size(pos2(r.left() + x, r.top() + y), vec2(w, h));
        p.rect(g, CornerRadius::same(rad), Color32::from_white_alpha(38), Stroke::new(1.0_f32, Color32::from_white_alpha(46)), egui::StrokeKind::Inside);
    };
    let w = r.width();
    glass(6.0, 5.0, 34.0, 8.0, 4);
    glass(w / 2.0 - 18.0, 5.0, 36.0, 8.0, 4);
    glass(w - 46.0, 5.0, 40.0, 8.0, 4);
    glass(w / 2.0 - 40.0, r.height() - 18.0, 80.0, 12.0, 5);
    if style == Style::ShelfWidgets {
        glass(5.0, 42.0, 20.0, 68.0, 7);
        glass(w - 72.0, 19.0, 66.0, 52.0, 7);
        glass(w - 72.0, 75.0, 66.0, 15.0, 5);
    }
}
