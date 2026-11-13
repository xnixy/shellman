//! The ranked result list shown while typing: icon and name per row, the selected row (the top
//! hit at first) an accent row with an "Enter" hint, and a "Run" row last.

use crate::fade::Fade;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, Painter, Rect};
use shellman_core::apps_index::App;
use shellman_core::thumbs::Thumbs;
use shellman_ui::theme::{colors, glyph, icon_font};

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
}

/// Draw the rows in view. `reveal` scrolls the selected row into view (after arrow keys).
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    items: &[Item],
    apps: &[App],
    query: &str,
    selected: usize,
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
                if resp.clicked() {
                    out.launch = Some(i);
                }
                if resp.hovered() && moved {
                    out.hover = Some(i);
                }
                if i == selected && reveal {
                    ui.scroll_to_rect(rect, None);
                }
                let (is_selected, hovered) = (i == selected, resp.hovered());
                fade.paint(ui, rect, |p| {
                    draw_row(p, rect, *item, apps, query, is_selected, hovered, thumbs)
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
    hovered: bool,
    thumbs: &mut Thumbs,
) {
    let c = colors();
    let row = rect.shrink2(egui::vec2(0.0, 2.0));
    // The selected row is a rounded accent fill, its text on the accent.
    let ink = if selected { c.on_accent } else { c.text };
    if selected {
        p.rect_filled(row, 14.0, c.accent_fill);
    } else if hovered {
        p.rect_filled(row, 12.0, c.text.gamma_multiply(0.08));
    }
    let icon = Rect::from_center_size(
        egui::pos2(row.left() + 14.0 + ICON / 2.0, row.center().y),
        egui::Vec2::splat(ICON),
    );
    // The hint takes room at the right of the selected row.
    let hint_w = if selected { 54.0 } else { 0.0 };
    let name_x = icon.right() + 14.0;
    let name_w = row.right() - 14.0 - hint_w - name_x;
    // The name sits a little above the middle, with what kind of thing it is beneath.
    let name_at = egui::pos2(name_x, row.center().y - 8.0);
    let sub_at = egui::pos2(name_x, row.center().y + 9.0);
    match item {
        Item::App(index) => {
            crate::icon::paint(p, icon, &apps[index], thumbs);
            name(p, name_at, name_w, &apps[index].name, apps[index].alias().as_deref(), Some(query), ink);
            let kind = if apps[index].own {
                "shellman"
            } else if apps[index].windows_twin {
                "Windows"
            } else {
                "App"
            };
            sub(p, sub_at, kind, ink, selected);
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
            sub(p, sub_at, "Run this command", ink, selected);
        }
    }
    if selected {
        hint(p, row, ink);
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
    let any = marks.contains(&true);
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

/// The small line under a name, fainter than it.
fn sub(p: &Painter, at: egui::Pos2, text: &str, ink: Color32, selected: bool) {
    let color = if selected { ink.gamma_multiply(0.8) } else { colors().muted };
    p.text(at, egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(11.5), color);
}

/// "Enter", at the right end of the selected row: what the key will do.
fn hint(p: &Painter, row: Rect, ink: Color32) {
    p.text(
        egui::pos2(row.right() - 16.0, row.center().y),
        egui::Align2::RIGHT_CENTER,
        "Enter",
        egui::FontId::proportional(12.0),
        ink.gamma_multiply(0.85),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_row_is_always_last() {
        assert_eq!(items([3, 1].into_iter(), false), [Item::App(3), Item::App(1), Item::Run]);
        assert_eq!(items(std::iter::empty(), false), [Item::Run]);
        assert!(items(std::iter::empty(), true).is_empty());
    }
}
