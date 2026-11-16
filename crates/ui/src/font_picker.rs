//! The font control of the Appearance page: a dropdown with a search box over the installed
//! families, and a line that previews the font in use. Entries are plain text: showing each in
//! its own face would mean loading every font into the atlas.

use crate::fonts::{self, Family, FontChoice, SF_DEFAULT};
use crate::theme::{self, semibold_font};
use egui::{Id, RichText, TextEdit, Ui};

/// The families whose name has every word of `query` in it (any case); all of them when empty.
pub fn matching<'a>(families: &'a [Family], query: &str) -> Vec<&'a Family> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    families
        .iter()
        .filter(|f| {
            let name = f.name.to_lowercase();
            words.iter().all(|w| name.contains(w))
        })
        .collect()
}

/// The text the dropdown shows for a choice.
fn shown(choice: FontChoice) -> String {
    match choice {
        FontChoice::Default => format!("Default ({})", fonts::default_label()),
        FontChoice::Named(n) => n.as_str().to_owned(),
    }
}

/// The dropdown. Returns the new choice in the frame it is made. Picking the family the default
/// stands for gives `Default`, so the two never differ.
pub fn font_picker(ui: &mut Ui, current: FontChoice) -> Option<FontChoice> {
    let query_id = Id::new("font search");
    let mut pick = None;
    let out = egui::ComboBox::from_id_salt("font family")
        .width(320.0)
        .selected_text(shown(current))
        .show_ui(ui, |ui| {
            let mut query: String = ui.data(|d| d.get_temp(query_id)).unwrap_or_default();
            let search = ui.add(
                TextEdit::singleline(&mut query)
                    .hint_text("Search fonts")
                    .desired_width(300.0),
            );
            if !search.has_focus() && query.is_empty() {
                search.request_focus();
            }
            ui.data_mut(|d| d.insert_temp(query_id, query.clone()));
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                if query.trim().is_empty()
                    && ui
                        .selectable_label(current == FontChoice::Default, shown(FontChoice::Default))
                        .clicked()
                {
                    pick = Some(FontChoice::Default);
                }
                for f in matching(fonts::installed(), &query) {
                    let this = if f.name == SF_DEFAULT {
                        FontChoice::Default
                    } else {
                        FontChoice::named(&f.name)
                    };
                    let on = current == this || current == FontChoice::named(&f.name);
                    if ui.selectable_label(on, &f.name).clicked() {
                        pick = Some(this);
                    }
                }
            });
        });
    let label = format!("Font: {}", shown(current));
    out.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, label.clone()));
    if pick.is_some() {
        ui.data_mut(|d| d.remove::<String>(query_id));
    }
    pick
}

/// A line in the font in use, regular and semibold, so a change is seen before leaving the page.
pub fn preview(ui: &mut Ui) {
    let c = theme::colors();
    ui.add_space(6.0);
    ui.label(RichText::new("The quick brown fox jumps over the lazy dog").size(15.0).color(c.text));
    ui.label(
        RichText::new("Semibold names and headings  0123456789")
            .font(semibold_font(15.0))
            .color(c.text),
    );
    ui.label(theme::muted("Small text, as in captions and details").size(12.0));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fam(n: &str) -> Family {
        Family { name: n.into(), faces: vec![] }
    }

    #[test]
    fn search_needs_every_word_in_any_case() {
        let all = [fam("SF Pro Text"), fam("SF Pro Display"), fam("Inter")];
        assert_eq!(matching(&all, "").len(), 3);
        assert_eq!(matching(&all, "sf disp").len(), 1);
        assert_eq!(matching(&all, "pro sf").len(), 2);
        assert!(matching(&all, "zzz").is_empty());
    }
}
