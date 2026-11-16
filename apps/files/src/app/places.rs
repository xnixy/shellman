//! The apps' places on the rail: the places other apps registered for Explorer's navigation
//! pane (OneDrive, Dropbox, iCloud, ...), with their own names and icons.

use super::actions::Action;
use super::tab::Loc;
use super::rail::{Kind, Spot};
use egui::{ColorImage, TextureHandle, TextureOptions};
use shellman_ui::rail::{Icon, Row};
use shellman_core::places::Place;
use shellman_core::places_reg::{self, PlacesWatcher};
use shellman_ui::theme::glyph;
use super::menu::item;
use shellman_ui::menu::popup::Entry;
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
                        let width = img.size[0];
                        mono(&mut img.pixels, width);
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
/// pixel becomes white, and how solid it is follows how far its colour stands out from the
/// tile's. The tile's colour is the most common one inside it; whatever differs from it is the
/// figure: a white cloud on a blue tile, a blue cloud on a white one (iCloud Drive), a coloured
/// flower on white (iCloud Photos), a dark logo on a light tile. The tile's own rim (a thin dark
/// or light edge) is left out, so it never comes through as an empty square. A flat icon keeps
/// its outline. `width`: pixels per row (the icon is `width` wide).
fn mono(pixels: &mut [egui::Color32], width: usize) {
    let width = width.max(1);
    let height = pixels.len() / width;
    // Colours are stored premultiplied; undo that so a soft edge is not read as dark.
    let rgb = |c: &egui::Color32| {
        let a = c.a().max(1) as f32;
        [c.r() as f32 / a, c.g() as f32 / a, c.b() as f32 / a]
    };
    // Within `rim` pixels of the outside (a see-through pixel or the image's edge): the tile's rim.
    let rim = (width / 24).max(1) as isize;
    let near_edge = |i: usize| {
        let (x, y) = ((i % width) as isize, (i / width) as isize);
        (-rim..=rim).any(|dy| {
            (-rim..=rim).any(|dx| {
                let (nx, ny) = (x + dx, y + dy);
                nx < 0 || ny < 0 || nx >= width as isize || ny >= height as isize || pixels[ny as usize * width + nx as usize].a() < 200
            })
        })
    };
    let inner: Vec<usize> = (0..pixels.len()).filter(|&i| pixels[i].a() >= 200 && !near_edge(i)).collect();
    // The tile's colour: the per-channel median of the inside.
    let median = |k: usize| {
        let mut v: Vec<f32> = inner.iter().map(|&i| rgb(&pixels[i])[k]).collect();
        v.sort_by(|a, b| a.total_cmp(b));
        v.get(v.len() / 2).copied().unwrap_or(0.0)
    };
    let tile = [median(0), median(1), median(2)];
    let dist = |c: &egui::Color32| {
        let p = rgb(c);
        ((p[0] - tile[0]).powi(2) + (p[1] - tile[1]).powi(2) + (p[2] - tile[2]).powi(2)).sqrt() / 3f32.sqrt()
    };
    let most = inner.iter().map(|&i| dist(&pixels[i])).fold(0.0f32, f32::max);
    let flat = inner.is_empty() || most < 0.12;
    let level = |d: f32| {
        let t = ((d / most - 0.25) / 0.4).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let parts: Vec<f32> = (0..pixels.len())
        .map(|i| if flat { 1.0 } else if near_edge(i) { 0.0 } else { level(dist(&pixels[i])) })
        .collect();
    for (c, part) in pixels.iter_mut().zip(parts) {
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

    /// The apps' places' rows on the rail, with their own icons: the app's, else the folder's
    /// shell icon, else a plain folder glyph (nothing while the app's icon is still loading).
    pub(super) fn rail_apps(&mut self, ctx: &egui::Context) -> Vec<(Row<'static>, Spot)> {
        let px = (ICON_PT * ctx.pixels_per_point()).round() as i32;
        let mut rows = Vec::new();
        for place in self.shown_apps() {
            let own = self.apps.icon(&place, px).map(|t| t.id());
            let pending = self.apps.icon_pending(&place, px);
            let shell = if own.is_none() && !pending { self.thumbs.get(&place.target, false).map(|t| t.id()) } else { None };
            // The textures are white masks (see `mono`): the rail tints them like its glyphs.
            let icon = match own.or(shell) {
                Some(id) => Icon::Image(id),
                None if pending => Icon::Blank,
                None => Icon::Glyph(glyph::FOLDER_FILL),
            };
            let tip = format!("{}\n{}", place.name, place.target.display());
            let row = Row::new(format!("app:{}", place.id), icon, place.name.clone()).tip(tip);
            rows.push((row, Spot { loc: Loc::Dir(place.target.clone()), kind: Kind::App(place.id.clone()) }));
        }
        rows
    }
    /// The menu of the rail's empty part: it brings hidden places back.
    pub(super) fn rail_background_menu(
        &mut self,
        resp: &egui::Response,
    ) {
        let hidden: Vec<&Place> = self
            .apps
            .list
            .iter()
            .filter(|p| is_hidden(&self.settings.hidden_places, &p.id))
            .collect();
        super::menu::on_right_click(resp, "rail-background", || hidden_entries(&hidden));
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

/// The rail background's menu: the hidden places, to show again, or a note that there are none.
fn hidden_entries(hidden: &[&Place]) -> Vec<Entry<Action>> {
    if hidden.is_empty() {
        return vec![Entry::item(Action::Refresh, "No hidden places").icon(glyph::INFO).enabled(false)];
    }
    hidden.iter().map(|p| item(Action::ShowPlace(p.id.clone()), glyph::FOLDER, format!("Show {}", p.name))).collect()
}
#[cfg(test)]
mod tests {
    use super::*;

    /// An 8 x 8 icon: a `tile` square with a 1 px `rim`, and a 2 x 2 `figure` in the middle.
    fn icon(tile: egui::Color32, rim: egui::Color32, figure: egui::Color32) -> Vec<egui::Color32> {
        (0..64)
            .map(|i| {
                let (x, y) = (i % 8, i / 8);
                if x == 0 || y == 0 || x == 7 || y == 7 {
                    rim
                } else if (3..5).contains(&x) && (3..5).contains(&y) {
                    figure
                } else {
                    tile
                }
            })
            .collect()
    }

    #[test]
    fn a_coloured_icon_becomes_a_white_mark_of_its_figure() {
        use egui::Color32 as C;
        let (centre, tile) = (3 * 8 + 3, 2 * 8 + 2);
        // A white figure on a blue tile: the figure stays, the tile goes.
        let mut px = icon(C::from_rgb(20, 90, 230), C::from_rgb(20, 90, 230), C::WHITE);
        mono(&mut px, 8);
        assert_eq!(px[tile].a(), 0);
        assert_eq!(px[centre], C::WHITE);
        // A dark figure on a light tile.
        let mut px = icon(C::WHITE, C::WHITE, C::from_rgb(10, 10, 10));
        mono(&mut px, 8);
        assert_eq!(px[tile].a(), 0);
        assert_eq!(px[centre], C::WHITE);
        // One flat colour keeps its shape.
        let mut px = vec![C::from_rgb(200, 30, 30); 64];
        mono(&mut px, 8);
        assert!(px.iter().all(|c| *c == C::WHITE));
    }

    /// iCloud Drive's icon: a blue cloud on a white tile with a thin near-black rim. The rim was
    /// the darkest thing in it and swallowed the cloud: an empty square came out.
    #[test]
    fn a_dark_rim_is_not_the_figure() {
        use egui::Color32 as C;
        let mut px = icon(C::WHITE, C::from_rgb(5, 5, 5), C::from_rgb(70, 150, 235));
        mono(&mut px, 8);
        assert_eq!(px[3 * 8 + 3], C::WHITE, "the cloud stays");
        assert_eq!(px[0].a(), 0, "the rim goes");
        assert_eq!(px[2 * 8 + 2].a(), 0, "the tile goes");
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
