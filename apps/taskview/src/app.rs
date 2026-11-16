//! Task View's life: open with every app window on show, keep up while windows open, close or
//! change, and go away by switching, Esc, a click beside the tiles, Win+Tab again, or losing the
//! foreground.
//!
//! While out of sight it holds no previews and no window watcher, and draws nothing.

use crate::layout::{self, Dir, Layout};
use crate::summon::{self, Shared};
use crate::view::{self, Action};
use crate::select;
use shellman_core::live_thumb::{fit, LiveThumb, PxRect};
use shellman_core::thumbs::Thumbs;
use shellman_core::winlist::{AppWindow, WindowList, WindowWatcher};
use shellman_ui::theme;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    IsIconic, IsWindow, PostMessageW, SetForegroundWindow, ShowWindowAsync, SW_RESTORE, WM_CLOSE,
};

/// While showing, how often to look for listed windows that are gone.
const GONE_CHECK: Duration = Duration::from_secs(1);
/// After showing, how long to wait for the foreground before "lost focus" means anything.
const FOCUS_GRACE: Duration = Duration::from_millis(400);

pub struct TaskView {
    hwnd: isize,
    shared: Arc<Shared>,
    list: WindowList,
    icons: Thumbs,
    seen_shown: u32,
    open: Option<Open>,
}

struct Open {
    windows: Vec<AppWindow>,
    apps: Vec<String>,
    /// One per window, in the same order; None where the compositor won't preview it.
    previews: Vec<Option<LiveThumb>>,
    watcher: WindowWatcher,
    sel: usize,
    /// The selection moved by key: give its tile the keyboard focus (and the screen reader).
    focus: bool,
    more: usize,
    /// The screen size in points the list was cut for (`fill`).
    filled_for: egui::Vec2,
    /// The screen size in points the layout was made for.
    laid_for: Option<egui::Vec2>,
    /// What the previews were last placed for: the screen size in points, pixels per point and
    /// the window's real client size in pixels.
    placed_for: Option<(egui::Vec2, f32, (i32, i32))>,
    layout: Layout,
    front: Option<isize>,
    since: Instant,
    had_focus: bool,
}

impl Open {
    /// A listed window no longer exists.
    fn gone(&self) -> bool {
        self.windows.iter().any(|w| !unsafe { IsWindow(Some(HWND(w.hwnd as *mut _))) }.as_bool())
    }
}

impl TaskView {
    pub fn new(ctx: egui::Context, hwnd: isize, shared: Arc<Shared>) -> Self {
        Self { icons: Thumbs::new(ctx), hwnd, shared, list: WindowList::default(), seen_shown: 0, open: None }
    }

