//! Painting the two widgets on their glass, as the design has them: Today (the date large, the
//! weather beside it, then the next events or a note) and Now playing (art, title, artist and
//! time, a thin progress line, previous / play-pause / next). The glass itself is the window's.

use super::icons::{Icons, Pic, SkyPic};
use super::model::{NowModel, TodayModel};
use super::place::{self, ART, NOTE_H, PAD_TOP, PAD_X, PLAY_PAD, ROW_H, ROWS_GAP};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui};
use shellman_core::system::media::Action;
use shellman_ui::glass::islands::shows_as;
use shellman_ui::theme::{self, colors, light_font, semibold_font};
use shellman_ui::widgets::draw_text;

/// What the pointer asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Media(Action),
    /// Open Settings on the Desktop page (a note that needs setting up was clicked).
    Settings,
}

/// The design's dimmer grey (`--dim`), under the muted one.
fn dim() -> Color32 {
    let c = colors();
    theme::mix(c.muted, if c.light { Color32::WHITE } else { Color32::BLACK }, 0.22)
}

fn hairline() -> Color32 {
    let c = colors();
    theme::with_alpha(c.text, if c.light { 18 } else { 20 })
}

/// The text colour's faint wash, for hover.
fn soft(light: u8, dark: u8) -> Color32 {
    let c = colors();
    shows_as(theme::with_alpha(c.text, if c.light { light } else { dark }))
}

pub fn today(ui: &mut Ui, rect: Rect, m: &TodayModel, icons: &mut Icons, events: &mut Vec<Event>) {
    let c = colors();
    let p = ui.painter().clone();
    let (left, right) = (rect.left() + PAD_X, rect.right() - PAD_X);
    let top = rect.top() + PAD_TOP;
    // The date.
    p.text(pos2(left, top + 11.0), Align2::LEFT_CENTER, &m.weekday, FontId::proportional(14.0), c.muted);
    p.text(pos2(left - 3.0, top + 26.0 + 32.0), Align2::LEFT_CENTER, &m.day, light_font(64.0), c.text);
    p.text(pos2(left, top + 26.0 + 64.0 + 4.0 + 11.0), Align2::LEFT_CENTER, &m.month, semibold_font(14.0), c.text);
    // The weather, right-aligned beside it.
    if let Some(w) = &m.weather {
        let t = p.text(pos2(right, top + 17.0), Align2::RIGHT_CENTER, &w.temp, light_font(34.0), c.text);
        let icon = Rect::from_center_size(pos2(t.left() - 8.0 - 13.0, top + 17.0), vec2(26.0, 26.0));
        icons.paint(ui, Pic::Sky(SkyPic::from(w.sky)), icon, c.muted);
        p.text(pos2(right, top + 34.0 + 6.0 + 9.0), Align2::RIGHT_CENTER, &w.line, FontId::proportional(12.0), dim());
        p.text(pos2(right, top + 34.0 + 6.0 + 9.0 + 18.0), Align2::RIGHT_CENTER, &w.range, FontId::proportional(12.0), dim());
    }
    let mut y = top + place::HEADER_H + ROWS_GAP;
    if let Some(h) = &m.heading {
        p.hline(left..=right, y, Stroke::new(1.0_f32, hairline()));
        p.text(pos2(left, y + place::HEADING_H / 2.0 + 1.0), Align2::LEFT_CENTER, h, semibold_font(12.0), dim());
        y += place::HEADING_H;
    }
    // The times' column: the design's 46 pt, wider for "10:05 PM" on a 12-hour clock.
    let time_w = m
        .rows
        .iter()
        .map(|r| ui.fonts(|f| f.layout_no_wrap(r.time.clone(), semibold_font(14.0), c.text).size().x))
        .fold(46.0_f32, f32::max);
    for row in &m.rows {
        p.hline(left..=right, y, Stroke::new(1.0_f32, hairline()));
        let line1 = y + 11.0 + 11.0;
        let line2 = y + 11.0 + 22.0 + 2.0 + 9.0;
        p.text(pos2(left, line1), Align2::LEFT_CENTER, &row.time, semibold_font(14.0), c.text);
        let x = left + time_w + 14.0;
        let mut room = right - x;
        if let Some(chip) = &row.chip {
            let font = semibold_font(12.0);
            let w = ui.fonts(|f| f.layout_no_wrap(chip.clone(), font.clone(), c.bg).size().x) + 22.0;
            let r = Rect::from_min_size(pos2(right - w, y + 11.0), vec2(w, 26.0));
            p.rect_filled(r, CornerRadius::same(13), c.text);
            p.text(r.center(), Align2::CENTER_CENTER, chip, font, c.bg);
            room -= w + 10.0;
        }
        let title_font = if row.next { semibold_font(14.0) } else { FontId::proportional(14.0) };
        draw_text(ui, Rect::from_min_size(pos2(x, line1 - 10.0), vec2(room, 20.0)), &row.title, title_font, c.text, false);
        let mut dx = x;
        if let Some(dot) = row.color {
            p.circle_filled(pos2(x + 3.5, line2), 3.5, dot);
            dx += 12.0;
        }
        draw_text(ui, Rect::from_min_size(pos2(dx, line2 - 9.0), vec2(right - dx, 18.0)), &row.detail, FontId::proportional(12.0), dim(), false);
        y += ROW_H;
    }
    if let Some(note) = &m.note {
        let h = if note.detail.is_some() { NOTE_H } else { place::NOTE_ONE_H };
        let r = Rect::from_min_max(pos2(rect.left(), y), pos2(rect.right(), y + h));
        let resp = ui.interact(r, ui.id().with("today-note"), if note.settings { Sense::click() } else { Sense::hover() });
        p.hline(left..=right, y, Stroke::new(1.0_f32, hairline()));
        let line1 = if note.detail.is_some() { y + 22.0 } else { y + h / 2.0 + 1.0 };
        let line2 = y + 11.0 + 22.0 + 2.0 + 9.0;
        let font = if note.strong { semibold_font(14.0) } else { FontId::proportional(14.0) };
        draw_text(ui, Rect::from_min_size(pos2(left, line1 - 10.0), vec2(right - left, 20.0)), &note.title, font, if note.strong { c.text } else { c.muted }, false);
        if let Some(d) = &note.detail {
            draw_text(ui, Rect::from_min_size(pos2(left, line2 - 9.0), vec2(right - left, 18.0)), d, FontId::proportional(12.0), dim(), false);
        }
        if note.settings && resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if note.settings && resp.clicked() {
            events.push(Event::Settings);
        }
    }
}

