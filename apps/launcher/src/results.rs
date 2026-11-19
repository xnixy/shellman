//! The ranked result list shown while typing: icon and name per row, the selected row (the top
//! hit at first) marked with the field fill and an "Enter" hint, and a "Run" row last.

use crate::fade::Fade;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, Painter, Rect};
use shellman_core::apps_index::App;
use shellman_core::thumbs::Thumbs;
use shellman_ui::chips::soft_colors;
use shellman_ui::theme::{colors, glyph, icon_font, shape};

/// A row's height; the results island is as tall as its rows.
pub const ROW_H: f32 = 58.0;
const ICON: f32 = 32.0;
const NAME_SIZE: f32 = 15.0;

/// One result row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    /// An app, by index into the list.
    App(usize),
    /// Run what was typed.
    Run,
}

/// The rows for a ranked list of app indices, then "Run" always last, so it is there both when
/// nothing matches and as a deliberate choice when something does.
pub fn items(ranked: impl Iterator<Item = usize>, query_is_empty: bool) -> Vec<Item> {
    let mut items: Vec<Item> = ranked.map(Item::App).collect();
    if !query_is_empty {
        items.push(Item::Run);
    }
    items
}

#[derive(Default)]
pub struct Outcome {
    pub launch: Option<usize>,
    pub hover: Option<usize>,
    /// A row was right-clicked (which, and where the pointer was).
    pub menu: Option<(usize, egui::Pos2)>,
}

/// Draw the rows in view. `reveal` scrolls the selected row into view (after arrow keys).
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    items: &[Item],
    apps: &[App],
    query: &str,
    selected: usize,
    admin: bool,
    thumbs: &mut Thumbs,
    fade: Fade,
    reveal: bool,
) -> Outcome {
    let mut out = Outcome::default();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show_viewport(ui, |ui, view| {
            let width = ui.available_width();
            let origin = ui.cursor().min;
            // A little room at the end, so the last row can scroll clear of the fade.
            ui.allocate_space(egui::vec2(width, ROW_H * items.len() as f32 + 16.0));
            let moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
            let first = (view.min.y / ROW_H).floor().max(0.0) as usize;
            let last = ((view.max.y / ROW_H).ceil() as usize).min(items.len());
            for (i, item) in items.iter().enumerate().take(last).skip(first) {
                let rect = Rect::from_min_size(
                    origin + egui::vec2(0.0, i as f32 * ROW_H),
                    egui::vec2(width, ROW_H),
                );
                let resp = ui.interact(rect, egui::Id::new(("result", i)), egui::Sense::click());
                // The search field keeps the keyboard; arrows move the selection, which a screen
                // reader sees as this row's selected state.
                shellman_ui::theme::speak_selected(&resp, egui::WidgetType::SelectableLabel, i == selected, || spoken(*item, apps, query));
                if resp.clicked() {
                    out.launch = Some(i);
                }
                if resp.secondary_clicked() {
                    out.menu = Some((i, resp.interact_pointer_pos().unwrap_or(rect.center())));
                }
                if resp.hovered() && moved {
                    out.hover = Some(i);
                }
                if i == selected && reveal {
                    ui.scroll_to_rect(rect, None);
                }
                let (is_selected, hovered) = (i == selected, resp.hovered());
                fade.paint(ui, rect, |p| {
                    draw_row(p, rect, *item, apps, query, is_selected, admin, hovered, thumbs)
                });
            }
        });
    out
}

