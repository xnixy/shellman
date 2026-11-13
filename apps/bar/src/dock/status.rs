//! What an app's windows say about themselves through the taskbar (see `tasks`): an overlay
//! badge, progress, and a call for attention, and how each is shown on the dock icon. Pure, so
//! the mapping and the animations' timing are tested without windows.

use shellman_core::taskbar_list::Progress;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// What one window has told the taskbar.
#[derive(Clone, Default)]
pub struct Window {
    pub progress: Progress,
    /// The fraction done, 0..=1.
    pub done: f32,
    /// The overlay badge: a counter that changes with each new picture, and the picture.
    pub badge: Option<(u64, Arc<egui::ColorImage>)>,
    /// Since when it asks for attention (flashing).
    pub attention: Option<Instant>,
}

/// What a dock icon shows for its app.
#[derive(Clone, Default)]
pub struct Status {
    pub progress: Option<(Progress, f32)>,
    /// The window showing that progress.
    pub progress_window: Option<isize>,
    /// The badge of the front-most window that has one: its window and counter, and picture.
    pub badge: Option<(isize, u64, Arc<egui::ColorImage>)>,
    pub attention: Option<Instant>,
}

/// An app's status from its windows (front-most first): the first window showing progress, the
/// first with a badge, the earliest call for attention. Explorer shows one progress per button.
pub fn of(windows: &[isize], known: &HashMap<isize, Window>) -> Status {
    let mut s = Status::default();
    for &w in windows {
        let Some(k) = known.get(&w) else { continue };
        if s.progress.is_none() && k.progress != Progress::None {
            s.progress = Some((k.progress, k.done));
            s.progress_window = Some(w);
        }
        if s.badge.is_none() {
            s.badge = k.badge.as_ref().map(|(rev, img)| (w, *rev, img.clone()));
        }
        if let Some(t) = k.attention {
            s.attention = Some(s.attention.map_or(t, |a| a.min(t)));
        }
    }
    s
}

/// What a dock icon draws for its status this frame.
#[derive(Clone, Default)]
pub struct Shown {
    pub progress: Option<(Progress, f32)>,
    /// An indeterminate sweep moves on this dock (the one on the window's monitor); elsewhere it
    /// stands still.
    pub sweeping: bool,
    pub badge: Option<egui::TextureId>,
    pub attention: Option<Instant>,
}

/// The badge's side, in points.
pub const BADGE: f32 = 20.0;

/// The badge over an icon whose left edge is `left` and top `top`, `icon` points square: a
/// small disc on its top-right corner, sticking out a little.
pub fn badge_rect(left: f32, top: f32, icon: f32) -> egui::Rect {
    let center = egui::pos2(left + icon - BADGE * 0.3, top + BADGE * 0.3);
    egui::Rect::from_center_size(center, egui::vec2(BADGE, BADGE))
}

/// How the bar under the icon looks for a progress state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bar {
    Accent,
    Amber,
    Red,
}

pub fn bar_of(p: Progress) -> Option<Bar> {
    match p {
        Progress::None => None,
        Progress::Normal | Progress::Indeterminate => Some(Bar::Accent),
        Progress::Paused => Some(Bar::Amber),
        Progress::Error => Some(Bar::Red),
    }
}

/// One gentle pulse lasts this long; the icon pulses `PULSES` times, then shows a steady dot.
pub const PULSE: Duration = Duration::from_millis(900);
pub const PULSES: u32 = 2;

/// The pulse's strength (0..=1) `since` the call for attention began, and whether the pulses are
/// over (then only the steady dot shows, and nothing animates).
pub fn attention(since: Duration) -> (f32, bool) {
    let total = PULSE * PULSES;
    if since >= total {
        return (0.0, true);
    }
    let t = (since.as_secs_f32() / PULSE.as_secs_f32()).fract();
    // A soft rise and fall: sin² over each pulse.
    ((t * std::f32::consts::PI).sin().powi(2), false)
}

/// The indeterminate sweep: a segment `SWEEP_W` of the bar wide gliding back and forth, as a
/// fraction of the bar where it starts, `t` seconds in.
pub const SWEEP_W: f32 = 0.35;
const SWEEP_PERIOD: f32 = 2.4;

pub fn sweep(t: f32) -> f32 {
    let phase = (t / SWEEP_PERIOD).fract();
    let tri = 1.0 - (2.0 * phase - 1.0).abs();
    // Eased, so it slows at the ends.
    let eased = tri * tri * (3.0 - 2.0 * tri);
    eased * (1.0 - SWEEP_W)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img() -> Arc<egui::ColorImage> {
        Arc::new(egui::ColorImage::new([2, 2], egui::Color32::RED))
    }

    #[test]
    fn the_front_most_window_with_each_thing_wins() {
        let t = Instant::now();
        let mut known = HashMap::new();
        known.insert(1, Window::default());
        known.insert(2, Window { progress: Progress::Paused, done: 0.4, badge: Some((7, img())), ..Default::default() });
        known.insert(3, Window { progress: Progress::Normal, done: 0.9, badge: Some((9, img())), attention: Some(t), ..Default::default() });
        let s = of(&[1, 2, 3], &known);
        assert_eq!(s.progress, Some((Progress::Paused, 0.4)));
        assert_eq!(s.badge.as_ref().map(|b| (b.0, b.1)), Some((2, 7)));
        assert_eq!(s.attention, Some(t));
        // Nothing known: nothing shown.
        let s = of(&[1, 99], &known);
        assert!(s.progress.is_none() && s.badge.is_none() && s.attention.is_none());
    }

    #[test]
    fn progress_states_map_to_bars() {
        assert_eq!(bar_of(Progress::None), None);
        assert_eq!(bar_of(Progress::Normal), Some(Bar::Accent));
        assert_eq!(bar_of(Progress::Indeterminate), Some(Bar::Accent));
        assert_eq!(bar_of(Progress::Paused), Some(Bar::Amber));
        assert_eq!(bar_of(Progress::Error), Some(Bar::Red));
    }

    #[test]
    fn attention_pulses_twice_then_rests() {
        let ms = Duration::from_millis;
        assert_eq!(attention(ms(0)), (0.0, false));
        let (peak, done) = attention(PULSE / 2);
        assert!(peak > 0.99 && !done);
        let (second, _) = attention(PULSE + PULSE / 2);
        assert!(second > 0.99);
        assert_eq!(attention(PULSE * PULSES), (0.0, true));
        assert_eq!(attention(ms(60_000)), (0.0, true));
    }

    #[test]
    fn the_sweep_stays_on_the_bar() {
        for i in 0..200 {
            let x = sweep(i as f32 * 0.037);
            assert!((0.0..=1.0 - SWEEP_W + 1e-6).contains(&x), "{x}");
        }
        assert!(sweep(0.0) < 0.01);
        assert!((sweep(SWEEP_PERIOD / 2.0) - (1.0 - SWEEP_W)).abs() < 0.01);
    }
}
