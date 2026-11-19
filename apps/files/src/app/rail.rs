//! The rail: the shared sidebar's rows (`shellman_ui::rail`) in the window's sidebar, under its
//! head, with the places, the pinned folders, the tags in use as coloured dots, the apps' places,
//! and This PC with the drives, each group under its heading (a thin line when the rail shows
//! icons only). The current place is lit. Rows take drops, open in a new tab on a middle click,
//! and pins and apps' places have menus; the rail's empty part moves the window, and its menu
//! brings hidden places back.

use super::actions::Action;
use super::dnd::{self, Dest};
use super::tab::Loc;
use egui::{Rect, Ui};
use shellman_ui::appearance::RailApp;
use shellman_ui::glass::{Island, Style};
use shellman_ui::rail::{self, Entry, Icon, Mode, Row};
use shellman_ui::theme::glyph;
use super::menu::item;
use shellman_ui::menu::popup::Entry as MenuEntry;
use std::path::PathBuf;

/// The places shown on the rail and on This PC.
pub(super) fn quick_places() -> Vec<(char, &'static str, PathBuf)> {
    [
        (glyph::HOME, "Home", dirs::home_dir()),
        (glyph::DESKTOP, "Desktop", dirs::desktop_dir()),
        (glyph::DOWNLOADS, "Downloads", dirs::download_dir()),
        (glyph::DOCUMENTS, "Documents", dirs::document_dir()),
        (glyph::PICTURES, "Pictures", dirs::picture_dir()),
        (glyph::MUSIC, "Music", dirs::audio_dir()),
        (glyph::VIDEOS, "Videos", dirs::video_dir()),
    ]
    .into_iter()
    .filter_map(|(g, n, p)| p.map(|p| (g, n, p)))
    .collect()
}

/// What a row of the rail is, beside where it goes.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Kind {
    Place,
    Bin,
    /// A pinned folder, and whether it is still there.
    Pin { there: bool },
    Tag,
    /// An app's place, by its id.
    App(String),
    Pc,
    Drive,
    /// A Linux distribution of WSL.
    Linux,
}

/// Where a row goes, and what it is.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Spot {
    pub(super) loc: Loc,
    pub(super) kind: Kind,
}

impl Spot {
    /// What dropping files on the row does, if anything.
    fn dest(&self) -> Option<Dest> {
        match (&self.kind, &self.loc) {
            (Kind::Bin, _) => Some(Dest::Bin),
            (Kind::Place | Kind::Drive | Kind::Linux | Kind::Pin { there: true }, Loc::Dir(p)) => Some(Dest::Dir(p.clone())),
            _ => None,
        }
    }

    /// Whether the row can be opened (a pin whose folder is gone cannot).
    fn usable(&self) -> bool {
        self.kind != Kind::Pin { there: false }
    }
}

/// The rail's rows, from its groups: each group that has rows under its heading, the empty ones
/// left out. `spots` runs beside the entries (`None` for a heading).
pub(super) struct Rail {
    pub(super) entries: Vec<Entry<'static>>,
    pub(super) spots: Vec<Option<Spot>>,
}

impl Rail {
    pub(super) fn build(groups: Vec<(&'static str, Vec<(Row<'static>, Spot)>)>) -> Self {
        let mut entries = Vec::new();
        let mut spots = Vec::new();
        for (heading, rows) in groups.into_iter().filter(|(_, rows)| !rows.is_empty()) {
            entries.push(Entry::heading(heading));
            spots.push(None);
            for (row, spot) in rows {
                entries.push(row.into());
                spots.push(Some(spot));
            }
        }
        Self { entries, spots }
    }

    /// The key of the row to light: the first one for the place that is open.
    pub(super) fn selected(&self, current: &Loc) -> Option<&str> {
        self.spots.iter().zip(&self.entries).find_map(|(spot, entry)| match (spot, entry) {
            (Some(s), Entry::Row(row)) if s.loc == *current => Some(row.key.as_ref()),
            _ => None,
        })
    }
}

fn glyph_row(key: impl Into<String>, g: char, label: impl Into<String>, loc: Loc, kind: Kind) -> (Row<'static>, Spot) {
    (Row::new(key.into(), Icon::Glyph(g), label.into()), Spot { loc, kind })
}

