//! The "now" pill: a quiet island just left of the clock that shows the one live thing worth a
//! glance (`pick`): an event about to start or under way ("Design review · in 12 min", its
//! calendar's colour), else what is playing ("Midnight City · M83" with a still equaliser mark).
//!
//! It lives in the top bar's own window, as a fourth island. Its glass changes size only when
//! the thing shown changes (it snaps to the new width; only the content cross-fades), never per
//! frame; at rest nothing animates and nothing ticks (the countdown moves with the clock's
//! minute repaint, and the track's progress is not shown here). Long titles are cut with an
//! ellipsis; the whole text is in the hover tip.
//!
//! A click on an event opens the calendar on its day; on a track it plays or pauses. The wheel
//! over a track goes to the next (down) or previous (up) one.

pub mod pick;

use super::{highlight, layout};
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{pos2, vec2, Color32, FontId, Rect};
use pick::{Fade, Kind, Shown};
use shellman_core::system::media::Action;
use shellman_ui::theme::{colors, icon_font, semibold_font};
use std::time::{Duration, Instant};

/// Inside the pill's ends, between the mark and the text, and the widest it gets.
const PAD: f32 = 16.0;
const MARK: f32 = 14.0;
const MARK_GAP: f32 = 9.0;
pub const MAX_W: f32 = 340.0;
/// Narrower than this and the pill is left out (a crowded bar).
const MIN_W: f32 = 120.0;
/// The least room the left island keeps for the app's name.
const LEFT_ROOM: f32 = 220.0;
const SEP: &str = "  ·  ";
const PAUSE: char = '\u{E769}';
/// Wheel steps closer than this make one skip.
const SKIP_GAP: Duration = Duration::from_millis(350);

/// What a click or the wheel on the pill asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    /// Open the calendar on this day.
    Day(i64),
    Media(Action),
}

#[derive(Default)]
pub struct Pill {
    pub fade: Fade,
    /// When the wheel last skipped a track.
    skipped: Option<Instant>,
}

fn title_font() -> FontId {
    semibold_font(13.5)
}

fn detail_font() -> FontId {
    FontId::proportional(13.5)
}

fn width_of(ui: &egui::Ui, text: &str, font: FontId) -> f32 {
    ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, Color32::WHITE)).size().x
}

/// The title's and the detail's widths as drawn, cut so the pill stays within `max`.
fn widths(ui: &egui::Ui, s: &Shown, max: f32) -> (f32, f32) {
    let title = width_of(ui, &s.title, title_font()).ceil();
    let detail = if s.detail.is_empty() { 0.0 } else { width_of(ui, &format!("{SEP}{}", s.detail), detail_font()).ceil() };
    let room = (max - 2.0 * PAD - MARK - MARK_GAP).max(0.0);
    // The title gives way first (down to a fair share), then the detail.
    let detail = detail.min((room - title.min(room * 0.5)).max(0.0));
    let title = title.min(room - detail);
    (title, detail)
}

fn content_width(ui: &egui::Ui, s: &Shown, max: f32) -> f32 {
    let (t, d) = widths(ui, s, max);
    2.0 * PAD + MARK + MARK_GAP + t + d
}

fn cut(text: &str, font: FontId, color: Color32, width: f32) -> LayoutJob {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat { font_id: font, color, ..Default::default() });
    job.wrap = TextWrapping::truncate_at_width(width.max(1.0));
    job
}

