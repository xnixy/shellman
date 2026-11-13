//! Taking the shown tab's snapshot when something changed, the cursor's blink, and zoom.

use crate::config;
use crate::palette::Palette;
use crate::session::Colours;
use crate::snapshot::{self, Hover, Look};
use shellman_ui::theme::colors;
use std::time::{Duration, Instant};

/// A blinking cursor stops (and stays on) after this long without input or output, so an idle
/// window draws nothing, as Windows' own caret does.
const BLINK_FOR: Duration = Duration::from_secs(15);

/// What the last snapshot was taken with: when any of it changes, it is taken again.
#[derive(Clone, PartialEq)]
struct Drawn {
    light: bool,
    accent: egui::Color32,
    focused: bool,
    tab: u64,
    cursor_on: bool,
    hover: Hover,
}

pub struct View {
    drawn: Option<Drawn>,
    /// When the blink last started over (input, output, focus).
    blink_epoch: Instant,
    blink: Duration,
    /// The hovered link, drawn underlined.
    pub hover: Hover,
}

impl View {
    pub fn new() -> Self {
        Self { drawn: None, blink_epoch: Instant::now(), blink: caret_blink(), hover: Hover::None }
    }

    /// The cursor shows at once and blinks from now.
    pub fn restart_blink(&mut self) {
        self.blink_epoch = Instant::now();
    }

    /// The blink's phase now, and when it next changes (None when it does not blink now).
    fn blink_phase(&self, blinking: bool) -> (bool, Option<Duration>) {
        let since = self.blink_epoch.elapsed();
        if !blinking || since >= BLINK_FOR {
            return (true, None);
        }
        // Timers wake a little early (Windows' tick is 15.6 ms): a frame that close to the
        // change counts as after it, or each early frame would ask for another one at once.
        let slack = Duration::from_millis(20);
        let n = (since + slack).as_millis() / self.blink.as_millis().max(1);
        let next = self.blink * (n as u32 + 1) - since;
        (n % 2 == 0, Some(next.min(BLINK_FOR.saturating_sub(since)).max(slack)))
    }
}

/// The grid font's size in pixels for `pt` points at `ppp` pixels per point.
pub fn font_px(pt: f32, ppp: f32) -> f32 {
    (pt * 96.0 / 72.0 * ppp).round().max(4.0)
}

/// Windows' caret blink time (530 ms unless changed).
fn caret_blink() -> Duration {
    let ms = unsafe { windows::Win32::UI::WindowsAndMessaging::GetCaretBlinkTime() };
    // INFINITE (no blinking) and nonsense values: the default.
    Duration::from_millis(if (100..=5000).contains(&ms) { ms as u64 } else { 530 })
}

impl super::TerminalApp {
    /// Take a new snapshot and send what changed, if the grid, its size, the look, the blink or
    /// the hover changed. Returns how many rows went to the GPU.
    pub(super) fn redraw_if_changed(&mut self, ctx: &egui::Context) -> usize {
        let c = colors();
        let focused = self.focused;
        let tab = self.tabs.current();
        let dirty = tab.session.take_dirty();
        if dirty {
            // New output (or a selection): the cursor shows and blinks from now.
            self.view.restart_blink();
        }
        let (cursor_on, next) = self.view.blink_phase(self.frame.blinking);
        if let Some(next) = next {
            ctx.request_repaint_after(next);
        }
        let with = Drawn { light: c.light, accent: c.accent, focused, tab: tab.id, cursor_on, hover: self.view.hover.clone() };
        if !dirty && self.view.drawn.as_ref() == Some(&with) && self.frame.cols != 0 {
            return 0;
        }
        let rgb = |c: egui::Color32| [c.r(), c.g(), c.b()];
        let palette = Palette::new(c.light, rgb(c.text), rgb(c.bg));
        let look = Look {
            palette,
            cursor: rgb(c.accent_fill),
            on_cursor: rgb(c.on_accent),
            focused,
            selection: rgb(c.accent_fill),
            cursor_on,
            hover: self.view.hover.clone(),
        };
        if self.view.drawn.as_ref().is_none_or(|d| d.light != with.light || d.accent != with.accent || d.tab != with.tab) {
            tab.session.set_colours(Colours { palette, cursor: look.cursor });
        }
        self.view.drawn = Some(with);
        let was_blinking = self.frame.blinking;
        tab.session.with_term(|term| snapshot::take(term, &look, &mut self.frame));
        if self.frame.blinking && !was_blinking {
            // It just started to blink: the timer starts now.
            if let (_, Some(next)) = self.view.blink_phase(true) {
                ctx.request_repaint_after(next);
            }
        }
        self.renderer.prepare(&self.frame)
    }

    /// Zoom by `steps` points (Ctrl and the wheel, Ctrl+plus/minus); None goes back to the
    /// configured size.
    pub(super) fn zoom(&mut self, steps: Option<f32>) {
        let pt = match steps {
            Some(s) => (self.font_pt + s).clamp(config::MIN_PT, config::MAX_PT),
            None => self.config.font_size,
        };
        if pt != self.font_pt {
            self.font_pt = pt;
            self.tabs.current().session.mark_dirty();
        }
    }
}
