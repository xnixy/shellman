//! Notifications: the toasts apps send, shown as islands hanging under a top bar's bell (at the
//! right end of its right island), and the list a click on the bell (or Win+N) opens there: a
//! pill with do-not-disturb and "Clear all", then every notification grouped by app.
//!
//! Toasts pop up under the bell of the display with the window in front (or of the main display,
//! as Settings › Top bar says), newest nearest the bell, at most three; each slides out from
//! under the bar, stays a few seconds (held while the pointer is over it) and slides back. They
//! never take the focus, wait while do-not-disturb is on, and while a fullscreen app covers their
//! display (they are counted on the bell instead). The list opens under the bell that was
//! clicked, on that monitor.
//!
//! The toasts come from the platform's own store (`shellman_core::notify`), read again on a
//! thread whenever the platform logs a change; nothing polls. Tray apps' balloons (which Explorer
//! would have turned into toasts) join them from the tray (`tray::balloon`); those live only here.
//!
//! The window is one egui viewport, made the first time something shows and kept from then on:
//! between uses it is parked, cloaked, with no islands (so nothing shows and every click goes
//! through) at its last size, never hidden (a hidden egui window spins on a pending frame) and
//! never shrunk to nothing. It is as wide as the column and as tall as its islands reach, never
//! past the work area, placed in physical pixels on the bell's monitor; its size passes
//! `glass::guard` before Windows sees it, and changes only when what shows does.
//!
//! Pieces: `feed` (what shows, and when), `layout` (geometry), `target` (which bar), `draw`
//! (painting), `act` (doing what was clicked, off the UI thread), `window` (Win32).

mod act;
mod draw;
mod feed;
pub(crate) mod layout;
pub mod summon;
pub mod target;
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
use shellman_core::settings::bar::{BarSettings, ToastScreen};
use shellman_ui::glass::{self, Style};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use target::Clock;
use windows::Win32::Foundation::RECT;

const TITLE: &str = "shellman notifications";
/// Frames drawn before the window is uncloaked, as the preview panel does.
const WARM_FRAMES: u8 = 2;

pub use act::Running;
pub use draw::Icons;
pub use window::{areas, show as show_window};

pub fn id() -> ViewportId {
    ViewportId::from_hash_of("shellman notifications")
}

/// The islands' glass: the dock's material at the toasts' radius.
pub fn style() -> Style {
    Style::from_theme(layout::RADIUS)
}

/// The window as the host declares it: hidden, far away and small until placed.
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
        .with_inner_size([64.0, 64.0])
}

/// What the top bar's clock shows.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Badge {
    pub unseen: usize,
    /// How many notifications there are, seen or not.
    pub count: usize,
    pub any: bool,
    pub dnd: bool,
    pub open: bool,
}

type Incoming = Arc<Mutex<Option<Result<Vec<Toast>, String>>>>;

/// What the host tells the notifications each frame.
pub struct Tick<'a> {
    pub now: Instant,
    pub dnd: bool,
    /// "15:42" when do-not-disturb ends by itself.
    pub dnd_until: Option<String>,
    pub toasts_on: ToastScreen,
    /// The display of the window in front.
    pub focused: Option<String>,
    /// Whether a fullscreen app covers this display.
    pub covered: &'a dyn Fn(&str) -> bool,
}

