//! The notifications bell at the right end of the right island (where the mockup has its count
//! chip): a plain bell when nothing is new, the bell and an accent chip with the count of unseen
//! notifications (up to "9+") when some are waiting, a crossed bell while do-not-disturb is on.
//! A click opens the notifications under it; a right-click opens a small menu (do not disturb,
//! for an hour, until tomorrow, notification settings).

use super::highlight;
use crate::notify::Badge;
use shellman_core::settings::bar::BarSettings;
use shellman_ui::menu::popup::Entry;
use shellman_ui::theme::{colors, glyph, icon_font, mix, semibold_font};

pub const BELL: char = '\u{EA8F}';
pub const BELL_OFF: char = '\u{E7ED}';
const ICON: f32 = 16.0;
const H: f32 = 30.0;
const PAD: f32 = 7.0;
const CHIP_H: f32 = 20.0;
const CHIP_GAP: f32 = 6.0;

/// The count as the chip shows it: none for nothing new, "9+" past nine.
pub fn count_text(unseen: usize) -> Option<String> {
    match unseen {
        0 => None,
        1..=9 => Some(unseen.to_string()),
        _ => Some("9+".into()),
    }
}

/// The hover tip.
pub fn tip(b: Badge) -> String {
    match (b.dnd, b.unseen, b.count) {
        (true, 0, _) => "Do not disturb is on".into(),
        (true, n, _) => format!("Do not disturb is on · {n} new"),
        (false, 0, 0) => "No new notifications".into(),
        (false, 0, 1) => "1 notification".into(),
        (false, 0, n) => format!("{n} notifications"),
        (false, 1, _) => "1 new notification".into(),
        (false, n, _) => format!("{n} new notifications"),
    }
}

/// The bell's right-click menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Do not disturb on or off.
    Dnd,
    DndHour,
    DndTomorrow,
    Settings,
}

/// The menu's entries, with do-not-disturb ticked while on at `now`.
pub fn menu(settings: &BarSettings, now: i64) -> Vec<Entry<Choice>> {
    let on = settings.dnd_at(now);
    vec![
        Entry::item(Choice::Dnd, "Do not disturb").icon(BELL_OFF).checked(on),
        Entry::item(Choice::DndHour, "For 1 hour"),
        Entry::item(Choice::DndTomorrow, "Until tomorrow"),
        Entry::separator(),
        Entry::item(Choice::Settings, "Notification settings").icon(glyph::SETTINGS),
    ]
}

/// What the bell was asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    Toggle,
    Menu,
}

/// Draw the bell ending at `right`, centred on `mid`. Returns its rectangle and what was asked.
pub fn draw(ui: &mut egui::Ui, mid: f32, right: f32, badge: Badge) -> (egui::Rect, Option<Asked>) {
    let c = colors();
    let count = count_text(badge.unseen);
    let chip = count.as_ref().map(|t| ui.painter().layout_no_wrap(t.clone(), semibold_font(12.0), c.on_accent));
    let chip_w = chip.as_ref().map_or(0.0, |g| (g.size().x + 12.0).max(CHIP_H));
    let width = if chip.is_some() { PAD + ICON + CHIP_GAP + chip_w + (H - CHIP_H) / 2.0 } else { H };
    let rect = egui::Rect::from_min_max(egui::pos2(right - width, mid - H / 2.0), egui::pos2(right, mid + H / 2.0));
    let resp = ui.interact(rect, egui::Id::new("notifications bell"), egui::Sense::click());
    // "Selected" while the list is open.
    shellman_ui::theme::speak_selected(&resp, egui::WidgetType::Button, badge.open, || format!("Notifications, {}", tip(badge)));
    highlight(ui, rect, &resp, (H / 2.0) as u8);
    let glyph = if badge.dnd { BELL_OFF } else { BELL };
    let icon_x = if chip.is_some() { rect.left() + PAD + ICON / 2.0 } else { rect.center().x };
    let color = if badge.open { c.accent_text } else { c.text };
    ui.painter().text(egui::pos2(icon_x, mid), egui::Align2::CENTER_CENTER, glyph, icon_font(ICON), color);
    if let Some(g) = chip {
        let r = egui::Rect::from_min_size(egui::pos2(icon_x + ICON / 2.0 + CHIP_GAP, mid - CHIP_H / 2.0), egui::vec2(chip_w, CHIP_H));
        // Hovered, the accent leans toward the text colour (darker on light, lighter on dark) so
        // the count keeps its contrast; high contrast keeps Windows' highlight pair as it is.
        let fill = if resp.hovered() && !c.high_contrast { mix(c.accent_fill, c.text, 0.12) } else { c.accent_fill };
        ui.painter().rect_filled(r, (CHIP_H / 2.0) as u8, fill);
        ui.painter().galley(r.center() - g.size() / 2.0, g, c.on_accent);
    }
    let resp = resp.on_hover_text(tip(badge));
    let asked = if resp.clicked() {
        Some(Asked::Toggle)
    } else if resp.secondary_clicked() {
        Some(Asked::Menu)
    } else {
        None
    };
    (rect, asked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_is_capped() {
        assert_eq!(count_text(0), None);
        assert_eq!(count_text(3).as_deref(), Some("3"));
        assert_eq!(count_text(9).as_deref(), Some("9"));
        assert_eq!(count_text(10).as_deref(), Some("9+"));
    }

    #[test]
    fn the_menu_ticks_do_not_disturb_while_on() {
        let ticked = |s: &BarSettings, now| match &menu(s, now)[0] {
            Entry::Item(i) => i.checked,
            _ => panic!("an item"),
        };
        let mut s = BarSettings::default();
        assert!(!ticked(&s, 100));
        s.do_not_disturb = true;
        s.dnd_until = Some(200);
        assert!(ticked(&s, 100));
        assert!(!ticked(&s, 300), "it ended by itself");
    }

    #[test]
    fn tips_say_what_is_waiting() {
        let b = |unseen, count, dnd| Badge { unseen, count, dnd, ..Default::default() };
        assert_eq!(tip(b(0, 0, false)), "No new notifications");
        assert_eq!(tip(b(0, 3, false)), "3 notifications");
        assert_eq!(tip(b(3, 3, false)), "3 new notifications");
        assert_eq!(tip(b(1, 1, false)), "1 new notification");
        assert_eq!(tip(b(0, 2, true)), "Do not disturb is on");
    }
}
