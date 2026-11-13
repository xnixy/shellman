//! The switcher's life: open on Alt+Tab with the window list in most-recently-used order, move
//! the selection, and close by switching or cancelling.
//!
//! A quick Alt+Tab never shows the panel: it only appears once Alt has been held a moment, or on
//! the second Tab. While hidden it holds no previews and no window watcher, and draws nothing
//! unless a key or the look changes.

use crate::keys::{self, Key};
use crate::layout::{self, Layout};
use crate::shape::{self, Screen};
use crate::{activate, panel, select};
use shellman_core::live_thumb::{fit, LiveThumb, PxRect};
use shellman_core::thumbs::Thumbs;
use shellman_core::winlist::{self, AppWindow, WindowList, WindowWatcher};
use shellman_ui::theme;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_MENU};

/// How long Alt+Tab is held before the panel shows. A tap is quicker, so it never flashes.
const DELAY: Duration = Duration::from_millis(120);
/// While open, how often to check that Alt is still held, in case its release was never seen
/// (the secure desktop took the keyboard, say).
const ALT_CHECK: Duration = Duration::from_millis(500);

pub struct Switcher {
    ctx: egui::Context,
    hwnd: isize,
    glass: bool,
    keys: Receiver<Key>,
    list: WindowList,
    icons: Thumbs,
    open: Option<Open>,
    /// A second frame was asked for, to hide the window the library shows after the first.
    primed: bool,
}

struct Open {
    windows: Vec<AppWindow>,
    sel: usize,
    since: Instant,
    screen: Screen,
    layout: Layout,
    shown: Option<Shown>,
}

/// What exists only while the panel is on screen.
struct Shown {
    /// One per window, in the same order; None where the window has no preview (minimized, or
    /// one the compositor won't preview), which then shows its icon.
    previews: Vec<Option<LiveThumb>>,
    watcher: WindowWatcher,
}

impl Switcher {
    pub fn new(ctx: egui::Context, hwnd: isize, glass: bool, keys: Receiver<Key>) -> Self {
        Self {
            icons: Thumbs::new(ctx.clone()),
            ctx,
            hwnd,
            glass,
            keys,
            list: WindowList::default(),
            open: None,
            primed: false,
        }
    }

    pub fn frame(&mut self, ctx: &egui::Context) {
        theme::set_light(ctx.theme() == egui::Theme::Light);
        if !std::mem::replace(&mut self.primed, true) {
            ctx.request_repaint();
        }
        self.icons.poll(ctx);
        while let Ok(key) = self.keys.try_recv() {
            self.on_key(key);
        }
        if let Some(open) = &self.open {
            match &open.shown {
                None => match DELAY.checked_sub(open.since.elapsed()) {
                    Some(left) if !left.is_zero() => ctx.request_repaint_after(left),
                    _ => self.show(),
                },
                Some(shown) => {
                    if !alt_held() {
                        // The hook hands over Alt's release before Windows records it, so a
                        // release it saw is in the queue by now.
                        while let Ok(key) = self.keys.try_recv() {
                            self.on_key(key);
                        }
                        if self.open.is_some() {
                            keys::close();
                            self.close();
                        }
                    } else if shown.watcher.take_dirty() {
                        self.refresh();
                    }
                    ctx.request_repaint_after(ALT_CHECK);
                }
            }
        }
        // Asleep whenever closed, including after the library's own first show.
        if self.open.is_none() && shape::is_on(self.hwnd) {
            shape::hide(self.hwnd);
        }
        let Some(open) = &self.open else { return };
        // Drawn while still hidden too, so icons are loaded and the first frame on screen is
        // already complete.
        let live: Vec<bool> = match &open.shown {
            Some(s) => s.previews.iter().zip(&open.windows).map(|(p, w)| p.is_some() && !w.minimized).collect(),
            None => vec![false; open.windows.len()],
        };
        let view = panel::View {
            windows: &open.windows,
            layout: &open.layout,
            sel: open.sel,
            live: &live,
            glass: self.glass,
            scale: open.screen.scale,
        };
        if open.shown.is_some() {
            shellman_ui::timing::content(ctx, "switcher", OPENED.load(Ordering::Relaxed));
        }
        if let Some(i) = panel::draw(ctx, &view, &mut self.icons) {
            let hwnd = open.windows[i].hwnd;
            keys::close();
            self.close();
            activate::bring_to_front(hwnd);
        }
    }

    fn on_key(&mut self, key: Key) {
        let Some(open) = &mut self.open else {
            if matches!(key, Key::Next | Key::Prev) {
                self.start(key == Key::Prev);
            }
            return;
        };
        let n = open.windows.len();
        match key {
            Key::Next | Key::Prev => open.sel = select::step(open.sel, n, key == Key::Prev),
            Key::Arrow(dir) => open.sel = select::arrow(open.sel, n, open.layout.cols, dir),
            Key::Commit => {
                let target = open.windows.get(open.sel).map(|w| w.hwnd);
                self.close();
                if let Some(hwnd) = target {
                    activate::bring_to_front(hwnd);
                }
                return;
            }
            Key::Cancel => return self.close(),
        }
        // A second key means the user is choosing: show the panel now.
        if self.open.as_ref().is_some_and(|o| o.shown.is_none()) {
            self.show();
        }
    }