/// `1:52`, `1:02:05`.
pub fn clock(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// A round button: a glyph, a hover wash, and a click.
fn button(ui: &mut Ui, id: &str, center: egui::Pos2, hit: f32, enabled: bool) -> egui::Response {
    let r = Rect::from_center_size(center, vec2(hit, hit));
    let resp = ui.interact(r, ui.id().with(id), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Now playing while arranging with nothing playing: where it will be, and what it is.
pub fn playing_stand_in(ui: &mut Ui, rect: Rect) {
    let c = colors();
    let p = ui.painter().clone();
    let inner = rect.shrink(PLAY_PAD);
    let art_rect = Rect::from_min_size(inner.min, vec2(ART, ART));
    p.rect_filled(art_rect, CornerRadius::same(14), soft(14, 22));
    p.text(art_rect.center(), Align2::CENTER_CENTER, theme::glyph::AUDIO, theme::icon_font(22.0), c.muted);
    let x = art_rect.right() + 14.0;
    p.text(pos2(x, inner.center().y - 10.0), Align2::LEFT_CENTER, "Now playing", theme::semibold_font(14.0), c.text);
    p.text(pos2(x, inner.center().y + 11.0), Align2::LEFT_CENTER, "Shows when music or a video plays", egui::FontId::proportional(12.5), c.muted);
}

pub fn playing(ui: &mut Ui, rect: Rect, m: &NowModel, art: Option<&egui::TextureHandle>, icons: &mut Icons, events: &mut Vec<Event>) {
    let c = colors();
    let p = ui.painter().clone();
    let inner = rect.shrink(PLAY_PAD);
    let art_rect = Rect::from_min_size(inner.min, vec2(ART, ART));
    match art {
        Some(t) => {
            let shape = egui::epaint::RectShape::filled(art_rect, CornerRadius::same(14), Color32::WHITE)
                .with_texture(t.id(), crate::cards::cover_uv(t.size_vec2()));
            p.add(shape);
        }
        None => {
            p.rect_filled(art_rect, CornerRadius::same(14), soft(14, 22));
            p.text(art_rect.center(), Align2::CENTER_CENTER, theme::glyph::AUDIO, theme::icon_font(22.0), c.muted);
        }
    }
    // The buttons, right to left: next (4 pt from the edge), play/pause, previous, 14 pt apart.
    let right = inner.right() - 4.0;
    let cy = inner.center().y;
    let next_c = pos2(right - 7.5, cy);
    let play_c = pos2(right - 15.0 - 14.0 - 18.0, cy);
    let prev_c = pos2(play_c.x - 18.0 - 14.0 - 7.5, cy);
    let quiet = |on: bool| if on { c.muted } else { theme::with_alpha(c.muted, 90) };
    let r = button(ui, "prev", prev_c, 28.0, m.can_previous);
    if r.hovered() && m.can_previous {
        p.circle_filled(prev_c, 14.0, soft(14, 22));
    }
    icons.paint(ui, Pic::Previous, Rect::from_center_size(prev_c, vec2(15.0, 15.0)), if r.hovered() && m.can_previous { c.text } else { quiet(m.can_previous) });
    if r.clicked() {
        events.push(Event::Media(Action::Previous));
    }
    let r = button(ui, "play", play_c, 36.0, m.can_toggle);
    let fill = if r.hovered() && m.can_toggle { theme::mix(c.text, c.bg, 0.12) } else { c.text };
    p.circle_filled(play_c, 18.0, if m.can_toggle { fill } else { theme::with_alpha(c.text, 120) });
    let glyph = if m.playing { Pic::Pause } else { Pic::Play };
    let nudge = if m.playing { 0.0 } else { 1.0 };
    icons.paint(ui, glyph, Rect::from_center_size(play_c + vec2(nudge, 0.0), vec2(14.0, 14.0)), c.bg);
    if r.clicked() {
        events.push(Event::Media(Action::PlayPause));
    }
    let r = button(ui, "next", next_c, 28.0, m.can_next);
    if r.hovered() && m.can_next {
        p.circle_filled(next_c, 14.0, soft(14, 22));
    }
    icons.paint(ui, Pic::Next, Rect::from_center_size(next_c, vec2(15.0, 15.0)), if r.hovered() && m.can_next { c.text } else { quiet(m.can_next) });
    if r.clicked() {
        events.push(Event::Media(Action::Next));
    }
    // Title, artist and time, the progress line.
    let x = art_rect.right() + 14.0;
    let room = (prev_c.x - 7.5 - 14.0 - x).max(10.0);
    let has_bar = m.duration > 0.0;
    let block = 20.0 + 3.0 + 18.0 + if has_bar { 9.0 + 4.0 } else { 0.0 };
    let y0 = cy - block / 2.0;
    draw_text(ui, Rect::from_min_size(pos2(x, y0), vec2(room, 20.0)), &m.title, semibold_font(14.0), c.text, false);
    draw_text(ui, Rect::from_min_size(pos2(x, y0 + 23.0), vec2(room, 18.0)), &m.line, FontId::proportional(12.0), c.muted, false);
    if has_bar {
        let bar = Rect::from_min_size(pos2(x, y0 + 20.0 + 3.0 + 18.0 + 9.0), vec2(room, 4.0));
        p.rect_filled(bar, CornerRadius::same(2), theme::with_alpha(c.text, if c.light { 22 } else { 30 }));
        let f = (m.position / m.duration).clamp(0.0, 1.0) as f32;
        if f > 0.0 {
            let done = Rect::from_min_size(bar.min, vec2((bar.width() * f).max(4.0), 4.0));
            p.rect_filled(done, CornerRadius::same(2), c.text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_clock() {
        assert_eq!(clock(112.4), "1:52");
        assert_eq!(clock(243.0), "4:03");
        assert_eq!(clock(3725.0), "1:02:05");
        assert_eq!(clock(-3.0), "0:00");
    }
}