pub struct Notify {
    ctx: egui::Context,
    source: Arc<dyn Source>,
    incoming: Incoming,
    _watch: Watch,
    feed: Feed,
    /// The window, once it exists (it is kept from then on).
    hwnd: Option<isize>,
    warm: Option<u8>,
    /// Whether it is out (not parked) now.
    out: bool,
    /// Text typed into reply boxes, by toast.
    replies: HashMap<u32, String>,
    /// Toasts whose long body shows in full.
    expanded: HashSet<u32>,
    /// The app in front when the list opened: the list closes when another comes forward.
    front_at_open: Option<isize>,
    /// The last read failed (shown once on stderr, not on screen).
    failed: bool,
    /// Every top bar's bell, by monitor device, and which is the primary's.
    clocks: HashMap<String, Clock>,
    primary: Option<String>,
    /// The bar the open list hangs from.
    list_device: Option<String>,
    /// The bar the toasts now up hang from.
    toast_device: Option<String>,
    /// Where the window was put.
    placed: Option<[i32; 4]>,
    /// How tall the islands reach, in points, as last laid out.
    content_h: f32,
    dnd_until: Option<String>,
    /// The store as last read, and the tray's balloons shown with it.
    store: Vec<Toast>,
    balloons: Vec<(Toast, crate::tray::balloon::Balloon)>,
    next_balloon: u32,
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
            out: false,
            replies: HashMap::new(),
            expanded: HashSet::new(),
            front_at_open: None,
            failed: false,
            clocks: HashMap::new(),
            primary: None,
            list_device: None,
            toast_device: None,
            placed: None,
            content_h: 200.0,
            dnd_until: None,
            store: Vec::new(),
            balloons: Vec::new(),
            next_balloon: crate::tray::balloon::FIRST_ID,
        }
    }

    pub fn badge(&self) -> Badge {
        Badge {
            unseen: self.feed.unseen(),
            count: self.feed.toasts().len(),
            any: !self.feed.toasts().is_empty(),
            dnd: self.feed.dnd,
            open: self.feed.list_open(),
        }
    }

    /// A top bar's bell, as that bar last drew it.
    pub fn set_bell(&mut self, device: &str, primary: bool, clock: Clock) {
        if self.clocks.get(device) != Some(&clock) {
            self.clocks.insert(device.to_owned(), clock);
        }
        if primary && self.primary.as_deref() != Some(device) {
            self.primary = Some(device.to_owned());
        }
    }

    /// Forget the bars that are gone.
    pub fn keep_clocks(&mut self, devices: &[&str]) {
        self.clocks.retain(|d, _| devices.contains(&d.as_str()));
    }

    /// Where the window hangs from now: the list's clock, or the toasts'.
    fn anchor(&self) -> Option<(i32, i32)> {
        let device = if self.feed.list_open() { self.list_device.as_ref() } else { self.toast_device.as_ref() };
        let clock = device.and_then(|d| self.clocks.get(d)).or_else(|| self.primary.as_ref().and_then(|p| self.clocks.get(p)));
        clock.or_else(|| self.clocks.values().next()).map(|c| c.anchor)
    }

    /// The monitor the window is on now, as the layout needs it.
    fn area(&self) -> Option<(layout::Area, (i32, i32))> {
        let anchor = self.anchor()?;
        Some((layout::area_of(anchor, &window::areas())?, anchor))
    }

    fn place(&mut self, hwnd: isize) {
        let Some((area, anchor)) = self.area() else { return };
        let Some(wanted) = layout::window_for(anchor, &area, self.content_h) else { return };
        let size = glass::guard::window((wanted[2] - wanted[0], wanted[3] - wanted[1]));
        let Some((w, h)) = size else { return };
        let wanted = [wanted[0], wanted[1], wanted[0] + w, wanted[1] + h];
        if self.placed != Some(wanted) {
            let [left, top, right, bottom] = wanted;
            place::put(hwnd, RECT { left, top, right, bottom });
            self.placed = Some(wanted);
        }
    }

    /// The bell was clicked on `device`'s bar (or Win+N asked for that display), with `front` the
    /// app window in front: open the list there, or close it.
    pub fn toggle_list(&mut self, front: Option<isize>, device: &str) {
        if self.feed.list_open() {
            self.feed.close_list();
        } else {
            self.open_list(front, device);
        }
        self.changed();
    }

    fn open_list(&mut self, front: Option<isize>, device: &str) {
        self.list_device = Some(device.to_owned());
        self.feed.open_list();
        self.front_at_open = front;
    }

    pub fn list_open(&self) -> bool {
        self.feed.list_open()
    }

    pub fn close_list(&mut self) {
        if self.feed.list_open() {
            self.feed.close_list();
            self.changed();
        }
    }

    /// The app in front is now `front`: an open list closes when another app comes forward.
    pub fn front_is(&mut self, front: Option<isize>) {
        if self.feed.list_open() && front.is_some() && front != self.front_at_open {
            self.feed.close_list();
            self.changed();
        }
    }

    /// In the host's frame: take a fresh read, let the timers run, and say whether the window's
    /// viewport is to be declared (from its first use on, always).
    pub fn tick(&mut self, t: &Tick) -> bool {
        let now = t.now;
        self.feed.dnd = t.dnd;
        self.dnd_until = t.dnd_until.clone();
        // Where new toasts would go, and whether they may pop up there.
        let devices: Vec<&str> = self.clocks.keys().map(String::as_str).collect();
        let showing = (!self.feed.popups().is_empty()).then_some(()).and(self.toast_device.as_deref());
        let device = target::toast_device(t.toasts_on, showing, t.focused.as_deref(), self.primary.as_deref(), &devices).map(str::to_owned);
        let quiet = device.as_deref().is_some_and(|d| (t.covered)(d));
        let read = self.incoming.lock().unwrap().take();
        // Balloons from the tray join the store's toasts.
        let fresh = crate::tray::balloon::take();
        for b in fresh.iter() {
            let toast = crate::tray::balloon::to_toast(b, self.next_balloon, std::time::SystemTime::now());
            self.next_balloon = self.next_balloon.wrapping_add(1).max(crate::tray::balloon::FIRST_ID);
            self.balloons.push((toast, b.clone()));
        }
        let read = match read {
            Some(Ok(list)) => {
                self.store = list;
                Some(Ok(self.with_balloons()))
            }
            None if !fresh.is_empty() => Some(Ok(self.with_balloons())),
            other => other,
        };
        match read {
            Some(Ok(list)) => {
                self.failed = false;
                if self.feed.update(list, now, quiet) {
                    let feed = &self.feed;
                    self.replies.retain(|id, _| feed.toast(*id).is_some());
                    self.expanded.retain(|id| feed.toast(*id).is_some());
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
        if self.feed.popups().is_empty() {
            self.toast_device = None;
        } else if self.toast_device.is_none() {
            self.toast_device = device;
        }
        if let Some(due) = self.feed.due(now) {
            self.ctx.request_repaint_after_for(due, ViewportId::ROOT);
        }
        if let Some(hwnd) = self.hwnd {
            if self.feed.visible() {
                self.place(hwnd);
                if !self.out {
                    self.out = true;
                    self.warm = Some(0);
                    self.repaint();
                }
            } else if self.out {
                self.park(hwnd);
            }
        }
        self.hwnd.is_some() || self.feed.visible()
    }

    /// The store's toasts and the balloons, oldest first.
    fn with_balloons(&self) -> Vec<Toast> {
        let mut all = self.store.clone();
        all.extend(self.balloons.iter().map(|(t, _)| t.clone()));
        all.sort_by_key(|t| t.time);
        all
    }

    /// Remove toasts from the store (off the UI thread) and balloons from here.
    fn forget(&mut self, ids: Vec<u32>) {
        let (local, stored): (Vec<u32>, Vec<u32>) = ids.into_iter().partition(|id| self.balloons.iter().any(|(t, _)| t.id == *id));
        self.balloons.retain(|(t, _)| !local.contains(&t.id));
        if !stored.is_empty() {
            act::dismiss(self.source.clone(), stored);
        }
    }

    /// Out of sight and out of the way: cloaked, no islands, every click through. Kept shown
    /// at its size.
    fn park(&mut self, hwnd: isize) {
        self.out = false;
        self.warm = None;
        glass::cloak(hwnd, true);
        surface::set_islands(hwnd, &[], 1.0, true, None);
    }

    fn repaint(&self) {
        if self.hwnd.is_some() {
            self.ctx.request_repaint_of(id());
        }
    }

    /// Something changed what shows: the host decides where the window goes.
    fn changed(&self) {
        host::request_frame();
        self.repaint();
    }

    /// In the host's frame, after it declared the window: find it once it exists and give it its
    /// manners and glass, once. False while it is still to come.
    pub fn adopt(&mut self) -> bool {
        if self.hwnd.is_some() || !self.feed.visible() {
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
        self.out = true;
        self.warm = Some(0);
        self.ctx.request_repaint_of(id());
        true
    }

    /// The window's frame: lay out, draw, and do what was clicked. `running` lists the apps
    /// with windows, to bring the clicked toast's app forward.
    pub fn frame(&mut self, ctx: &egui::Context, icons: Icons, running: &dyn Fn() -> act::Running) {
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
        if !self.feed.visible() || !self.out {
            // Parked: draw nothing (and so ask for no more frames).
            surface::set_islands(hwnd, &[], ctx.pixels_per_point(), true, None);
            egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |_| {});
            return;
        }
        surface::set_style(hwnd, style());
        let screen = ctx.screen_rect();
        let room = self.area().map_or(screen.height(), |(area, anchor)| layout::room_below(anchor, &area));
        let measure = draw::Fonts(ctx);
        let list = self.feed.list_open();
        let (cards, controls, content_h) = if list {
            list_cards(&self.feed, &self.expanded, screen.width(), room, &measure)
        } else {
            let (cards, h) = popup_cards(&self.feed, &self.expanded, screen, room, now, &measure);
            (cards, None, h)
        };
        let view = View {
            cards,
            controls,
            dnd: self.feed.dnd,
            dnd_until: self.dnd_until.clone(),
            any: !self.feed.toasts().is_empty(),
            composed: surface::composed(hwnd),
        };
        let drawn = draw::draw(ctx, &view, &mut self.replies, icons);
        drop(view);
        surface::set_islands(hwnd, &drawn.islands, ctx.pixels_per_point(), true, None);
        let over = ctx.pointer_hover_pos().is_some_and(|p| drawn.islands.iter().any(|i| i.rect.contains(p)));
        self.feed.hold(over || drawn.typing, now);
        // The window follows its content's height (only when that changes).
        if (content_h - self.content_h).abs() > 0.5 {
            self.content_h = content_h;
            self.place(hwnd);
            ctx.request_repaint();
        }
        let was_visible = self.feed.visible();
        for event in drawn.events {
            self.act(event, hwnd, running);
        }
        if self.feed.visible() != was_visible {
            host::request_frame();
        }
        if self.feed.moving(Instant::now()) {
            ctx.request_repaint();
        }
        self.warm_up(ctx, hwnd);
    }

    fn act(&mut self, event: Event, hwnd: isize, running: &dyn Fn() -> act::Running) {
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
                self.forget(vec![id]);
                self.feed.remove(id);
                None
            }
            Event::ClearApp(id) => {
                if let Some(app) = self.feed.toast(id).map(|t| t.app_id.clone()) {
                    let ids = self.feed.clear_app(&app);
                    self.forget(ids);
                }
                None
            }
            Event::Expand(id) => {
                if !self.expanded.remove(&id) {
                    self.expanded.insert(id);
                }
                None
            }
            Event::ToggleDnd => {
                let mut settings = BarSettings::load();
                let on = !self.feed.dnd;
                settings.do_not_disturb = on;
                settings.dnd_until = None;
                settings.save();
                self.feed.dnd = on;
                None
            }
            Event::DndHour => {
                let mut settings = BarSettings::load();
                settings.do_not_disturb = true;
                settings.dnd_until = Some(now_utc() + 3600);
                settings.save();
                self.feed.dnd = true;
                None
            }
            Event::ClearAll => {
                let ids = self.feed.toasts().iter().map(|t| t.id).collect();
                self.forget(ids);
                self.feed.clear();
                None
            }
            Event::Focus => {
                window::take_focus(hwnd);
                None
            }
        };
        if let Some((toast, invoke)) = activation {
            if let Some(i) = self.balloons.iter().position(|(t, _)| t.id == toast.id) {
                // A balloon: its app is told, as Explorer would.
                let (_, b) = self.balloons.remove(i);
                crate::tray::balloon::clicked(&b);
                self.feed.remove(toast.id);
                self.feed.close_list();
                return;
            }
            self.feed.remove(toast.id);
            self.replies.remove(&toast.id);
            // Acted on from the list: it closes as the app comes forward.
            self.feed.close_list();
            act::activate(self.source.clone(), toast, invoke, running());
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

    /// The open list's islands in screen pixels, for what counts as inside it.
    pub fn list_rects(&self) -> Vec<(i32, i32, i32, i32)> {
        let mut out: Vec<(i32, i32, i32, i32)> = self.clocks.values().flat_map(|c| std::iter::once(c.rect).chain(c.also)).collect();
        if let Some(p) = self.placed {
            out.push((p[0], p[1], p[2], p[3]));
        }
        out
    }
}

/// The display of the window in front.
pub fn focused_device() -> Option<String> {
    window::foreground_device()
}

fn now_utc() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The popups, newest at the top (nearest the clock), each slid up under the clock by how far
/// it is out of place; and how tall they reach when all are in place.
fn popup_cards<'a>(
    feed: &'a Feed,
    expanded: &HashSet<u32>,
    screen: Rect,
    room: f32,
    now: Instant,
    measure: &dyn layout::Measure,
) -> (Vec<Card<'a>>, f32) {
    let shown: Vec<(&Toast, f32)> = feed
        .popups()
        .iter()
        .rev()
        .filter_map(|p| feed.toast(p.id).map(|t| (t, Feed::offset(p, now))))
        .collect();
    let parts: Vec<_> = shown.iter().map(|(t, _)| layout::card(t, t.attribution.is_some(), expanded.contains(&t.id), measure)).collect();
    let heights: Vec<f32> = parts.iter().map(|p| p.size.y).collect();
    let rects = layout::stack(&heights, &[], UNDER, room - layout::EDGE, screen.width());
    let reach = rects.last().map_or(UNDER, |r| r.bottom()) + layout::EDGE;
    let cards = rects
        .into_iter()
        .zip(shown.into_iter().zip(parts))
        .map(|(rect, ((toast, off), parts))| Card {
            toast,
            // Fully out, it is just above the window's top edge: tucked under the clock.
            rect: rect.translate(vec2(0.0, -off * (rect.bottom() - screen.top()))),
            parts,
            opacity: 1.0 - off,
            small: toast.attribution.clone(),
            expanded: expanded.contains(&toast.id),
            group: None,
        })
        .collect();
    (cards, reach)
}

/// The list: the do-not-disturb pill first, under the bell, then the newest app's group, and so
/// on down, each app's newest first. At most `PER_APP` of an app's toasts; the last says how
/// many more there are. Returns the cards, the pill and how tall it all reaches.
fn list_cards<'a>(
    feed: &'a Feed,
    expanded: &HashSet<u32>,
    width: f32,
    room: f32,
    measure: &dyn layout::Measure,
) -> (Vec<Card<'a>>, Option<Rect>, f32) {
    let mut items: Vec<(&Toast, Option<String>, Option<usize>)> = Vec::new();
    let mut gaps = vec![0.0];
    for group in feed.groups() {
        let more = group.toasts.len().saturating_sub(PER_APP);
        let count = group.toasts.len();
        for (i, t) in group.toasts.iter().take(PER_APP).enumerate() {
            let last = i + 1 == count.min(PER_APP);
            let small = if last && more > 0 { Some(format!("{more} more")) } else { t.attribution.clone() };
            gaps.push(if i == 0 { GROUP_GAP } else { GAP });
            items.push((t, small, (i == 0 && count > 1).then_some(count)));
        }
    }
    let parts: Vec<_> = items.iter().map(|(t, small, _)| layout::card(t, small.is_some(), expanded.contains(&t.id), measure)).collect();
    let mut heights = vec![layout::CONTROLS_H];
    heights.extend(parts.iter().map(|p| p.size.y));
    if let Some(g) = gaps.get_mut(1) {
        *g = GAP;
    }
    let rects = layout::stack(&heights, &gaps, UNDER, room - layout::EDGE, width);
    let reach = rects.last().map_or(UNDER, |r| r.bottom()) + layout::EDGE;
    let controls = rects.first().copied();
    let cards = rects
        .into_iter()
        .skip(1)
        .zip(items.into_iter().zip(parts))
        .map(|(rect, ((toast, small, group), parts))| Card {
            toast,
            rect,
            parts,
            opacity: 1.0,
            small,
            expanded: expanded.contains(&toast.id),
            group,
        })
        .collect();
    (cards, controls, reach)
}
