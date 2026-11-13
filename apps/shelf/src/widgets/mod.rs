//! The desktop's widgets (stage 2 of "Shelf + Widgets"): Today (date, weather, next events) and
//! Now playing, stacked top right of the primary monitor (or every monitor, as Settings ›
//! Desktop › "Show widgets on" says), each a real glass island following the glass slider.
//!
//! They live in the shelf process: it already owns desktop-level glass windows per monitor that
//! sit under every app, never take the focus and step aside for a fullscreen app, and it follows
//! `desktop.json` live. Each monitor with widgets gets one small window of its own (`view`),
//! sized once to the tallest stack (`place`), never the monitor.
//!
//! Idle costs nothing: frames run when data arrives (the feeds worker, the media session), when
//! the pointer is over a widget, once a minute for the date and the "in 5 min" chips, and once a
//! second while something plays (its time and progress line), never while a fullscreen app
//! covers the monitor.

pub mod feeds;
mod icons;
pub mod model;
mod paint;
pub mod place;
mod view;

use crate::{actions, displays, log, window};
use feeds::{CalConfig, Config, Feeds, WeatherConfig};
use icons::Icons;
use shellman_core::calendar::system::SystemClock;
use shellman_core::calendar::zone::Clock;
use shellman_core::monitors::Monitor;
use shellman_core::settings::desktop::DesktopSettings;
use shellman_core::system::media::{MediaWatch, NowPlaying};
use shellman_core::system::media_art::Art;
use shellman_ui::glass::{self, GlassPanel, Island, Style};
use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use view::WidgetView;

thread_local! {
    /// The widgets' state, on the window thread. Their windows are deferred viewports, which
    /// eframe draws on this thread whenever one of them asks: the once-a-second tick of Now
    /// playing and the minute tick of Today redraw that window alone, never the shelves (which an
    /// immediate viewport would, being drawn only inside the shelf's own frame).
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// The state, if this is the window thread and it is not in use (it never is re-entered).
fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok()?.as_mut().map(f))
}

/// The shelf's handle on the widgets; their state lives in `STATE`.
pub struct Widgets {
    /// Not Send: the state is on this thread.
    _here: std::marker::PhantomData<*const ()>,
}

struct State {
    views: Vec<WidgetView>,
    /// The widget windows, for the feeds and the media session to ask for a frame of them
    /// (from their own threads) without one of the shelves.
    targets: Arc<Mutex<Vec<egui::ViewportId>>>,
    feeds: Option<Feeds>,
    media: Option<MediaWatch>,
    data: feeds::State,
    playing: Option<NowPlaying>,
    /// The art's texture, and the pixels it was made from.
    art: Option<(Arc<Art>, egui::TextureHandle)>,
    icons: Icons,
    clock: SystemClock,
    h24: bool,
    settings: DesktopSettings,
    /// The minute the frame last ran in (the clock and region are read again each minute).
    minute: i64,
}

fn now_utc() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// Ask for a frame of each widget window.
fn repaint_all(ctx: &egui::Context, targets: &Mutex<Vec<egui::ViewportId>>) {
    for id in targets.lock().map(|t| t.clone()).unwrap_or_default() {
        ctx.request_repaint_of(id);
    }
}

impl Widgets {
    pub fn new() -> Self {
        STATE.with(|s| *s.borrow_mut() = Some(State::new()));
        Self { _here: std::marker::PhantomData }
    }

    /// See `State::apply`.
    pub fn apply(&mut self, settings: &DesktopSettings, ctx: &egui::Context) -> bool {
        with_state(|s| s.apply(settings, ctx)).unwrap_or(false)
    }

    /// See `State::lay_out`.
    pub fn lay_out(&mut self, monitors: &[Monitor]) {
        with_state(|s| s.lay_out(monitors));
    }

    /// In the shelf's frame: keep every widget window declared, and have it drawn again after
    /// this frame (whatever made the shelf draw, the settings, the look or a fullscreen app, may
    /// change it too). Their own ticks do not come through here.
    pub fn frame(&mut self, ctx: &egui::Context) {
        let views: Vec<(egui::ViewportId, egui::ViewportBuilder)> =
            with_state(|s| s.views.iter().map(|v| (v.id, v.builder())).collect()).unwrap_or_default();
        for (id, builder) in views {
            ctx.show_viewport_deferred(id, builder, move |vctx, _| {
                with_state(|s| s.frame_of(vctx, id));
                shellman_ui::font_atlas::end_frame(vctx);
            });
            ctx.request_repaint_of(id);
        }
    }
}

