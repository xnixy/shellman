//! The "Apps" section of the sidebar: the places other apps registered for Explorer's navigation
//! pane (OneDrive, Dropbox, iCloud, ...), with their own names and icons.

use super::actions::Action;
use super::tab::Loc;
use egui::emath::GuiRounding;
use egui::{ColorImage, TextureHandle, TextureOptions, Ui, Vec2};
use shellman_core::places::Place;
use shellman_core::places_reg::{self, PlacesWatcher};
use shellman_ui::nav::{nav_row_with, section_label};
use shellman_ui::theme::{colors, glyph, icon_font};
use shellman_ui::widgets::menu_item;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};

/// The icon's size in points; the pixels asked for follow the screen's scale.
const ICON_PT: f32 = 16.0;

enum Slot {
    Pending,
    Ready(TextureHandle),
    /// The entry's own icon could not be loaded; the folder's shell icon stands in.
    Missing,
}

/// A request: place id, the icon as registered, and the size in pixels.
type Request = (String, String, i32);

pub(super) struct Registered {
    watcher: PlacesWatcher,
    list: Vec<Place>,
    icons: HashMap<(String, i32), Slot>,
    ask: Sender<Request>,
    done: Receiver<(String, i32, Option<ColorImage>)>,
}

impl Registered {
    pub(super) fn new(ctx: &egui::Context) -> Self {
        let (ask, requests) = channel::<Request>();
        let (reply, done) = channel();
        // One worker: cloud folders' icons can be slow, and the UI must never wait for them.
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            for (id, spec, px) in requests {
                let image = places_reg::load_icon(&spec, px);
                if reply.send((id, px, image)).is_err() {
                    return;
                }
                repaint.request_repaint();
            }
        });
        Self {
            watcher: PlacesWatcher::new(ctx.clone()),
            list: Vec::new(),
            icons: HashMap::new(),
            ask,
            done,
        }
    }

    /// Take what the watcher and the icon worker have for us.
    pub(super) fn poll(&mut self, ctx: &egui::Context) {
        if let Some(list) = self.watcher.poll() {
            self.list = list;
            self.icons.clear();
        }
        while let Ok((id, px, image)) = self.done.try_recv() {
            let slot = match image {
                Some(img) => Slot::Ready(ctx.load_texture(
                    format!("place:{id}:{px}"),
                    img,
                    TextureOptions::LINEAR,
                )),
                None => Slot::Missing,
            };
            self.icons.insert((id, px), slot);
        }
    }

    /// The entry's own icon, once loaded: None while it is loading or when it has none.
    fn icon(&mut self, place: &Place, px: i32) -> Option<&TextureHandle> {
        let key = (place.id.clone(), px);
        if !self.icons.contains_key(&key) {
            match &place.icon {
                Some(spec) => {
                    self.icons.insert(key.clone(), Slot::Pending);
                    let _ = self.ask.send((place.id.clone(), spec.clone(), px));
                }
                None => {
                    self.icons.insert(key.clone(), Slot::Missing);
                }
            }
        }
        match self.icons.get(&key) {
            Some(Slot::Ready(t)) => Some(t),
            _ => None,
        }
    }

    fn icon_pending(&self, place: &Place, px: i32) -> bool {
        matches!(self.icons.get(&(place.id.clone(), px)), Some(Slot::Pending))
    }
}

/// `ids` with `id` added: the hidden list's only operations, kept apart so they can be tested.
pub(super) fn hide(hidden: &mut Vec<String>, id: &str) -> bool {
    if hidden.iter().any(|h| h.eq_ignore_ascii_case(id)) {
        return false;
    }
    hidden.push(id.to_string());
    true
}

pub(super) fn show(hidden: &mut Vec<String>, id: &str) -> bool {
    let before = hidden.len();
    hidden.retain(|h| !h.eq_ignore_ascii_case(id));
    hidden.len() != before
}

fn is_hidden(hidden: &[String], id: &str) -> bool {
    hidden.iter().any(|h| h.eq_ignore_ascii_case(id))
}