    pub fn frame(&mut self, ctx: &egui::Context) {
        self.icons.poll(ctx);
        let shown = self.shared.shown_count();
        if shown != self.seen_shown {
            self.seen_shown = shown;
            self.start(ctx);
        }
        if self.shared.take_hide_request() {
            self.close(false);
        }
        // After a `--background` start the window library shows the window once by itself.
        if !self.shared.is_showing() && summon::is_visible(self.hwnd) {
            summon::hide(self.hwnd);
        }
        if self.open.is_none() {
            egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |_| {});
            return;
        }
        // The list changed, or the screen did (the window library caught up with a move to
        // another monitor): read the windows again, and cut them to what this screen holds.
        let size = ctx.screen_rect().size();
        if self.open.as_ref().is_some_and(|o| o.watcher.take_dirty() || o.filled_for != size || o.gone()) {
            self.refresh(ctx);
        }
        // Windows of a process that was ended are destroyed without the event the watcher hears;
        // look once a second while showing.
        ctx.request_repaint_after(GONE_CHECK);
        if self.lost_focus(ctx) {
            self.close(false);
            return;
        }
        self.keys(ctx);
        let Some(open) = &mut self.open else { return };
        let screen = ctx.screen_rect();
        if open.laid_for != Some(screen.size()) {
            relayout(open, screen.size());
        }
        // Moved onto a monitor of another scale, the window library catches up with the window's
        // real size a frame or two late; previews placed meanwhile land in the wrong spots.
        let ppp = ctx.pixels_per_point();
        let client = client_size(self.hwnd);
        if (client.0 as f32 - screen.width() * ppp).abs() > 2.0 || (client.1 as f32 - screen.height() * ppp).abs() > 2.0 {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if open.placed_for != Some((screen.size(), ppp, client)) {
            place(open, ppp);
            open.placed_for = Some((screen.size(), ppp, client));
        }
        // A key moved the selection: the focus (and a screen reader) goes with it. Asked before
        // the tiles are drawn, since egui drops the focus of a widget not drawn in its frame.
        if std::mem::take(&mut open.focus) {
            if let Some(w) = open.windows.get(open.sel) {
                ctx.memory_mut(|m| m.request_focus(view::tile_id(w.hwnd)));
            }
        }
        let live: Vec<bool> = open.previews.iter().zip(&open.windows).map(|(p, w)| p.is_some() && !w.minimized).collect();
        let mut action = None;
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
            // Over the acrylic, the same tint as every shellman window; without it (high contrast,
            // no transparency, Remote Desktop), a solid page.
            let c = theme::colors();
            if crate::GLASS.load(std::sync::atomic::Ordering::Relaxed) {
                ui.painter().rect_filled(screen, 0.0, c.shell);
                if c.light {
                    // White cards need a page a shade darker to stand on.
                    ui.painter().rect_filled(screen, 0.0, egui::Color32::from_black_alpha(22));
                }
            } else {
                ui.painter().rect_filled(screen, 0.0, theme::with_alpha(c.bg, 255));
            }
            let v = view::View {
                windows: &open.windows,
                apps: &open.apps,
                layout: &open.layout,
                sel: open.sel,
                live: &live,
                more: open.more,
            };
            action = view::draw(ui, &v, &mut self.icons);
        });
        // Follow the keyboard focus: Tab moves it between tiles and close buttons.
        if let Some(focused) = ctx.memory(|m| m.focused()) {
            if let Some(i) = open.windows.iter().position(|w| view::tile_id(w.hwnd) == focused || view::close_id(w.hwnd) == focused) {
                open.sel = i;
            }
        }
        match action {
            Some(Action::Pick(i)) => self.pick(i),
            Some(Action::Close(i)) => self.close_window(i),
            Some(Action::Dismiss) => self.close(true),
            None => {}
        }
    }

    /// A new showing: list the windows, preview them, select the one that was in front.
    fn start(&mut self, ctx: &egui::Context) {
        self.open = None;
        let front = self.shared.front();
        let windows = self.list.refresh();
        let ids: Vec<isize> = windows.iter().map(|w| w.hwnd).collect();
        let wake = ctx.clone();
        let mut open = Open {
            apps: Vec::new(),
            previews: Vec::new(),
            watcher: WindowWatcher::start(move || wake.request_repaint()),
            sel: select::first(&ids, front),
            focus: true,
            more: 0,
            filled_for: egui::Vec2::ZERO,
            laid_for: None,
            placed_for: None,
            layout: layout::arrange(0, ctx.screen_rect().size()),
            windows,
            front,
            since: Instant::now(),
            had_focus: false,
        };
        // The watcher's first report is the list just read.
        open.watcher.take_dirty();
        self.fill(&mut open, ctx);
        self.open = Some(open);
        ctx.request_repaint_after(FOCUS_GRACE + Duration::from_millis(10));
    }

    /// The window list changed while showing.
    fn refresh(&mut self, ctx: &egui::Context) {
        let Some(mut open) = self.open.take() else { return };
        let old: Vec<isize> = open.windows.iter().map(|w| w.hwnd).collect();
        open.windows = self.list.refresh();
        let new: Vec<isize> = open.windows.iter().map(|w| w.hwnd).collect();
        open.sel = select::keep(&old, open.sel, &new);
        // Keep the focus ring where it was when the focused window went away.
        open.focus |= !new.contains(old.get(open.sel).unwrap_or(&0));
        self.fill(&mut open, ctx);
        self.open = Some(open);
    }

    /// Cut the list to what fits, and preview the windows (keeping the previews already made).
    fn fill(&self, open: &mut Open, ctx: &egui::Context) {
        open.filled_for = ctx.screen_rect().size();
        let cap = layout::capacity(open.filled_for);
        open.more = open.windows.len().saturating_sub(cap);
        open.windows.truncate(cap);
        open.sel = open.sel.min(open.windows.len().saturating_sub(1));
        let wake = ctx.clone();
        open.apps = open
            .windows
            .iter()
            .map(|w| {
                let wake = wake.clone();
                shellman_core::system::app_name::display_name_soon(&w.app_id, w.exe.as_deref(), move || wake.request_repaint())
            })
            .collect();
        let mut kept: Vec<LiveThumb> = open.previews.drain(..).flatten().collect();
        open.previews = open
            .windows
            .iter()
            .map(|w| match kept.iter().position(|p| p.source() == w.hwnd) {
                Some(i) => Some(kept.swap_remove(i)),
                None => LiveThumb::register(self.hwnd, w.hwnd),
            })
            .collect();
        open.laid_for = None;
        open.placed_for = None;
    }

    fn keys(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        let Some(open) = &mut self.open else { return };
        let press = |key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
        if press(Key::Escape) {
            return self.close(true);
        }
        for (key, dir) in [(Key::ArrowLeft, Dir::Left), (Key::ArrowRight, Dir::Right), (Key::ArrowUp, Dir::Up), (Key::ArrowDown, Dir::Down)] {
            if press(key) {
                open.sel = layout::neighbour(&open.layout.tiles, open.sel, dir);
                open.focus = true;
            }
        }
        if press(Key::Delete) && !open.windows.is_empty() {
            let i = open.sel;
            return self.close_window(i);
        }
        // Enter on a focused widget is that widget's click; with nothing focused, the selection.
        if ctx.memory(|m| m.focused()).is_none() && press(Key::Enter) && !open.windows.is_empty() {
            let i = open.sel;
            self.pick(i);
        }
    }

    /// Lost the foreground after having had it: the user went elsewhere.
    fn lost_focus(&mut self, ctx: &egui::Context) -> bool {
        let Some(open) = &mut self.open else { return false };
        let focused = ctx.input(|i| i.focused);
        if focused {
            open.had_focus = true;
            return false;
        }
        open.had_focus || open.since.elapsed() > FOCUS_GRACE * 4
    }

    /// Switch to window `i`: Task View has the foreground, so Windows lets it hand it on.
    fn pick(&mut self, i: usize) {
        let Some(hwnd) = self.open.as_ref().and_then(|o| o.windows.get(i)).map(|w| w.hwnd) else { return };
        bring_to_front(hwnd);
        self.close(false);
    }

    /// Ask window `i` to close, as its own close button would. Its tile goes when the watcher
    /// sees it gone (a window that asks "Save changes?" stays).
    fn close_window(&mut self, i: usize) {
        let Some(open) = &mut self.open else { return };
        if let Some(w) = open.windows.get(i) {
            // Posted, never sent: a hung app can't hang Task View.
            unsafe {
                let _ = PostMessageW(Some(HWND(w.hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            open.focus = true;
        }
    }

    /// Out of sight, letting go of the previews. `back`: give the foreground back to the window
    /// that was in front (Esc), rather than leave it to Windows.
    fn close(&mut self, back: bool) {
        let front = self.open.take().and_then(|o| o.front);
        if back {
            if let Some(hwnd) = front {
                unsafe {
                    let _ = SetForegroundWindow(HWND(hwnd as *mut _));
                }
            }
        }
        self.shared.mark_hidden();
        summon::hide(self.hwnd);
        self.icons.cancel_pending();
    }
}

fn relayout(open: &mut Open, screen: egui::Vec2) {
    open.layout = layout::arrange(open.windows.len(), screen);
    open.sel = open.sel.min(open.windows.len().saturating_sub(1));
    open.laid_for = Some(screen);
    open.placed_for = None;
}

/// Put each preview over its tile's frame, at the window's own shape.
fn place(open: &Open, ppp: f32) {
    for ((preview, tile), window) in open.previews.iter().zip(&open.layout.tiles).zip(&open.windows) {
        let Some(preview) = preview else { continue };
        let r = tile.preview;
        let area = PxRect { x: r.left() * ppp, y: r.top() * ppp, w: r.width() * ppp, h: r.height() * ppp };
        match preview.source_size() {
            Some(size) if !window.minimized => preview.show(fit(size, area)),
            _ => preview.hide(),
        }
    }
}

fn bring_to_front(hwnd: isize) {
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return;
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindowAsync(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

fn client_size(hwnd: isize) -> (i32, i32) {
    let mut r = windows::Win32::Foundation::RECT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(HWND(hwnd as *mut _), &mut r);
    }
    (r.right, r.bottom)
}