    /// Alt+Tab: list the windows, in front first, and select the one to switch to.
    fn start(&mut self, back: bool) {
        shellman_core::timing::mark("open started (UI thread)");
        let front = winlist::foreground();
        let screen = Screen::of(front.unwrap_or(0));
        let mut windows = self.list.refresh();
        windows.truncate(layout::capacity(screen.size_points()));
        if windows.is_empty() {
            // The hook marked the switcher open with Alt+Tab; nothing will show, so give Esc,
            // Enter and the arrows back to the apps now rather than when Alt is released.
            keys::close();
            return;
        }
        OPENED.fetch_add(1, Ordering::Relaxed);
        let front_is_first = front == Some(windows[0].hwnd);
        self.open = Some(Open {
            sel: select::first(windows.len(), front_is_first, back),
            layout: layout::arrange(windows.len(), screen.size_points()),
            windows,
            since: Instant::now(),
            screen,
            shown: None,
        });
        // Out of sight it was one pixel (`shape::hide`): the frames drawn before it shows need
        // its full size.
        if let Some(open) = &self.open {
            shape::presize(self.hwnd, panel_rect(open));
        }
        self.ctx.request_repaint();
    }

    fn show(&mut self) {
        let Some(open) = &mut self.open else { return };
        let previews = open.windows.iter().map(|w| LiveThumb::register(self.hwnd, w.hwnd)).collect();
        let wake = self.ctx.clone();
        open.shown = Some(Shown {
            previews,
            watcher: WindowWatcher::start(move || wake.request_repaint()),
        });
        // The watcher's first report is the list just read.
        if let Some(s) = &open.shown {
            s.watcher.take_dirty();
        }
        self.place();
    }

    /// The list changed while the panel is up: a window opened, closed or was minimized.
    fn refresh(&mut self) {
        let Some(open) = &mut self.open else { return };
        let mut windows = self.list.refresh();
        windows.truncate(layout::capacity(open.screen.size_points()));
        if windows.is_empty() {
            keys::close();
            return self.close();
        }
        let old: Vec<isize> = open.windows.iter().map(|w| w.hwnd).collect();
        let new: Vec<isize> = windows.iter().map(|w| w.hwnd).collect();
        open.sel = select::keep(&old, open.sel, &new);
        open.layout = layout::arrange(windows.len(), open.screen.size_points());
        if let Some(shown) = &mut open.shown {
            // Keep the previews of windows still listed; register the new ones.
            let mut kept: Vec<LiveThumb> = shown.previews.drain(..).flatten().collect();
            shown.previews = new
                .iter()
                .map(|&h| match kept.iter().position(|p| p.source() == h) {
                    Some(i) => Some(kept.swap_remove(i)),
                    None => LiveThumb::register(self.hwnd, h),
                })
                .collect();
        }
        open.windows = windows;
        self.place();
    }

    /// Size and centre the panel on its monitor, put each preview over its tile, and show it.
    fn place(&mut self) {
        let Some(open) = &self.open else { return };
        let Some(shown) = &open.shown else { return };
        let s = open.screen.scale;
        for ((preview, tile), window) in shown.previews.iter().zip(&open.layout.tiles).zip(&open.windows) {
            let Some(preview) = preview else { continue };
            let area = PxRect { x: tile.preview.x * s, y: tile.preview.y * s, w: tile.preview.w * s, h: tile.preview.h * s };
            match preview.source_size() {
                Some(size) if !window.minimized => preview.show(fit(size, area)),
                _ => preview.hide(),
            }
        }
        shape::show_at(self.hwnd, panel_rect(open));
        shellman_core::timing::mark("window shown");
        self.ctx.request_repaint();
    }

    /// Hide the panel and let go of its previews.
    fn close(&mut self) {
        shape::hide(self.hwnd);
        self.open = None;
        // One frame with nothing drawn, so the next showing never starts from this panel's image.
        self.ctx.request_repaint();
    }
}

/// The panel's rectangle in pixels: its layout's size, centred on its monitor.
fn panel_rect(open: &Open) -> windows::Win32::Foundation::RECT {
    let s = open.screen.scale;
    let size = ((open.layout.w * s).round() as i32, (open.layout.h * s).round() as i32);
    open.screen.centred(size)
}

/// Counts openings, for the opening-speed milestones (`SHELLMAN_TIMING=1`).
static OPENED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Set in a test copy, which has no keyboard: Alt counts as held until Esc.
pub static DEMO: AtomicBool = AtomicBool::new(false);

fn alt_held() -> bool {
    DEMO.load(Ordering::Relaxed) || (unsafe { GetAsyncKeyState(VK_MENU.0 as i32) }) < 0
}