impl super::FileMan {
    /// The rail's rows for this frame.
    pub(super) fn rail_rows(&mut self, ctx: &egui::Context) -> Rail {
        let current = self.tabs[self.active].loc.clone();
        let mut places: Vec<_> = quick_places()
            .into_iter()
            .map(|(g, name, path)| glyph_row(format!("place:{}", path.display()), g, name, Loc::Dir(path), Kind::Place))
            .collect();
        // A picker has no use for the bin.
        if !self.pick_hides(&Loc::Bin) {
            places.push(glyph_row("bin", glyph::DELETE, "Recycle Bin", Loc::Bin, Kind::Bin));
        }
        let pins = self.rail_pins(ctx);
        let tags = self.rail_tags(&current);
        let apps = self.rail_apps(ctx);
        let mut drives = vec![glyph_row("pc", glyph::PC, "This PC", Loc::Pc, Kind::Pc)];
        drives.extend(self.drives.iter().map(|d| {
            glyph_row(format!("drive:{}", d.root.display()), glyph::DRIVE, d.title(), Loc::Dir(d.root.clone()), Kind::Drive)
        }));
        // Opening one starts the distribution (the listing waits for it on its worker); the
        // row itself costs nothing.
        let linux = self
            .linux
            .iter()
            .map(|d| glyph_row(format!("linux:{}", d.name), glyph::LINUX, d.name.clone(), Loc::Dir(d.root.clone()), Kind::Linux))
            .collect();
        Rail::build(vec![("Places", places), ("Pinned", pins), ("Tags", tags), ("Apps", apps), ("Drives", drives), ("Linux", linux)])
    }

    /// How the rail shows in a window this wide.
    pub(super) fn rail_mode(&self, window_width: f32) -> Mode {
        rail::mode(window_width, self.look.rail_folded(RailApp::Files))
    }

    /// Draw the rail in `column` (the sidebar under its head), and act on what was done to it.
    /// `list` only takes the pill a folded rail shows a name in.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn rail(
        &mut self,
        ui: &mut Ui,
        list: &mut Vec<Island>,
        column: Rect,
        screen: Rect,
        rows: &Rail,
        mode: Mode,
        solid: Option<&Style>,
        actions: &mut Vec<Action>,
    ) {
        let current = self.tabs[self.active].loc.clone();
        let out = rail::show_in(ui, list, column, screen, &rows.entries, rows.selected(&current), mode, solid);
        self.rail_background_menu(&out.background);
        if out.toggled {
            self.look = shellman_ui::appearance::Appearance::toggle_rail_saved(RailApp::Files, self.look);
        }
        for (resp, spot) in out.rows.iter().zip(&rows.spots) {
            let (Some(resp), Some(spot)) = (resp, spot) else { continue };
            if let Some(dest) = spot.dest() {
                dnd::zone(ui, resp.interact_rect, dest, 12.0);
            }
            if resp.clicked() {
                match &spot.loc {
                    Loc::Dir(p) if !spot.usable() => self.toast(format!("{} is not there any more", p.display())),
                    loc => actions.push(Action::Navigate(loc.clone())),
                }
            }
            if resp.middle_clicked() && spot.usable() {
                actions.push(Action::NewTab(spot.loc.clone()));
            }
            self.rail_row_menu(resp, spot);
        }
    }

    /// The menu of a pinned folder or an app's place.
    fn rail_row_menu(&self, resp: &egui::Response, spot: &Spot) {
        super::menu::on_right_click(resp, "rail-row", || row_entries(spot));
    }
}