#[allow(clippy::too_many_arguments)]
fn draw_row(
    p: &Painter,
    rect: Rect,
    item: Item,
    apps: &[App],
    query: &str,
    selected: bool,
    admin: bool,
    hovered: bool,
    thumbs: &mut Thumbs,
) {
    let c = colors();
    let row = rect.shrink2(egui::vec2(0.0, 2.0));
    // Glasshouse's chosen row: the field fill inside a hairline, ordinary text on it (the accent
    // is kept for what is on). High contrast: a highlight outline around ordinary text, and a
    // text outline for hover.
    let ink = c.text;
    let radius = shape::corner(shape::TILE);
    let (soft, soft_hover, line) = soft_colors(c.light);
    if c.high_contrast && (selected || hovered) {
        let stroke = if selected { egui::Stroke::new(2.0_f32, c.row_selected) } else { egui::Stroke::new(1.0_f32, c.text) };
        p.rect_stroke(row, radius, stroke, egui::StrokeKind::Inside);
    } else if selected {
        p.rect(row, radius, soft_hover, line, egui::StrokeKind::Inside);
    } else if hovered {
        p.rect_filled(row, radius, soft);
    }
    let icon = Rect::from_center_size(
        egui::pos2(row.left() + 14.0 + ICON / 2.0, row.center().y),
        egui::Vec2::splat(ICON),
    );
    // The hint takes room at the right of the selected row.
    let hint_w = match (selected, admin) {
        (false, _) => 0.0,
        (true, false) => 54.0,
        (true, true) => 150.0,
    };
    let name_x = icon.right() + 14.0;
    let name_w = row.right() - 14.0 - hint_w - name_x;
    // The name sits a little above the middle, with what kind of thing it is beneath.
    let name_at = egui::pos2(name_x, row.center().y - 8.0);
    let sub_at = egui::pos2(name_x, row.center().y + 9.0);
    match item {
        Item::App(index) => {
            crate::icon::paint(p, icon, &apps[index], thumbs);
            name(p, name_at, name_w, &apps[index].name, apps[index].alias().as_deref(), Some(query), ink);
            let kind = kind_of(&apps[index]);
            sub(p, sub_at, kind);
        }
        Item::Run => {
            crate::icon::plate(p, icon);
            p.text(
                icon.center(),
                egui::Align2::CENTER_CENTER,
                glyph::OPEN_EXTERNAL,
                icon_font(16.0),
                c.accent_text,
            );
            name(p, name_at, name_w, &format!("Run \"{}\"", query.trim()), None, None, ink);
            sub(p, sub_at, "Run this command");
        }
    }
    if selected {
        hint(p, row, ink, admin);
    }
}

/// A name at `at` (left, vertically centred), cut to `width`; letters the query matched stand
/// out against the rest.
fn name(
    p: &Painter,
    at: egui::Pos2,
    width: f32,
    text: &str,
    alias: Option<&str>,
    query: Option<&str>,
    ink: Color32,
) {
    let marks = query.map(|q| crate::rank::matched_with_alias(text, alias, q)).unwrap_or_default();
    // High contrast has no faded text colour: the matched letters are not set apart there.
    let any = marks.contains(&true) && !colors().high_contrast;
    let format = |on: bool| TextFormat {
        font_id: egui::FontId::proportional(NAME_SIZE),
        color: if any && !on { ink.gamma_multiply(0.72) } else { ink },
        ..Default::default()
    };
    let mut job = LayoutJob::default();
    let mut run = String::new();
    let mut run_on = false;
    for (i, ch) in text.chars().enumerate() {
        let on = marks.get(i).copied().unwrap_or(false);
        if !run.is_empty() && on != run_on {
            job.append(&run, 0.0, format(run_on));
            run.clear();
        }
        run_on = on;
        run.push(ch);
    }
    job.append(&run, 0.0, format(run_on));
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(1.0));
    let galley = p.ctx().fonts(|f| f.layout_job(job));
    p.galley(egui::pos2(at.x, at.y - galley.size().y / 2.0), galley, ink);
}

/// What kind of thing an app is, the line under its name.
fn kind_of(app: &App) -> &'static str {
    if app.own {
        "shellman"
    } else if app.windows_twin {
        "Windows"
    } else {
        "App"
    }
}

/// What a screen reader reads for a row: the name and the line under it.
pub fn spoken(item: Item, apps: &[App], query: &str) -> String {
    match item {
        Item::App(index) => apps.get(index).map_or_else(String::new, |a| format!("{}, {}", a.name, kind_of(a))),
        Item::Run => run_name(query),
    }
}

fn run_name(query: &str) -> String {
    format!("Run \"{}\", run this command", query.trim())
}

/// The small line under a name, fainter than it.
fn sub(p: &Painter, at: egui::Pos2, text: &str) {
    p.text(at, egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(11.5), colors().muted);
}

/// "Enter", at the right end of the selected row: what the key will do. With Ctrl and Shift
/// held it says what Enter does then.
fn hint(p: &Painter, row: Rect, ink: Color32, admin: bool) {
    p.text(
        egui::pos2(row.right() - 16.0, row.center().y),
        egui::Align2::RIGHT_CENTER,
        if admin { "Run as administrator" } else { "Enter" },
        egui::FontId::proportional(12.0),
        if colors().high_contrast { ink } else { ink.gamma_multiply(0.85) },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_run_row_says_what_it_runs() {
        assert_eq!(spoken(Item::Run, &[], "  notepad "), "Run \"notepad\", run this command");
        // An index past the list (a stale frame) reads as nothing rather than panicking.
        assert_eq!(spoken(Item::App(3), &[], ""), "");
    }

    #[test]
    fn run_row_is_always_last() {
        assert_eq!(items([3, 1].into_iter(), false), [Item::App(3), Item::App(1), Item::Run]);
        assert_eq!(items(std::iter::empty(), false), [Item::Run]);
        assert!(items(std::iter::empty(), true).is_empty());
    }
}