impl State {
    fn new() -> Self {
        Self {
            views: Vec::new(),
            targets: Arc::new(Mutex::new(Vec::new())),
            feeds: None,
            media: None,
            data: feeds::State::default(),
            playing: None,
            art: None,
            icons: Icons::default(),
            clock: SystemClock::new(),
            h24: shellman_core::system::region::read_intl().is_24h(),
            settings: DesktopSettings::default(),
            minute: 0,
        }
    }

    /// Follow the settings: start or stop the feeds and the media watch, and tell the feeds what
    /// to read. True when the windows must be laid out again (which monitors, or on/off).
    pub fn apply(&mut self, settings: &DesktopSettings, ctx: &egui::Context) -> bool {
        let shown = settings.widgets_shown();
        let relayout = shown != self.settings.widgets_shown() || settings.widgets.on != self.settings.widgets.on;
        let w = &settings.widgets;
        if shown && w.today {
            let targets = self.targets.clone();
            let feeds = self.feeds.get_or_insert_with(|| {
                let ctx = ctx.clone();
                Feeds::start(move || repaint_all(&ctx, &targets), |line| log::line(line))
            });
            let cal = &w.calendar;
            let fahrenheit = w.weather.units.fahrenheit(shellman_core::system::region::imperial());
            feeds.configure(Config {
                calendar: (!cal.username.trim().is_empty()).then(|| CalConfig {
                    server: cal.server.clone(),
                    username: cal.username.clone(),
                    hidden: cal.hidden.clone(),
                    saved: cal.saved,
                }),
                weather: w.weather.is_set().then(|| WeatherConfig { place: w.weather.place.clone(), lat: w.weather.lat, lon: w.weather.lon, fahrenheit }),
            });
        } else if self.feeds.take().is_some() {
            self.data = feeds::State::default();
        }
        if shown && w.now_playing {
            if self.media.is_none() {
                let (ctx, targets) = (ctx.clone(), self.targets.clone());
                self.media = Some(MediaWatch::start(Arc::new(move || repaint_all(&ctx, &targets))));
            }
        } else if self.media.take().is_some() {
            self.playing = None;
            self.art = None;
        }
        self.settings = settings.clone();
        relayout
    }

    /// The machine woke or the network came back: read again.
    fn refresh(&self) {
        if let Some(f) = &self.feeds {
            f.refresh();
        }
    }

    /// Bring the windows in line with the monitors and the settings.
    pub fn lay_out(&mut self, monitors: &[Monitor]) {
        let want: Vec<Monitor> = if self.settings.widgets_shown() { displays::targets(self.settings.widgets.on, monitors) } else { Vec::new() };
        let names: Vec<&str> = want.iter().map(|m| m.device.as_str()).collect();
        log::line(format!("widgets on: {}", if names.is_empty() { "none".to_owned() } else { names.join(", ") }));
        self.views.retain(|v| want.iter().any(|m| m.device == v.device));
        for m in want {
            match self.views.iter_mut().find(|v| v.device == m.device) {
                Some(v) => {
                    v.monitor = m;
                    v.placed = None;
                }
                None => self.views.push(WidgetView::new(m)),
            }
        }
        if let Ok(mut t) = self.targets.lock() {
            *t = self.views.iter().map(|v| v.id).collect();
        }
    }

    fn take_data(&mut self, ctx: &egui::Context) {
        if let Some(state) = self.feeds.as_mut().and_then(Feeds::take) {
            self.data = state;
        }
        if let Some(now) = self.media.as_ref().and_then(MediaWatch::take) {
            match now.as_ref().and_then(|n| n.art.clone()) {
                Some(a) if !self.art.as_ref().is_some_and(|(b, _)| Arc::ptr_eq(&a, b)) => {
                    let side = a.side as usize;
                    if a.rgba.len() == side * side * 4 && side > 0 {
                        let image = egui::ColorImage::from_rgba_unmultiplied([side, side], &a.rgba);
                        let tex = ctx.load_texture("widget-art", image, egui::TextureOptions::LINEAR);
                        self.art = Some((a, tex));
                    }
                }
                Some(_) => {}
                None => self.art = None,
            }
            self.playing = now;
        }
    }

