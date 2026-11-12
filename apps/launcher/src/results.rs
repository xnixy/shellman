//! The ranked result list shown while typing: icon and name per row, the selected row (the top
//! hit at first) highlighted, and a "Run" row last.

use shellman_core::apps_index::App;
use shellman_core::thumbs::Thumbs;
use shellman_ui::theme::{colors, glyph, icon_font};
use shellman_ui::widgets::draw_text;

const ROW_H: f32 = 52.0;
const ICON: f32 = 32.0;

/// One result row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    /// An app, by index into the list.
    App(usize),
    /// shellman's own settings app, a built-in entry that is not in the Apps folder.
    Settings,
    /// Run what was typed.
    Run,
}

/// What the Settings row is called, and so what a query is matched against to show it.
pub const SETTINGS_NAME: &str = "shellman Settings";

/// The rows for a ranked list of app indices: the apps, the Settings entry when the query
/// matches it (`settings`), then "Run" always last, so it is there both when nothing matches and
/// as a deliberate choice when something does.
pub fn items(ranked: impl Iterator<Item = usize>, settings: bool, query_is_empty: bool) -> Vec<Item> {
    let mut items: Vec<Item> = ranked.map(Item::App).collect();
    if !query_is_empty {
        if settings {
            items.push(Item::Settings);
        }
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
pub fn show(
    ui: &mut egui::Ui,
    items: &[Item],
    apps: &[App],
    query: &str,
    selected: usize,
    thumbs: &mut Thumbs,
    reveal: bool,
) -> Outcome {
    let c = colors();
    let mut out = Outcome::default();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, view| {
            let width = ui.available_width();
            let origin = ui.cursor().min;
            ui.allocate_space(egui::vec2(width, ROW_H * items.len() as f32));
            let moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
            let first = (view.min.y / ROW_H).floor().max(0.0) as usize;
            let last = ((view.max.y / ROW_H).ceil() as usize).min(items.len());
            for (i, item) in items.iter().enumerate().take(last).skip(first) {
                let rect = egui::Rect::from_min_size(
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
                let row = rect.shrink2(egui::vec2(0.0, 2.0));
                if i == selected {
                    ui.painter().rect_filled(row, 10.0, c.row_selected);
                } else if resp.hovered() {
                    ui.painter().rect_filled(row, 10.0, c.row_hover);
                }
                let icon = egui::Rect::from_center_size(
                    egui::pos2(row.left() + 12.0 + ICON / 2.0, row.center().y),
                    egui::Vec2::splat(ICON),
                );
                let text = egui::Rect::from_min_max(
                    egui::pos2(icon.right() + 14.0, row.top()),
                    egui::pos2(row.right() - 12.0, row.bottom()),
                );
                match *item {
                    Item::App(index) => {
                        crate::icon::paint(ui, icon, &apps[index], false, thumbs);
                        draw_text(ui, text, &apps[index].name, egui::FontId::proportional(16.0), c.text, false);
                    }
                    Item::Settings => {
                        ui.painter().text(
                            icon.center(),
                            egui::Align2::CENTER_CENTER,
                            glyph::SETTINGS,
                            icon_font(20.0),
                            c.accent_text,
                        );
                        draw_text(ui, text, SETTINGS_NAME, egui::FontId::proportional(16.0), c.text, false);
                    }
                    Item::Run => {
                        ui.painter().text(
                            icon.center(),
                            egui::Align2::CENTER_CENTER,
                            glyph::OPEN_EXTERNAL,
                            icon_font(20.0),
                            c.accent_text,
                        );
                        draw_text(ui, text, &format!("Run \"{}\"", query.trim()), egui::FontId::proportional(16.0), c.muted, false);
                    }
                }
            }
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_row_is_always_last() {
        assert_eq!(items([3, 1].into_iter(), false, false), [Item::App(3), Item::App(1), Item::Run]);
        assert_eq!(items(std::iter::empty(), false, false), [Item::Run]);
        assert!(items(std::iter::empty(), false, true).is_empty());
    }

    #[test]
    fn settings_entry_sits_before_run_and_only_when_it_matches() {
        assert_eq!(items([2].into_iter(), true, false), [Item::App(2), Item::Settings, Item::Run]);
        assert_eq!(items(std::iter::empty(), true, false), [Item::Settings, Item::Run]);
        // Nothing typed: the grid shows, no rows at all.
        assert!(items(std::iter::empty(), true, true).is_empty());
    }

    #[test]
    fn typing_settings_matches_the_entry() {
        let hit = |q: &str| !crate::rank::rank(&[SETTINGS_NAME], &|_| 0, q, 1).is_empty();
        assert!(hit("sett") && hit("shellman set") && hit("Settings"));
        assert!(!hit("calc"));
    }
}
