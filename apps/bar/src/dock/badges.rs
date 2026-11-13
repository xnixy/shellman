//! Each icon's status for a frame (see `status` and `tasks`): read what the apps' windows said,
//! upload new badge pictures once, and keep the frames coming only while something animates.

use super::items::Item;
use super::status::{self, Shown};
use super::{tasks, Dock};
use shellman_core::taskbar_list::Progress;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How often an indeterminate sweep is redrawn: quiet, not every display frame. A dock frame
/// costs about 14 ms of CPU here (measured on a 3840 px wide dock window), so the sweep steps at
/// about 6 fps along a slow path, and only on the dock of the window's own monitor (measured
/// first at 25 fps on three docks: about 60 % of a core).
const SWEEP_FRAME: Duration = Duration::from_millis(160);

/// The attention pulse's frame pace. Not "as soon as possible": a viewport repainting without
/// pause starved the other docks' repaints (measured: they ran only when its 1.8 s of pulses
/// were over), so it leaves the event loop room between frames.
const PULSE_FRAME: Duration = Duration::from_millis(33);

/// Whether two windows are on the same monitor.
fn same_monitor(a: isize, b: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};
    unsafe {
        MonitorFromWindow(HWND(a as *mut _), MONITOR_DEFAULTTONEAREST)
            == MonitorFromWindow(HWND(b as *mut _), MONITOR_DEFAULTTONEAREST)
    }
}

/// Badge pictures uploaded as textures, by window, with the counter they were made at.
#[derive(Default)]
pub struct Textures(HashMap<isize, (u64, egui::TextureHandle)>);

impl Dock {
    /// The status of each item, and frames asked for while one animates.
    pub(super) fn statuses(&mut self, ctx: &egui::Context, items: &[Item], dock: isize) -> Vec<Shown> {
        let known = tasks::snapshot();        let now = Instant::now();
        let mut animating: Option<Duration> = None;
        let mut used = Vec::new();
        let shown = items
            .iter()
            .map(|item| {
                let s = status::of(&item.windows, &known);
                let badge = s.badge.map(|(hwnd, rev, img)| {
                    used.push(hwnd);
                    let fresh = self.badge_tex.0.get(&hwnd).is_none_or(|(r, _)| *r != rev);
                    if fresh {
                        let tex = ctx.load_texture(format!("dock badge {hwnd:x}"), (*img).clone(), egui::TextureOptions::LINEAR);
                        self.badge_tex.0.insert(hwnd, (rev, tex));
                    }
                    self.badge_tex.0[&hwnd].1.id()
                });
                if let Some(t) = s.attention {
                    if !status::attention(now.saturating_duration_since(t)).1 {
                        animating = Some(PULSE_FRAME);
                    }
                }
                let sweeping = matches!(s.progress, Some((Progress::Indeterminate, _)))
                    && s.progress_window.is_some_and(|w| same_monitor(w, dock));
                if sweeping {
                    animating = Some(animating.map_or(SWEEP_FRAME, |a| a.min(SWEEP_FRAME)));
                }
                Shown { progress: s.progress, sweeping, badge, attention: s.attention }
            })
            .collect();
        self.badge_tex.0.retain(|h, _| used.contains(h));
        if let Some(after) = animating {
            ctx.request_repaint_after(after);
        }
        shown
    }
}
