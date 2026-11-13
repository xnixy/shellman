//! Notifications: the toasts apps send, shown as islands hanging under the top bar's clock
//! island on the primary monitor, newest nearest the clock. Each slides out from under the clock,
//! stays a few seconds (held while the pointer is over it) and slides back; the clock island
//! counts the unseen ones, and a click on it opens the list under it: every notification,
//! grouped by app, with do-not-disturb and "Clear all".
//!
//! The toasts come from the platform's own store (`shellman_core::notify`), read again on a
//! thread whenever the platform logs a change; nothing polls. The window is one egui viewport
//! that exists only while something shows (as the preview panel does): a column as tall as the
//! work area, transparent and click-through except on its islands, never activated (it does not
//! take focus, except when the user clicks into a reply box) and reserving no space.
//!
//! When nothing shows any more, the window is hidden at once but its viewport is kept for a
//! moment (`LINGER`) before it goes: a repaint still pending for a viewport nobody declares (a
//! text cursor's blink, a last animation frame) would make egui open a blank window and spin.
//!
//! Pieces: `feed` (what shows, and when), `layout` (geometry), `draw` (painting), `act` (doing
//! what was clicked, off the UI thread), `window` (Win32).

mod act;
mod draw;
mod feed;
pub(crate) mod layout;
mod window;

use crate::shape::surface;
use crate::{host, place, shape};
use draw::{Card, Event, View};
use egui::{vec2, Rect, ViewportId};
use feed::Feed;
use layout::{GAP, GROUP_GAP, PER_APP, UNDER};
use shellman_core::notify::platform::Platform;
use shellman_core::notify::watch::Watch;
use shellman_core::notify::{Invoke, Source, Toast};
use shellman_core::settings::bar::BarSettings;
use shellman_ui::glass::{self, Style};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use windows::Win32::Foundation::RECT;

const TITLE: &str = "shellman notifications";
/// Frames drawn before the window is uncloaked, as the preview panel does.
const WARM_FRAMES: u8 = 2;
/// How long the hidden window's viewport is kept after the last island went.
const LINGER: std::time::Duration = std::time::Duration::from_secs(1);

pub use draw::Icons;

pub fn id() -> ViewportId {
    ViewportId::from_hash_of("shellman notifications")
}

/// The islands' glass: the dock's material at the toasts' radius.
pub fn style() -> Style {
    Style::from_theme(layout::RADIUS)
}

/// The window as the host declares it: hidden, far away and tiny until placed.
pub fn builder() -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title(TITLE)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_visible(false)
        .with_position([-30000.0, -30000.0])
        .with_inner_size([8.0, 8.0])
}

/// What the top bar's badge shows.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Badge {
    pub unseen: usize,
    pub any: bool,
    pub dnd: bool,
    pub open: bool,
}

type Incoming = Arc<Mutex<Option<Result<Vec<Toast>, String>>>>;

pub struct Notify {
    ctx: egui::Context,
    source: Arc<dyn Source>,
    incoming: Incoming,
    _watch: Watch,
    feed: Feed,
    /// The window, once the host has declared it and it exists.
    hwnd: Option<isize>,
    warm: Option<u8>,
    /// Text typed into reply boxes, by toast.
    replies: HashMap<u32, String>,
    /// The app in front when the list opened: the list closes when another comes forward.
    front_at_open: Option<isize>,
    /// The last read failed (shown once on stderr, not on screen).
    failed: bool,
    /// The primary top bar's clock island: its centre and lower edge, in screen pixels.
    anchor: Option<(i32, i32)>,
    /// The clock island the open list was opened from, when not the primary's.
    list_anchor: Option<(i32, i32)>,
    /// Where the window was put.
    placed: Option<[i32; 4]>,
    /// Since when nothing has shown (the window is hidden, its viewport lingering).
    idle_since: Option<Instant>,
}

impl Notify {
    pub fn new(ctx: egui::Context, dnd: bool) -> Self {
        let source: Arc<dyn Source> = Arc::new(Platform::new());
        let incoming: Incoming = Arc::default();
        let inbox = incoming.clone();
        let watch = Watch::start(
            source.clone(),
            Box::new(move |read| {
                *inbox.lock().unwrap() = Some(read);
                host::request_frame();
            }),
        );
        Self {
            ctx,
            source,
            incoming,
            _watch: watch,
            feed: Feed::with_dnd(dnd),
            hwnd: None,
            warm: None,
            replies: HashMap::new(),
            front_at_open: None,
            failed: false,
            anchor: None,
            list_anchor: None,
            placed: None,
            idle_since: None,
        }
    }