/// Paint one thing in `rect` at `alpha`.
fn paint(ui: &egui::Ui, rect: Rect, s: &Shown, alpha: f32, hovered: bool) {
    if alpha <= 0.0 {
        return;
    }
    let c = colors();
    let mut p = ui.painter_at(rect);
    p.multiply_opacity(alpha);
    let mid = rect.center().y;
    let mark = pos2(rect.left() + PAD + MARK / 2.0, mid);
    match s.kind {
        Kind::Event => {
            let color = s.color.map_or(c.accent, |[r, g, b]| Color32::from_rgb(r, g, b));
            p.circle_filled(mark, 4.5, color);
        }
        Kind::Media if s.playing && !hovered => {
            // A still equaliser: three bars of different heights, in the accent.
            for (i, h) in [7.0, 12.0, 9.0].into_iter().enumerate() {
                let x = mark.x - 4.5 + i as f32 * 4.5;
                let r = Rect::from_center_size(pos2(x, mid + 6.0 - h / 2.0), vec2(2.6, h));
                p.rect_filled(r, 1, c.accent_text);
            }
        }
        Kind::Media => {
            // Paused, or hovered while playing: what a click does.
            let g = if s.playing { PAUSE } else { '\u{E768}' };
            p.text(mark, egui::Align2::CENTER_CENTER, g, icon_font(12.0), c.text);
        }
    }
    let (tw, dw) = widths(ui, s, rect.width());
    let x = rect.left() + PAD + MARK + MARK_GAP;
    let title = ui.fonts(|f| f.layout_job(cut(&s.title, title_font(), c.text, tw + 1.0)));
    p.galley(pos2(x, mid - title.size().y / 2.0), title, c.text);
    if dw > 8.0 {
        let detail = ui.fonts(|f| f.layout_job(cut(&format!("{SEP}{}", s.detail), detail_font(), c.muted, dw + 1.0)));
        p.galley(pos2(x + tw, mid - detail.size().y / 2.0), detail, c.muted);
    }
}

impl Pill {
    /// What should show now (from `pick`). True when the bars must repaint.
    pub fn update(&mut self, next: Option<Shown>, now: Instant) -> bool {
        self.fade.update(next, now)
    }

    /// Draw the pill in `row`, ending at `right`, never left of `limit` plus the left island's
    /// room. Returns its island and what it was asked, if anything.
    pub fn draw(&mut self, ui: &mut egui::Ui, row: Rect, right: f32, limit: f32) -> (Option<Rect>, Option<Asked>) {
        let now = Instant::now();
        self.fade.settle(now);
        if !self.fade.visible() {
            return (None, None);
        }
        let room = (right - (limit + LEFT_ROOM)).min(MAX_W);
        if room < MIN_W {
            return (None, None);
        }
        let to_w = self.fade.to.as_ref().map(|s| content_width(ui, s, room));
        let from_w = self.fade.from.as_ref().map(|s| content_width(ui, s, room));
        let Some(w) = pick::island_width(from_w, to_w) else { return (None, None) };
        let w = w.clamp(MIN_W, room).round();
        let island = Rect::from_x_y_ranges(right - w..=right, row.y_range());

        let resp = ui.interact(island, egui::Id::new("now pill"), egui::Sense::click());
        let inset = 5.0;
        highlight(ui, island.shrink(inset), &resp, ((layout::ISLAND_H - 2.0 * inset) / 2.0) as u8);
        let t = self.fade.progress(now);
        let ease = t * t * (3.0 - 2.0 * t);
        if let Some(s) = &self.fade.from {
            paint(ui, island, s, 1.0 - ease, false);
        }
        if let Some(s) = &self.fade.to {
            paint(ui, island, s, ease, resp.hovered());
        }
        if self.fade.moving(now) {
            ui.ctx().request_repaint();
        }

        let mut asked = None;
        if let Some(s) = self.fade.to.clone() {
            let tip = match s.kind {
                Kind::Event => format!("{}{SEP}{}\nClick to open the calendar", s.title, s.detail),
                Kind::Media => format!("{}{SEP}{}\nClick to play or pause · scroll for the next or previous track", s.title, s.detail),
            };
            let resp = resp.on_hover_text(tip);
            if resp.clicked() {
                asked = Some(match s.kind {
                    Kind::Event => Asked::Day(s.day),
                    Kind::Media => Asked::Media(Action::PlayPause),
                });
            }
            if s.kind == Kind::Media && resp.hovered() {
                let scroll = ui.input(|i| i.raw_scroll_delta.y);
                let free = self.skipped.is_none_or(|at| now.saturating_duration_since(at) > SKIP_GAP);
                if scroll != 0.0 && free {
                    self.skipped = Some(now);
                    asked = Some(Asked::Media(if scroll < 0.0 { Action::Next } else { Action::Previous }));
                }
            }
        }
        (Some(island), asked)
    }
}
