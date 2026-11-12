//! The apps' places on the rail: the places other apps registered for Explorer's navigation
//! pane (OneDrive, Dropbox, iCloud, ...), with their own names and icons.

use super::actions::Action;
use super::tab::Loc;
use egui::emath::GuiRounding;
use egui::{ColorImage, TextureHandle, TextureOptions, Ui, Vec2};
use shellman_core::places::Place;
use shellman_core::places_reg::{self, PlacesWatcher};
use shellman_ui::theme::{glyph, icon_font};
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
                Some(mut img) => Slot::Ready(ctx.load_texture(
                    format!("place:{id}:{px}"),
                    {
                        mono(&mut img.pixels);
                        img
                    },
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
/// Turn an app's coloured icon into a one-colour mark, so it sits with the rail's glyphs: every
/// pixel becomes white, and how solid it is follows how far its brightness stands out from the
/// rest of the icon. The part that covers less of the icon is taken for the figure (iCloud's
/// white cloud on its blue tile, a dark logo on a light tile). A flat icon keeps its outline.
fn mono(pixels: &mut [egui::Color32]) {
    let lum = |c: &egui::Color32| {
        // Colours are stored premultiplied; undo that so a soft edge is not read as dark.
        let a = c.a().max(1) as f32;
        (0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32) / a
    };
    let solid: Vec<f32> = pixels.iter().filter(|c| c.a() > 24).map(lum).collect();
    let (lo, hi) = solid.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &l| (lo.min(l), hi.max(l)));
    let flat = solid.is_empty() || hi - lo < 0.15;
    // Steepened, so a tile's own shading (a gradient, a soft shadow) does not come through as a
    // faint square round the figure.
    let level = |l: f32| {
        let t = (((l - lo) / (hi - lo) - 0.45) / 0.35).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    // Mostly bright means the figure is the dark part.
    let invert = !flat && solid.iter().map(|&l| (l - lo) / (hi - lo)).sum::<f32>() / solid.len() as f32 > 0.5;
    for c in pixels.iter_mut() {
        let part = if flat {
            1.0
        } else {
            let l = lum(c);
            level(if invert { lo + hi - l } else { l })
        };
        let a = (c.a() as f32 * part).round() as u8;
        *c = egui::Color32::from_rgba_premultiplied(a, a, a, a);
    }
}

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
    /// The apps' places the user has not hidden.
    pub(super) fn shown_apps(&self) -> Vec<Place> {
        self.apps
            .list
            .iter()
            .filter(|p| !is_hidden(&self.settings.hidden_places, &p.id))
            .cloned()
            .collect()
    }

    /// The apps' places, as icons on the rail; their names show on hover.
    pub(super) fn rail_apps(&mut self, ui: &mut Ui, current: &Loc, actions: &mut Vec<Action>) {
        let ppp = ui.ctx().pixels_per_point();
        let px = (ICON_PT * ppp).round() as i32;
        for place in self.shown_apps() {
            let loc = Loc::Dir(place.target.clone());
            // Own icon, else the folder's shell icon, else a plain folder glyph.
            let own = self.apps.icon(&place, px).map(|t| t.id());
            let shell = if own.is_none() && !self.apps.icon_pending(&place, px) {
                self.thumbs.get(&place.target, false).map(|t| t.id())
            } else {
                None
            };
            let wait = own.is_none() && shell.is_none() && self.apps.icon_pending(&place, px);
            let resp = super::rail::item(
                ui,
                ("app", &place.id),
                *current == loc,
                false,
                |p, square, fg| {
                    if let Some(id) = own.or(shell) {
                        let at =
                            egui::Rect::from_center_size(square.center(), Vec2::splat(ICON_PT))
                                .round_to_pixels(ppp);
                        p.image(
                            id,
                            at,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            // The texture is a white mask (see `mono`), so this draws it in the
                            // rail's own colour, like the glyphs beside it.
                            fg,
                        );
                    } else if !wait {
                        p.text(
                            square.center(),
                            egui::Align2::CENTER_CENTER,
                            glyph::FOLDER_FILL,
                            icon_font(15.0),
                            fg,
                        );
                    }
                },
            );
            super::rail::label(ui, &resp, &format!("{}\n{}", place.name, place.target.display()));
            if resp.clicked() {
                actions.push(Action::Navigate(loc.clone()));
            }
            if resp.middle_clicked() {
                actions.push(Action::NewTab(loc.clone()));
            }
            resp.context_menu(|ui| {
                super::controls::flat_items(ui);
                if super::controls::mi(ui, glyph::ADD, "Open in new tab", "") {
                    actions.push(Action::NewTab(loc.clone()));
                    ui.close_menu();
                }
                if super::controls::mi(ui, glyph::CLOSE, "Hide from the rail", "") {
                    actions.push(Action::HidePlace(place.id.clone()));
                    ui.close_menu();
                }
            });
        }
    }

    /// The menu of the rail's empty part: it brings hidden places back.
    pub(super) fn rail_background_menu(
        &mut self,
        resp: &egui::Response,
        actions: &mut Vec<Action>,
    ) {
        let hidden: Vec<&Place> = self
            .apps
            .list
            .iter()
            .filter(|p| is_hidden(&self.settings.hidden_places, &p.id))
            .collect();
        resp.context_menu(|ui| {
                super::controls::flat_items(ui);
            if hidden.is_empty() {
                super::controls::mdisabled(ui, glyph::INFO, "No hidden places");
                return;
            }
            // The hidden places, to show again.
            for p in &hidden {
                if super::controls::mi(ui, glyph::FOLDER, &format!("Show {}", p.name), "") {
                    actions.push(Action::ShowPlace(p.id.clone()));
                    ui.close_menu();
                }
            }
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
    fn a_coloured_icon_becomes_a_white_mark_of_its_figure() {
        use egui::Color32 as C;
        // A blue tile with a small white figure: the figure stays, the tile goes.
        let mut px = vec![C::from_rgb(20, 90, 230); 12];
        px.extend([C::WHITE; 4]);
        px.push(C::TRANSPARENT);
        mono(&mut px);
        assert_eq!(px[0].a(), 0);
        assert_eq!(px[12], C::WHITE);
        assert_eq!(px[16].a(), 0);
        // A light tile with a small dark figure: the dark part is the figure.
        let mut px = vec![C::WHITE; 12];
        px.extend([C::from_rgb(10, 10, 10); 4]);
        mono(&mut px);
        assert_eq!(px[0].a(), 0);
        assert_eq!(px[12], C::WHITE);
        // One flat colour keeps its shape.
        let mut px = vec![C::from_rgb(200, 30, 30); 4];
        mono(&mut px);
        assert!(px.iter().all(|c| *c == C::WHITE));
    }

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