    pub fn badge(&self) -> Badge {
        Badge {
            unseen: self.feed.unseen(),
            any: !self.feed.toasts().is_empty(),
            dnd: self.feed.dnd,
            open: self.feed.list_open(),
        }
    }

    /// The primary top bar's clock island is at `centre_x`, its lower edge at `bottom` (screen
    /// pixels): the window hangs from there.
    pub fn set_anchor(&mut self, centre_x: i32, bottom: i32) {
        self.anchor = Some((centre_x, bottom));
    }

    /// Where the window goes now: under the clock of the bar the list was opened from (any
    /// monitor's), else under the primary bar's, or at the top centre of the primary's work area
    /// until a top bar has been drawn. On that clock's own monitor, with its scale and work area.
    /// None when no usable window fits there.
    fn wanted(&self) -> Option<[i32; 4]> {
        let areas = window::areas();
        let anchor = if self.feed.list_open() { self.list_anchor.or(self.anchor) } else { self.anchor };
        let anchor = anchor.or_else(|| areas.first().map(|a| ((a.work[0] + a.work[2]) / 2, a.work[1])))?;
        let area = layout::area_of(anchor, &areas)?;
        layout::window_for(anchor, &area)
    }

    fn place(&mut self, hwnd: isize) {
        let Some(wanted) = self.wanted() else { return };
        if self.placed != Some(wanted) {
            let [left, top, right, bottom] = wanted;
            place::put(hwnd, RECT { left, top, right, bottom });
            self.placed = Some(wanted);
        }
    }

    /// The badge was clicked, with `front` the app window in front: open or close the list.
    /// `from` is the clicked bar's clock island (`layout::clock_anchor`): the list opens under
    /// that one, on its monitor.
    pub fn toggle_list(&mut self, front: Option<isize>, from: Option<(i32, i32)>) {
        if self.feed.list_open() {
            self.feed.close_list();
        } else {
            self.list_anchor = from;
            self.feed.open_list();
            self.front_at_open = front;
        }
        self.changed();
    }

    /// Open the list (if it is not open) under the clock `from`, showing `day` in the calendar.
    pub fn open_on_day(&mut self, _day: i64, front: Option<isize>, from: Option<(i32, i32)>) {
        if !self.feed.list_open() {
            self.toggle_list(front, from);
        }
    }

    /// Whether the window exists (it can be repainted).
    pub fn window_exists(&self) -> bool {
        self.hwnd.is_some()
    }

    /// The app in front is now `front`: an open list closes when another app comes forward.
    pub fn front_is(&mut self, front: Option<isize>) {
        if self.feed.list_open() && front.is_some() && front != self.front_at_open {
            self.feed.close_list();
            self.changed();
        }
    }

    /// In the host's frame: take a fresh read, let the timers run, and say whether the window
    /// should exist. `covered` says a fullscreen app is in front on the primary monitor.
    pub fn tick(&mut self, now: Instant, dnd: bool, covered: bool) -> bool {
        self.feed.dnd = dnd;
        let read = self.incoming.lock().unwrap().take();
        match read {
            Some(Ok(list)) => {
                self.failed = false;
                if self.feed.update(list, now, covered) {
                    self.replies.retain(|id, _| self.feed.toast(*id).is_some());
                    self.repaint();
                }
            }
            Some(Err(e)) if !self.failed => {
                self.failed = true;
                eprintln!("notifications: {e}");
            }
            _ => {}
        }
        if self.feed.tick(now) {
            self.repaint();
        }
        if let Some(due) = self.feed.due(now) {
            self.ctx.request_repaint_after_for(due, ViewportId::ROOT);
        }
        if self.feed.visible() {
            self.idle_since = None;
            if let Some(hwnd) = self.hwnd {
                self.place(hwnd);
                window::show(hwnd);
            }
            return true;
        }
        let since = *self.idle_since.get_or_insert(now);
        if let Some(hwnd) = self.hwnd {
            // Out of sight and out of the way at once; the viewport goes after `LINGER`.
            window::hide(hwnd);
            surface::set_islands(hwnd, &[], 1.0, true, None);
        }
        if self.hwnd.is_some() && now < since + LINGER {
            self.ctx.request_repaint_after_for(since + LINGER - now, ViewportId::ROOT);
            return true;
        }
        self.release();
        false
    }

