//! The connectivity grid: Wi-Fi, Bluetooth, the network, and Focus where Windows offers it, in
//! tiles two to a row. The chip turns a radio on or off; the rest of the tile (or a right click)
//! opens the matching Settings page.

use super::kit::{self, clipped_text, label_font, small_font, CHIP, GAP, TILE_INSET, TILE_RADIUS};
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Ui, Vec2};
use shellman_core::system::radios::{Connectivity, Kind, Switch};
use shellman_core::system::status::Network;
use shellman_ui::chips::soft_colors;
use shellman_ui::theme::{self, colors, glyph, icon_font, with_alpha};

const TILE_HEIGHT: f32 = 66.0;

/// Whether there is any tile to show.
pub fn has_tiles(n: &Connectivity) -> bool {
    !tiles(n).is_empty()
}

/// What a tile's chip does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    Radio(Kind, bool),
    Focus(bool),
}

/// What the user asked for in the grid.
pub enum Asked {
    Toggle(Toggle),
    Open(&'static str),
}

#[derive(Debug, PartialEq)]
struct Tile {
    glyph: char,
    title: &'static str,
    detail: String,
    on: bool,
    /// What the chip does; None when it cannot be switched here.
    toggle: Option<Toggle>,
    page: &'static str,
}

/// The tiles for what this machine has. A missing radio has no tile.
fn tiles(n: &Connectivity) -> Vec<Tile> {
    let mut list = Vec::new();
    if let Some(wifi) = n.wifi {
        let detail = match wifi {
            Switch::On => n.wifi_name.clone().unwrap_or_else(|| "Not connected".into()),
            Switch::Off => "Off".into(),
            Switch::Disabled => "Unavailable".into(),
        };
        list.push(radio_tile(glyph::WIFI, "Wi-Fi", detail, wifi, Kind::WiFi, "ms-settings:network-wifi"));
    }
    if let Some(bt) = n.bluetooth {
        let detail = match bt {
            Switch::On => "On",
            Switch::Off => "Off",
            Switch::Disabled => "Unavailable",
        };
        list.push(radio_tile(glyph::BLUETOOTH, "Bluetooth", detail.into(), bt, Kind::Bluetooth, "ms-settings:bluetooth"));
    }
    if let Some(network) = n.network {
        // Wi-Fi's own tile names its network; this one is about the wired connection then.
        let on_wifi = n.wifi_name.is_some();
        let name = n.network_name.clone().filter(|_| network != Network::Offline);
        let detail = match (network, name) {
            (Network::Offline, _) => "Offline".to_string(),
            (Network::Internet, Some(name)) => name,
            (Network::Internet, None) => "Connected".into(),
            (Network::Local, Some(name)) => format!("{name} · No internet"),
            (Network::Local, None) => "No internet".into(),
        };
        list.push(Tile {
            glyph: if on_wifi || network == Network::Offline { glyph::GLOBE } else { glyph::ETHERNET },
            title: if on_wifi { "Network" } else { "Ethernet" },
            detail,
            on: network != Network::Offline,
            toggle: None,
            page: if on_wifi { "ms-settings:network-status" } else { "ms-settings:network-ethernet" },
        });
    }
    if let Some(focus) = n.focus {
        list.push(Tile {
            glyph: glyph::MOON,
            title: "Focus",
            detail: if focus { "On" } else { "Off" }.into(),
            on: focus,
            toggle: Some(Toggle::Focus(!focus)),
            page: "ms-settings:quiethours",
        });
    }
    list
}

fn radio_tile(g: char, title: &'static str, detail: String, s: Switch, kind: Kind, page: &'static str) -> Tile {
    Tile {
        glyph: g,
        title,
        detail,
        on: s.is_on(),
        toggle: (s != Switch::Disabled).then_some(Toggle::Radio(kind, !s.is_on())),
        page,
    }
}

/// Lay the grid out. Returns what was asked for.
/// `links` is whether Settings can open: without it, a tile whose page has no classic fallback
/// shows no chevron and its body does nothing (the chip still toggles).
pub fn show(ui: &mut Ui, n: &Connectivity, links: bool) -> Option<Asked> {
    let list = tiles(n);
    if list.is_empty() {
        return None;
    }
    let width = ui.available_width();
    let half = (width - GAP) / 2.0;
    let mut asked = None;
    for row in list.chunks(2) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, TILE_HEIGHT), Sense::hover());
        for (i, t) in row.iter().enumerate() {
            let w = if row.len() == 1 { width } else { half };
            let r = Rect::from_min_size(Pos2::new(rect.left() + i as f32 * (half + GAP), rect.top()), Vec2::new(w, TILE_HEIGHT));
            if let Some(a) = tile(ui, r, t, links) {
                asked = Some(a);
            }
        }
    }
    asked
}