    /// The frame of the widget window `id`, in its own viewport's `ctx`: take new data, draw it,
    /// and ask for its next frame (its own, not the shelves') only when something on it will
    /// change by itself.
    fn frame_of(&mut self, ctx: &egui::Context, id: egui::ViewportId) {
        if window::take_resumed() {
            log::line("resumed: reading the widgets' data again");
            self.refresh();
        }
        self.take_data(ctx);
        let now = now_utc();
        let minute = (now / 60.0) as i64;
        if minute != self.minute {
            self.minute = minute;
            self.clock = SystemClock::new();
            self.h24 = shellman_core::system::region::read_intl().is_24h();
        }
        let w = &self.settings.widgets;
        let clock = &self.clock;
        let offset = |t: i64| clock.local_offset(t);
        let today = w.today.then(|| model::today(&self.data, !w.calendar.username.trim().is_empty(), now as i64, &offset, self.h24));
        let playing = if w.now_playing { self.playing.as_ref().map(|n| model::now_playing(n, now)) } else { None };
        let mut events = Vec::new();
        let art = self.art.as_ref().map(|(_, t)| t.clone());
        let Some(view) = self.views.iter_mut().find(|v| v.id == id) else { return };
        let visible = view_frame(ctx, view, today.as_ref(), playing.as_ref(), art.as_ref(), &mut self.icons, &mut events);
        for e in events {
            match e {
                paint::Event::Media(a) => {
                    if let Some(m) = &self.media {
                        m.act(a);
                    }
                }
                paint::Event::Settings => actions::settings("desktop"),
            }
        }
        if visible {
            let tick = if playing.as_ref().is_some_and(|p| p.playing) {
                Duration::from_secs(1)
            } else {
                shellman_core::system::clock::until_next_minute()
            };
            ctx.request_repaint_after(tick);
        }
    }
}

/// One widget window's frame. True when it shows anything.
fn view_frame(
    ctx: &egui::Context,
    view: &mut WidgetView,
    today: Option<&model::TodayModel>,
    playing: Option<&model::NowModel>,
    art: Option<&egui::TextureHandle>,
    icons: &mut Icons,
    events: &mut Vec<paint::Event>,
) -> bool {
    crate::theme_for(ctx);
    if !view.adopt() {
        ctx.request_repaint();
        return false;
    }
    let Some(hwnd) = view.hwnd else { return false };
    if view.placed.is_none() {
        view.place();
    }
    // Laid out only at the scale of the monitor it was placed on.
    let placed = view.placed.filter(|p| (p.scale - ctx.pixels_per_point()).abs() < 0.01);
    if view.placed.is_some() && placed.is_none() {
        ctx.request_repaint();
    }
    let shown = placed.is_some() && !window::covered(hwnd) && (today.is_some() || playing.is_some());
    let mut islands = Vec::new();
    if let (true, Some(p)) = (shown, placed) {
        let (t, n) = place::stack(p.column, today.map(model::TodayModel::shape), playing.is_some());
        islands.extend(t.iter().chain(n.iter()).map(|r| Island::new(*r, place::RADIUS)));
        let style = Style::from_theme(place::RADIUS);
        let composed = view.panel.as_ref().is_some_and(GlassPanel::composed);
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
            let under = ui.painter().add(egui::Shape::Noop);
            if let (Some(r), Some(m)) = (t, today) {
                paint::today(ui, r, m, icons, events);
            }
            if let (Some(r), Some(m)) = (n, playing) {
                paint::playing(ui, r, m, art, icons, events);
            }
            if !composed {
                ui.painter().set(under, egui::Shape::Vec(glass::islands::solid_shapes(&islands, &style)));
            }
        });
    }
    if let Some(panel) = &view.panel {
        panel.set_style(Style::from_theme(place::RADIUS));
        // Only the islands show and take clicks; the rest lets the desktop through.
        panel.set_islands_only(&islands, ctx.pixels_per_point(), None);
    }
    !islands.is_empty()
}