/// What a sidebar row's menu offers: a pinned folder or an app's place; nothing for the rest.
fn row_entries(spot: &Spot) -> Vec<MenuEntry<Action>> {
    let (Kind::Pin { .. } | Kind::App(_) | Kind::Linux, Loc::Dir(path)) = (&spot.kind, &spot.loc) else { return Vec::new() };
    let mut e = Vec::new();
    if spot.usable() {
        e.push(item(Action::NewTab(spot.loc.clone()), glyph::ADD, "Open in new tab"));
    }
    match &spot.kind {
        Kind::Pin { .. } => e.push(item(Action::TogglePin(path.clone()), glyph::UNPIN, "Unpin from sidebar")),
        Kind::App(id) => e.push(item(Action::HidePlace(id.clone()), glyph::CLOSE, "Hide from the sidebar")),
        _ => {}
    }
    e
}
#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: &str, loc: Loc, kind: Kind) -> (Row<'static>, Spot) {
        glyph_row(key.to_owned(), 'x', key.to_owned(), loc, kind)
    }

    fn dir(p: &str) -> Loc {
        Loc::Dir(PathBuf::from(p))
    }

    #[test]
    fn row_menus() {
        let label = |e: &MenuEntry<Action>| match e {
            MenuEntry::Item(i) => i.label.clone(),
            _ => "-".into(),
        };
        let pin = Spot { loc: dir(r"C:\a"), kind: Kind::Pin { there: true } };
        assert_eq!(row_entries(&pin).iter().map(label).collect::<Vec<_>>(), ["Open in new tab", "Unpin from sidebar"]);
        // A pinned folder that is gone can only be unpinned.
        let gone = Spot { loc: dir(r"C:\gone"), kind: Kind::Pin { there: false } };
        assert_eq!(row_entries(&gone).iter().map(label).collect::<Vec<_>>(), ["Unpin from sidebar"]);
        let app = Spot { loc: dir(r"C:\Dropbox"), kind: Kind::App("dropbox".into()) };
        assert_eq!(row_entries(&app).iter().map(label).collect::<Vec<_>>(), ["Open in new tab", "Hide from the sidebar"]);
        let linux = Spot { loc: dir(r"\\wsl.localhost\Debian"), kind: Kind::Linux };
        assert_eq!(row_entries(&linux).iter().map(label).collect::<Vec<_>>(), ["Open in new tab"]);
        assert!(matches!(linux.dest(), Some(Dest::Dir(_))));
        // Places and drives have no menu.
        assert!(row_entries(&Spot { loc: dir(r"C:\"), kind: Kind::Drive }).is_empty());
    }
    #[test]
    fn empty_groups_have_no_heading_and_spots_line_up() {
        let rail = Rail::build(vec![
            ("Places", vec![row("home", dir(r"C:\Users\a"), Kind::Place), row("bin", Loc::Bin, Kind::Bin)]),
            ("Pinned", vec![]),
            ("Tags", vec![row("tag:0", Loc::Tag(0), Kind::Tag)]),
            ("Drives", vec![row("pc", Loc::Pc, Kind::Pc)]),
        ]);
        let headings: Vec<&str> = rail
            .entries
            .iter()
            .filter_map(|e| match e {
                Entry::Heading(h) => Some(h.as_ref()),
                Entry::Row(_) => None,
            })
            .collect();
        assert_eq!(headings, ["Places", "Tags", "Drives"]);
        assert_eq!(rail.entries.len(), rail.spots.len());
        for (e, s) in rail.entries.iter().zip(&rail.spots) {
            assert_eq!(matches!(e, Entry::Heading(_)), s.is_none());
        }
    }

    #[test]
    fn the_first_row_for_the_open_place_is_lit() {
        let rail = Rail::build(vec![
            ("Places", vec![row("place", dir(r"C:\a"), Kind::Place)]),
            ("Pinned", vec![row("pin", dir(r"C:\a"), Kind::Pin { there: true })]),
        ]);
        assert_eq!(rail.selected(&dir(r"C:\a")), Some("place"));
        assert_eq!(rail.selected(&Loc::Pc), None);
    }

    #[test]
    fn a_gone_pin_takes_no_drops_and_does_not_open() {
        let gone = Spot { loc: dir(r"C:\gone"), kind: Kind::Pin { there: false } };
        assert_eq!(gone.dest(), None);
        assert!(!gone.usable());
        let bin = Spot { loc: Loc::Bin, kind: Kind::Bin };
        assert!(matches!(bin.dest(), Some(Dest::Bin)));
        let tag = Spot { loc: Loc::Tag(1), kind: Kind::Tag };
        assert_eq!(tag.dest(), None);
    }
}