    fn repaint(&self) {
        if self.hwnd.is_some() {
            self.ctx.request_repaint_of(id());
        }
    }

    /// Something changed what shows: the host decides whether the window exists.
    fn changed(&self) {
        host::request_frame();
        self.repaint();
    }

    fn release(&mut self) {
        if let Some(hwnd) = self.hwnd.take() {
            surface::detach(hwnd);
            window::hide(hwnd);
        }
        self.warm = None;
        self.placed = None;
        self.idle_since = None;
    }

    /// In the host's frame, after it declared the window: find it once it exists, give it its
    /// looks and its place. False while it is still to come.
    pub fn adopt(&mut self) -> bool {
        if !self.feed.visible() || self.hwnd.is_some() {
            return true;
        }
        let Some(hwnd) = shape::own_window(TITLE) else { return false };
        glass::cloak(hwnd, true);
        shape::never_activate(hwnd);
        shape::see_through(hwnd);
        surface::attach(hwnd, style());
        // No islands until the first frame lays them out: nothing shows, nothing takes clicks.
        surface::set_islands(hwnd, &[], 1.0, true, None);
        self.place(hwnd);
        window::show(hwnd);
        self.hwnd = Some(hwnd);
        self.warm = Some(0);
        self.ctx.request_repaint_of(id());
        true
    }

    /// The window's frame: lay out, draw, and do what was clicked. `front_of` finds an app's
    /// window, for apps whose toasts cannot be handed to them.
    pub fn frame(&mut self, ctx: &egui::Context, icons: Icons, front_of: &dyn Fn(&str) -> Option<isize>) {
        let Some(hwnd) = self.hwnd else { return };
        let now = Instant::now();
        // While something slides this window repaints itself every frame, and then the host's
        // timers do not come; so its own frames run the timers too, and wake the host when the
        // last island has gone.
        let was_visible = self.feed.visible();
        self.feed.tick(now);
        if was_visible && !self.feed.visible() {
            host::request_frame();
        }
        if !self.feed.visible() {
            // Lingering, hidden: draw nothing (and so ask for no more frames).
            window::hide(hwnd);
            surface::set_islands(hwnd, &[], ctx.pixels_per_point(), true, None);
            egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |_| {});
            return;
        }
        surface::set_style(hwnd, style());
        let screen = ctx.screen_rect();
        let measure = draw::Fonts(ctx);
        let (cards, controls) = if self.feed.list_open() {
            list_cards(&self.feed, screen, &measure)
        } else {
            (popup_cards(&self.feed, screen, now, &measure), None)
        };
        let view = View { cards, controls, dnd: self.feed.dnd, composed: surface::composed(hwnd) };
        let drawn = draw::draw(ctx, &view, &mut self.replies, icons);
        drop(view);
        surface::set_islands(hwnd, &drawn.islands, ctx.pixels_per_point(), true, None);
        let over = ctx.pointer_hover_pos().is_some_and(|p| drawn.islands.iter().any(|i| i.rect.contains(p)));
        self.feed.hold(over || drawn.typing, now);
        let was_visible = self.feed.visible();
        for event in drawn.events {
            self.act(event, hwnd, front_of);
        }
        if self.feed.visible() != was_visible {
            host::request_frame();
        }
        if self.feed.moving(Instant::now()) {
            ctx.request_repaint();
        }
        self.warm_up(ctx, hwnd);
    }

    fn act(&mut self, event: Event, hwnd: isize, front_of: &dyn Fn(&str) -> Option<isize>) {
        let invoke = |feed: &Feed, id: u32, how: &dyn Fn(&Toast) -> Option<Invoke>| {
            feed.toast(id).and_then(|t| how(t).map(|i| (t.clone(), i)))
        };
        let activation = match event {
            Event::Open(id) => invoke(&self.feed, id, &|t| Some(Invoke::click(t))),
            Event::Button(id, i) => invoke(&self.feed, id, &|t| t.actions.get(i).map(Invoke::button)),
            Event::Send(id) => {
                let text = self.replies.remove(&id).unwrap_or_default();
                invoke(&self.feed, id, &|t| t.reply.as_ref().map(|r| Invoke::reply(r, text.trim())))
            }
            Event::Dismiss(id) => {
                act::dismiss(self.source.clone(), vec![id]);
                self.feed.remove(id);
                None
            }
            Event::ToggleDnd => {
                let mut settings = BarSettings::load();
                settings.do_not_disturb = !self.feed.dnd;
                settings.save();
                self.feed.dnd = settings.do_not_disturb;
                None
            }
            Event::ClearAll => {
                let ids = self.feed.toasts().iter().map(|t| t.id).collect();
                act::dismiss(self.source.clone(), ids);
                self.feed.clear();
                self.feed.close_list();
                None
            }
            Event::Focus => {
                window::take_focus(hwnd);
                None
            }
        };
        if let Some((toast, invoke)) = activation {
            let front = front_of(&toast.app_id);
            self.feed.remove(toast.id);
            self.replies.remove(&toast.id);
            if self.feed.list_open() && !self.feed.toasts().is_empty() {
                // Acted on from the list: the list stays until the app comes forward.
            } else {
                self.feed.close_list();
            }
            act::activate(self.source.clone(), toast, invoke, front);
        }
    }

    fn warm_up(&mut self, ctx: &egui::Context, hwnd: isize) {
        let Some(n) = self.warm else { return };
        if n + 1 >= WARM_FRAMES {
            glass::cloak(hwnd, false);
            self.warm = None;
        } else {
            self.warm = Some(n + 1);
            ctx.request_repaint();
        }
    }
}