impl super::FileMan {
    pub(super) fn sidebar_apps(&mut self, ui: &mut Ui, current: &Loc, actions: &mut Vec<Action>) {
        let shown: Vec<Place> = self
            .apps
            .list
            .iter()
            .filter(|p| !is_hidden(&self.settings.hidden_places, &p.id))
            .cloned()
            .collect();
        if shown.is_empty() {
            return;
        }
        section_label(ui, "Apps");
        let ppp = ui.ctx().pixels_per_point();
        let px = (ICON_PT * ppp).round() as i32;
        for place in shown {
            let loc = Loc::Dir(place.target.clone());
            // Own icon, else the folder's shell icon, else a plain folder glyph.
            let own = self.apps.icon(&place, px).map(|t| t.id());
            let shell = if own.is_none() && !self.apps.icon_pending(&place, px) {
                self.thumbs.get(&place.target, false).map(|t| t.id())
            } else {
                None
            };
            let wait = own.is_none() && shell.is_none() && self.apps.icon_pending(&place, px);
            let resp = nav_row_with(ui, &place.name, *current == loc, false, |p, slot, active| {
                let c = colors();
                if let Some(id) = own.or(shell) {
                    let at = egui::Rect::from_center_size(slot.center(), Vec2::splat(ICON_PT))
                        .round_to_pixels(ppp);
                    p.image(
                        id,
                        at,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                } else if !wait {
                    p.text(
                        slot.left_center(),
                        egui::Align2::LEFT_CENTER,
                        glyph::FOLDER_FILL,
                        icon_font(14.0),
                        if active { c.accent_text } else { c.muted },
                    );
                }
            })
            .on_hover_text(place.target.display().to_string());
            if resp.clicked() {
                actions.push(Action::Navigate(loc.clone()));
            }
            if resp.middle_clicked() {
                actions.push(Action::NewTab(loc.clone()));
            }
            resp.context_menu(|ui| {
                if menu_item(ui, glyph::ADD, "Open in new tab", "") {
                    actions.push(Action::NewTab(loc.clone()));
                    ui.close_menu();
                }
                if menu_item(ui, glyph::CLOSE, "Hide from sidebar", "") {
                    actions.push(Action::HidePlace(place.id.clone()));
                    ui.close_menu();
                }
            });
        }
    }

    /// The empty part of the sidebar: its menu brings hidden entries back.
    pub(super) fn sidebar_background(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let rest = ui.available_rect_before_wrap();
        if rest.height() < 1.0 {
            return;
        }
        let resp = ui.interact(rest, egui::Id::new("sidebar_background"), egui::Sense::click());
        let hidden: Vec<&Place> = self
            .apps
            .list
            .iter()
            .filter(|p| is_hidden(&self.settings.hidden_places, &p.id))
            .collect();
        resp.context_menu(|ui| {
            if hidden.is_empty() {
                ui.add_enabled(false, egui::Button::new("No hidden sidebar items"));
                return;
            }
            ui.menu_button("Show hidden sidebar items", |ui| {
                for p in &hidden {
                    if menu_item(ui, glyph::FOLDER, &p.name, "") {
                        actions.push(Action::ShowPlace(p.id.clone()));
                        ui.close_menu();
                    }
                }
            });
        });
    }

    pub(super) fn set_place_hidden(&mut self, id: &str, hidden: bool) {
        let changed = if hidden {
            hide(&mut self.settings.hidden_places, id)
        } else {
            show(&mut self.settings.hidden_places, id)
        };
        if changed {
            self.save_pending = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_list_hides_once_and_shows_back() {
        let mut h = Vec::new();
        assert!(hide(&mut h, "{ABC}"));
        assert!(!hide(&mut h, "{abc}"));
        assert!(hide(&mut h, "{DEF}"));
        assert!(is_hidden(&h, "{Abc}"));
        assert!(show(&mut h, "{abc}"));
        assert!(!show(&mut h, "{abc}"));
        assert_eq!(h, ["{DEF}"]);
    }
}