fn tile(ui: &mut Ui, rect: Rect, t: &Tile, links: bool) -> Option<Asked> {
    let links = crate::settings_page::can_open(t.page, links);
    let c = colors();
    let id = egui::Id::new(("connect", t.title));
    let body = ui.interact(rect, id, if links { Sense::click() } else { Sense::hover() });
    let chip_center = Pos2::new(rect.left() + TILE_INSET + CHIP / 2.0, rect.center().y);
    let chip_rect = Rect::from_center_size(chip_center, Vec2::splat(CHIP));
    let chip_resp = ui.interact(chip_rect, id.with("chip"), Sense::click());
    let chip_hover = chip_resp.hovered() && t.toggle.is_some();
    // On: the accent fills the tile. Off: a soft tile with a hairline.
    let (on_fill, on_fg) = kit::on_colors(&c);
    let (soft, soft_hover, line) = soft_colors(c.light);
    let body_hover = links && body.hovered() && !chip_resp.hovered();
    let (fill, stroke, ink, detail) = if t.on {
        let fill = if body_hover { theme::mix(on_fill, Color32::WHITE, 0.08) } else { on_fill };
        (fill, egui::Stroke::NONE, on_fg, with_alpha(on_fg, 215))
    } else {
        (if body_hover { soft_hover } else { soft }, line, c.text, c.muted)
    };
    ui.painter().rect(rect, CornerRadius::same(TILE_RADIUS), fill, stroke, egui::StrokeKind::Inside);
    // The chip is a round well in the tile: a veil of the ink colour, stronger when hovered.
    let veil = with_alpha(ink, match (t.on, chip_hover) {
        (_, true) => 70,
        (true, false) => 40,
        (false, false) => 22,
    });
    ui.painter().circle_filled(chip_center, CHIP / 2.0, veil);
    ui.painter().text(chip_center, Align2::CENTER_CENTER, t.glyph, icon_font(kit::GLYPH), ink);
    let text_left = chip_rect.right() + 10.0;
    let text_width = rect.right() - 26.0 - text_left;
    clipped_text(ui, Pos2::new(text_left, rect.center().y - 1.0), Align2::LEFT_BOTTOM, t.title, label_font(), ink, text_width);
    clipped_text(ui, Pos2::new(text_left, rect.center().y + 1.0), Align2::LEFT_TOP, &t.detail, small_font(), detail, text_width);
    if links {
        let chevron = Pos2::new(rect.right() - 16.0, rect.center().y);
        ui.painter().text(chevron, Align2::CENTER_CENTER, glyph::CHEVRON, icon_font(10.0), with_alpha(detail, 170));
    }
    if chip_resp.clicked() {
        return t.toggle.map(Asked::Toggle);
    }
    if links && (body.clicked() || body.secondary_clicked() || chip_resp.secondary_clicked()) {
        return Some(Asked::Open(t.page));
    }
    None
}

/// Show a switch the user flipped at once, before the radio's own event confirms it.
pub fn assume(n: &mut Connectivity, toggle: Toggle) {
    let to = |on: bool| if on { Switch::On } else { Switch::Off };
    match toggle {
        Toggle::Radio(Kind::WiFi, on) => n.wifi = Some(to(on)),
        Toggle::Radio(Kind::Bluetooth, on) => n.bluetooth = Some(to(on)),
        Toggle::Focus(on) => n.focus = Some(on),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_radios_have_no_tiles() {
        let n = Connectivity { network: Some(Network::Internet), ..Default::default() };
        let list = tiles(&n);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Ethernet");
        assert_eq!(list[0].toggle, None);
    }

    #[test]
    fn a_radio_switched_off_in_hardware_cannot_be_toggled() {
        let n = Connectivity { wifi: Some(Switch::Disabled), bluetooth: Some(Switch::Off), ..Default::default() };
        let list = tiles(&n);
        assert_eq!(list[0].toggle, None);
        assert_eq!(list[0].detail, "Unavailable");
        assert_eq!(list[1].toggle, Some(Toggle::Radio(Kind::Bluetooth, true)));
    }

    #[test]
    fn wifi_names_its_network() {
        let n = Connectivity {
            wifi: Some(Switch::On),
            wifi_name: Some("Home".into()),
            network: Some(Network::Internet),
            network_name: Some("Home".into()),
            ..Default::default()
        };
        let list = tiles(&n);
        assert_eq!(list[0].detail, "Home");
        assert_eq!(list[0].toggle, Some(Toggle::Radio(Kind::WiFi, false)));
        assert_eq!(list[1].title, "Network");
    }

    #[test]
    fn a_flipped_switch_shows_at_once() {
        let mut n = Connectivity { bluetooth: Some(Switch::On), ..Default::default() };
        assume(&mut n, Toggle::Radio(Kind::Bluetooth, false));
        assert_eq!(n.bluetooth, Some(Switch::Off));
    }
}