/// The popups, newest at the top (nearest the clock), each slid up under the clock by how far
/// it is out of place.
fn popup_cards<'a>(feed: &'a Feed, screen: Rect, now: Instant, measure: &dyn layout::Measure) -> Vec<Card<'a>> {
    let shown: Vec<(&Toast, f32)> = feed
        .popups()
        .iter()
        .rev()
        .filter_map(|p| feed.toast(p.id).map(|t| (t, Feed::offset(p, now))))
        .collect();
    let parts: Vec<_> = shown.iter().map(|(t, _)| layout::card(t, t.attribution.is_some(), measure)).collect();
    let heights: Vec<f32> = parts.iter().map(|p| p.size.y).collect();
    let top = screen.top() + UNDER;
    let rects = layout::stack(&heights, &[], top, screen.bottom() - layout::EDGE, screen.width());
    rects
        .into_iter()
        .zip(shown.into_iter().zip(parts))
        .map(|(rect, ((toast, off), parts))| Card {
            toast,
            // Fully out, it is just above the window's top edge: tucked under the clock.
            rect: rect.translate(vec2(0.0, -off * (rect.bottom() - screen.top()))),
            parts,
            opacity: 1.0 - off,
            small: toast.attribution.clone(),
        })
        .collect()
}

/// The list: the do-not-disturb pill first, under the clock, then the newest app's group, and so
/// on down, each app's newest first. At most `PER_APP` of an app's toasts; the last says how
/// many more there are.
fn list_cards<'a>(feed: &'a Feed, screen: Rect, measure: &dyn layout::Measure) -> (Vec<Card<'a>>, Option<Rect>) {
    let mut items: Vec<(&Toast, Option<String>)> = Vec::new();
    let mut gaps = vec![0.0];
    for group in feed.groups() {
        let more = group.toasts.len().saturating_sub(PER_APP);
        for (i, t) in group.toasts.iter().take(PER_APP).enumerate() {
            let last = i + 1 == group.toasts.len().min(PER_APP);
            let small = if last && more > 0 { Some(format!("{more} more")) } else { t.attribution.clone() };
            gaps.push(if i == 0 { GROUP_GAP } else { GAP });
            items.push((t, small));
        }
    }
    let parts: Vec<_> = items.iter().map(|(t, small)| layout::card(t, small.is_some(), measure)).collect();
    let mut heights = vec![layout::CONTROLS_H];
    heights.extend(parts.iter().map(|p| p.size.y));
    if let Some(g) = gaps.get_mut(1) {
        *g = GAP;
    }
    let top = screen.top() + UNDER;
    let rects = layout::stack(&heights, &gaps, top, screen.bottom() - layout::EDGE, screen.width());
    let controls = rects.first().copied();
    let cards = rects
        .into_iter()
        .skip(1)
        .zip(items.into_iter().zip(parts))
        .map(|(rect, ((toast, small), parts))| Card { toast, rect, parts, opacity: 1.0, small })
        .collect();
    (cards, controls)
}
