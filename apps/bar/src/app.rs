//! The bar process's state and its windows' frames.
//!
//! One window list, one icon cache, one set of pins (`Shared`), drawn by many windows: a top bar
//! and a dock per monitor. Each bar window is a deferred egui viewport, so it repaints on its own
//! (a hover on one dock repaints that dock only); its frame locks the shared state and draws its
//! part. Viewports are declared by the host's frames, which run only when something changes
//! (monitors, settings, the look, a woken tray or window watcher), so the host costs nothing at
//! rest. Every frame runs on the UI thread, one at a time, so the lock never waits.

use crate::bars::{self, Bars, Kind};
use crate::monitors::{self, layout};
use crate::shape::surface;
use crate::wake::Wake;
use crate::workarea::HiddenMinimized;
use crate::tray::view::Place;
use crate::{calpanel, dock, host, notify, preview, stress, topbar};
use shellman_core::settings::bar::{BarSettings, TrayMonitors};
use shellman_core::winlist::fullscreen::foreground_covers_monitor_of;
use shellman_ui::appearance::Appearance;
use shellman_ui::{font_atlas, theme};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Whether the bars draw in light mode: Windows' app mode, or `SHELLMAN_FORCE_LIGHT` to look at
/// the light palette without changing Windows' setting.
pub fn light(ctx: &egui::Context) -> bool {
    std::env::var_os("SHELLMAN_FORCE_LIGHT").is_some() || ctx.theme() == egui::Theme::Light
}

pub struct Shared {
    pub dock: dock::Dock,
    pub top: topbar::TopBar,
    pub bars: Bars,
    pub preview: preview::Preview,
    pub notify: notify::Notify,
    /// The calendar panel under the clock.
    pub calendar: calpanel::CalPanel,
    /// The bell's right-click menu.
    pub bell_menu: shellman_ui::menu::popup::PopupMenu<topbar::bell::Choice>,
    /// The calendar and what is playing, for the "now" pill and the calendar.
    pub live: crate::live::Live,
    pub wake: Option<Wake>,
}

/// The calendar island's line, from where the calendar stands.
fn calendar_status(live: &crate::live::Live) -> calpanel::month::Status {
    use calpanel::month::Status;
    use shellman_core::calendar::feed::CalStatus;
    if !live.calendar_set_up() {
        return Status::NotConnected;
    }
    match (&live.cal.calendar, live.cal.snapshot.is_some()) {
        (CalStatus::SignInFailed, _) => Status::SignInFailed,
        (CalStatus::NotConnected, _) => Status::NotConnected,
        (_, true) => Status::Ready,
        (CalStatus::Unreachable(_), false) => Status::Unreachable,
        (_, false) => Status::Connecting,
    }
}

/// While a panel is open, watch for a click outside it (its clock, pill or bell count as inside:
/// they toggle it themselves) or Esc, and close it then.
fn dismiss_panels(dismiss: &mut Option<shellman_core::dismiss::Watch>, shared: &mut Shared) {
    let open = shared.notify.list_open() || shared.calendar.is_open();
    if !open {
        *dismiss = None;
        return;
    }
    let watch = dismiss.get_or_insert_with(|| shellman_core::dismiss::Watch::start(host::request_frame));
    if watch.take() {
        shared.notify.close_list();
        shared.calendar.close();
        *dismiss = None;
        return;
    }
    let mut inside = shared.notify.list_rects();
    inside.extend(shared.calendar.rect());
    inside.extend(shared.calendar.openers());
    watch.set_rects(&inside);
}

fn now_utc() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl Shared {
    fn top_frame(&mut self, ctx: &egui::Context, device: &str, primary: bool) {
        theme::set_light(light(ctx));
        self.dock.catch_up(ctx);
        let front = self.dock.front_window().cloned();
        let icon = front.as_ref().and_then(|_| self.dock.front_icon(ctx, 16.0 * ctx.pixels_per_point()));
        let hwnd = self.bars.top_hwnd(device).unwrap_or(0);
        surface::set_style(hwnd, bars::top_style());
        let origin = (hwnd != 0).then(|| bars::client_origin(hwnd)).flatten();
        let front_hwnd = front.as_ref().map(|w| w.hwnd);
        // The notifications list and the calendar close when another app comes forward.
        self.notify.front_is(front_hwnd);
        self.calendar.front_is(front_hwnd);
        self.top.badge = self.notify.badge();
        self.top.calendar_open = self.calendar.is_open();
        if host::take_power() {
            self.top.power_changed();
        }
        // What the "now" pill shows: worked out here, cheaply, in every top bar's frame (the
        // countdown moves on with the clock's minute repaint).
        let settings = self.top.settings();
        let shown = self.now_pill(&settings);
        if self.top.pill.update(shown, Instant::now()) {
            // Another bar may have taken the change first; the rest follow.
            self.wake_tops();
        }
        let front = front.as_ref().map(|window| topbar::Front { window, icon });
        let place = Place { device, primary, origin };
        let islands = self.top.frame(ctx, front, surface::composed(hwnd), &place);
        // The calendar hangs under the bar's clock island (the second), notifications under its
        // bell: the panels under the ones that were clicked, toasts under the bell of the display
        // they go to.
        let ppp = ctx.pixels_per_point();
        if let Some(o) = origin {
            let also = islands.get(3).map(|pill| notify::layout::screen_rect(o, pill.rect, ppp));
            if let Some(clock) = islands.get(1) {
                let anchor = notify::layout::clock_anchor(o, clock.rect.center().x, clock.rect.bottom(), ppp);
                let rect = notify::layout::screen_rect(o, clock.rect, ppp);
                self.calendar.set_clock(device, notify::target::Clock { anchor, rect, also });
            }
            if let (Some(bell), Some(right)) = (self.top.bell_rect, islands.get(2)) {
                let anchor = notify::layout::clock_anchor(o, bell.center().x, right.rect.bottom(), ppp);
                let rect = notify::layout::screen_rect(o, bell, ppp);
                self.notify.set_bell(device, primary, notify::target::Clock { anchor, rect, also: None });
            }
        }
        let today = shellman_core::calendar::month::local_day(now_utc(), &|t| self.live.offset(t));
        if self.top.take_clock_click() {
            self.notify.close_list();
            self.calendar.toggle(today, front_hwnd, device);
        }
        match self.top.take_bell() {
            Some(topbar::bell::Asked::Toggle) => {
                self.calendar.close();
                self.notify.toggle_list(front_hwnd, device);
            }
            Some(topbar::bell::Asked::Menu) => {
                if let Some(bell) = self.top.bell_rect {
                    let entries = topbar::bell::menu(&settings, now_utc());
                    self.bell_menu.open(ctx, shellman_ui::menu::popup::Anchor::under(ctx, bell), entries);
                }
            }
            None => {}
        }
        if let Some(choice) = self.bell_menu.take_pick() {
            self.bell_choice(choice);
        }
        match self.top.take_asked() {
            Some(topbar::now::Asked::Media(action)) => self.live.act(action),
            Some(topbar::now::Asked::Day(day)) => {
                self.notify.close_list();
                self.calendar.open_on(day, front_hwnd, device);
            }
            None => {}
        }
        // The whole window stays the bar's: a click between the islands lands on it and does
        // nothing (the space is reserved anyway).
        surface::set_islands(hwnd, &islands, ctx.pixels_per_point(), false, None);
    }

    /// Carry out a pick from the bell's menu: do-not-disturb is saved with the bar's settings
    /// (every bar follows the file), the settings open on the Top bar page.
    fn bell_choice(&mut self, choice: topbar::bell::Choice) {
        use topbar::bell::Choice;
        let now = now_utc();
        let mut s = BarSettings::load();
        match choice {
            Choice::Dnd => {
                s.do_not_disturb = !s.dnd_at(now);
                s.dnd_until = None;
            }
            Choice::DndHour => {
                s.do_not_disturb = true;
                s.dnd_until = Some(now + 3600);
            }
            Choice::DndTomorrow => {
                let offset = |t: i64| self.live.offset(t);
                let tomorrow = shellman_core::calendar::month::local_day(now, &offset) + 1;
                let guess = tomorrow * 86_400;
                s.do_not_disturb = true;
                s.dnd_until = Some(guess - offset(guess - offset(guess)));
            }
            Choice::Settings => {
                shellman_core::sibling::start_with("settings", &["--page", "top-bar"]);
                return;
            }
        }
        s.save();
        host::request_frame();
    }

    /// What the "now" pill should show now.
    fn now_pill(&self, settings: &BarSettings) -> Option<topbar::now::pick::Shown> {
        let live = &self.live;
        let snapshot = live.cal.snapshot.as_ref().filter(|_| live.calendar_set_up());
        let inputs = topbar::now::pick::Inputs {
            settings: settings.now,
            events: snapshot.map(|s| (s.instances.as_slice(), s.calendars.as_slice())),
            media: live.playing.as_ref(),
            paused_since: live.paused_since,
            now: now_utc(),
            h24: live.h24(),
        };
        topbar::now::pick::pick(&inputs, &|t| live.offset(t))
    }

    fn wake_tops(&self) {
        if let Some(w) = &self.wake {
            w.tops();
        }
    }

    /// Lay the open preview panel out for its app's current windows.
    fn update_preview(&mut self) {
        if let Some(app) = self.preview.app_id() {
            self.preview.update(self.dock.windows_of(&app));
        }
    }

    /// A frame of the preview panel: the hovered app's windows, drawn, and what was clicked.
    fn preview_frame(&mut self, ctx: &egui::Context, device: &str) {
        theme::set_light(light(ctx));
        self.dock.catch_up(ctx);
        if let Some(app) = self.preview.app_id() {
            self.preview.update(self.dock.windows_of(&app));
        }
        let dock = &mut self.dock;
        match self.preview.frame(ctx, device, &mut |ctx, path| dock.icon(ctx, path)) {
            Some(preview::Action::Show(hwnd)) => {
                dock::Dock::show_window(hwnd);
                self.preview.dismiss();
            }
            Some(preview::Action::Close(hwnd)) => dock::close_window(hwnd),
            None => {}
        }
    }

    /// A frame of the notifications window.
    fn notify_frame(&mut self, ctx: &egui::Context) {
        theme::set_light(light(ctx));
        self.dock.catch_up(ctx);
        let dock = &mut self.dock;
        let windows = dock.front_windows();
        let running = || crate::notify::Running(windows.clone());
        self.notify.frame(ctx, &mut |ctx, path| dock.icon(ctx, path), &running);
    }

    /// A frame of the calendar panel: the month's events from the regular reading and any month
    /// read on demand (asked for here, once per month shown).
    fn calendar_frame(&mut self, ctx: &egui::Context) {
        theme::set_light(light(ctx));
        use shellman_core::calendar::month::{self, Grid, WeekRule};
        let rule = WeekRule::from_windows(self.live.intl.first_week);
        let first_day = self.live.intl.first_day;
        let view = &self.calendar.view;
        let grid = Grid::new(view.year, view.month, first_day, rule);
        let (from, to) = month::utc_range(&grid);
        if self.calendar.is_open() && self.live.calendar_ready() {
            self.live.want(from, to);
        }
        let weeks = self.top.settings().week_numbers.shown(self.live.intl.first_week);
        let live = &self.live;
        let snapshot = live.cal.snapshot.as_ref();
        let regular = snapshot.map_or(&[][..], |s| s.instances.as_slice());
        let instances = shellman_core::calendar::span::merged(regular, &live.cal.spans, from, to);
        let offset = |t: i64| live.offset(t);
        let data = calpanel::month::Data {
            status: calendar_status(live),
            instances,
            calendars: snapshot.map_or(&[][..], |s| s.calendars.as_slice()),
            offset: &offset,
            h24: live.h24(),
            first_day,
            rule,
            weeks,
            today: month::local_day(now_utc(), &offset),
        };
        self.calendar.frame(ctx, &data);
        if std::mem::take(&mut self.calendar.asked_settings) {
            // Settings › Desktop holds the calendar account (with the widgets that use it too).
            shellman_core::sibling::start_with("settings", &["--page", "desktop"]);
        }
    }

    fn dock_frame(&mut self, ctx: &egui::Context, device: &str) {
        let Some(hwnd) = self.bars.dock_hwnd(device) else { return };
        let hover = self.dock.frame(ctx, hwnd, device);
        let blocked = shellman_ui::menu::popup::any_open();
        self.preview.hover_dock(device, hover, hwnd, ctx.pixels_per_point(), blocked);
    }
}

/// The host window's app: it keeps the bar windows matched to the monitors and settings.
pub struct Host {
    shared: Arc<Mutex<Shared>>,
    wake: Wake,
    look: Appearance,
    look_watch: shellman_core::watch::Watcher,
    settings: BarSettings,
    _minimized: Option<HiddenMinimized>,
    /// The notifications badge the top bars last showed.
    badge: notify::Badge,
    /// Bar windows were still being adopted in the last frame.
    adopting: bool,
    /// The live data has been told the settings once.
    live_configured: bool,
    /// While a panel (notifications or calendar) is open: a click outside it or Esc closes it.
    dismiss: Option<shellman_core::dismiss::Watch>,
    /// Set by Win+N (the switcher's hook signals a named event).
    summoned: Arc<std::sync::atomic::AtomicBool>,
    /// The number of a Win+1 ... 9 not yet acted on, 0 for none (named events too).
    dock_key: Arc<std::sync::atomic::AtomicU8>,
}

impl Host {
    pub fn new(
        shared: Shared,
        wake: Wake,
        look: Appearance,
        look_watch: shellman_core::watch::Watcher,
        minimized: Option<HiddenMinimized>,
    ) -> Self {
        Self {
            shared: Arc::new(Mutex::new(shared)),
            wake,
            look,
            look_watch,
            settings: BarSettings::load(),
            _minimized: minimized,
            badge: notify::Badge::default(),
            adopting: true,
            live_configured: false,
            dismiss: None,
            summoned: notify::summon::start(host::request_frame),
            dock_key: crate::dock::numkey::start(host::request_frame),
        }
    }

    /// Open (or keep open) every bar window. Windows not declared in a host frame are closed.
    fn declare(&self, ctx: &egui::Context, shared: &Shared, notify_open: bool, calendar_open: bool) {
        let bars = &shared.bars;
        for (view, device, primary) in bars.views() {
            let builder = egui::ViewportBuilder::default()
                .with_title(view.title.clone())
                .with_decorations(false)
                .with_transparent(true)
                .with_resizable(false)
                .with_always_on_top()
                .with_taskbar(false)
                // Shown by `Bars` once placed on its monitor.
                .with_visible(false);
            let shared = self.shared.clone();
            let device = device.to_owned();
            let kind = view.kind;
            ctx.show_viewport_deferred(view.id, builder, move |ctx, _| {
                let mut shared = shared.lock().unwrap();
                match kind {
                    Kind::Top => shared.top_frame(ctx, &device, primary),
                    Kind::Dock => shared.dock_frame(ctx, &device),
                }
                stress::glyphs(ctx);
                font_atlas::end_frame(ctx);
            });
        }
        // Each dock's preview panel, from its first preview on (see `preview`).
        for panel in shared.preview.panels() {
            let shared = self.shared.clone();
            let device = panel.device.clone();
            ctx.show_viewport_deferred(panel.id, preview::builder(&panel.device), move |ctx, _| {
                shared.lock().unwrap().preview_frame(ctx, &device);
                stress::glyphs(ctx);
                font_atlas::end_frame(ctx);
            });
        }
        // So does the notifications window (see `notify`).
        if notify_open {
            let shared = self.shared.clone();
            ctx.show_viewport_deferred(notify::id(), notify::builder(), move |ctx, _| {
                shared.lock().unwrap().notify_frame(ctx);
                font_atlas::end_frame(ctx);
            });
        }
        // And the calendar panel, from its first use on.
        if calendar_open {
            let shared = self.shared.clone();
            ctx.show_viewport_deferred(calpanel::id(), calpanel::builder(), move |ctx, _| {
                shared.lock().unwrap().calendar_frame(ctx);
                font_atlas::end_frame(ctx);
            });
        }
    }
}

impl eframe::App for Host {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // The dock's glass menu window, once there has been a menu.
        shellman_ui::menu::popup::declare(ctx);
        let mut shared = self.shared.lock().unwrap();
        let mut layout_needed = host::take_layout();
        // Bars repaint only for what they show: this frame runs for many reasons (a window event,
        // a timer of the preview or the notifications) that change nothing on most bars.
        let mut wake_all = false;
        let changed_files = !self.look_watch.poll().is_empty();
        if changed_files || !self.live_configured {
            // The calendar account lives in the desktop's settings, in the same folder.
            self.live_configured = true;
            let live_wake: Arc<dyn Fn() + Send + Sync> = Arc::new(host::request_frame);
            let bar = BarSettings::load();
            shared.live.configure(&bar, &shellman_core::settings::desktop::DesktopSettings::load(), &live_wake);
        }
        let (cal_changed, media_changed) = shared.live.poll();
        if cal_changed || media_changed {
            // The pill decides in the bars' frames whether it shows anything new.
            self.wake.tops();
            if shared.calendar.window_exists() {
                ctx.request_repaint_of(calpanel::id());
            }
        }
        if changed_files {
            // The same folder holds the dock's pins and the bar's settings.
            shared.dock.reload_pins();
            shared.top.reload_settings();
            // Other apps keep files there too (Files saves its state every half minute): only a
            // changed look or changed settings (the clock, the tray) repaint the bars, each of
            // which restyles its glass in its own frame. Pins wake the bars themselves.
            let look = Appearance::load_or(self.look);
            if look != self.look {
                self.look = look;
                look.apply(ctx);
                wake_all = true;
            }
            let settings = BarSettings::load();
            if settings.docks != self.settings.docks {
                layout_needed = true;
            }
            if settings != self.settings {
                self.settings = settings;
                wake_all = true;
            }
        }
        // Reads the window list again if it may have changed, and wakes the bars that show it.
        shared.dock.catch_up(ctx);
        match self.dock_key.swap(0, std::sync::atomic::Ordering::AcqRel) {
            0 => {}
            n => shared.dock.win_key(n),
        }
        // The dock's height follows its icon size, and its reserved band with it.
        let sizes = bars::sizes(shared.dock.height());
        let reassert = host::take_reassert();
        if layout_needed || reassert || sizes != shared.bars.sizes() {
            let slots = layout::plan(&monitors::all(), self.settings.docks, &sizes);
            shared.bars.update(slots, sizes, reassert);
            wake_all = true;
        }
        if host::take_requery() {
            shared.bars.requery();
            wake_all = true;
        }
        let docks: Vec<String> = shared.bars.views().filter(|(v, _, _)| v.kind == Kind::Dock).map(|(_, d, _)| d.to_owned()).collect();
        shared.preview.keep_docks(&docks.iter().map(String::as_str).collect::<Vec<_>>());
        shared.preview.tick(Instant::now());
        // Notifications: do-not-disturb (which may end by itself), and where toasts may go.
        let now = now_utc();
        let dnd = self.settings.dnd_at(now);
        let dnd_until = self.settings.dnd_until.filter(|_| dnd).map(|until| {
            if let Ok(left) = u64::try_from(until - now) {
                ctx.request_repaint_after(std::time::Duration::from_secs(left + 1));
            }
            shellman_core::calendar::select::clock(until + shared.live.offset(until), shared.live.h24())
        });
        let parts: &mut Shared = &mut shared;
        let bars = &parts.bars;
        let covered = |device: &str| bars.top_hwnd(device).is_some_and(foreground_covers_monitor_of);
        let was_open = parts.notify.list_open();
        let devices: Vec<String> = bars.views().filter(|(v, _, _)| v.kind == Kind::Top).map(|(_, d, _)| d.to_owned()).collect();
        let tick = notify::Tick {
            now: Instant::now(),
            dnd,
            dnd_until,
            toasts_on: self.settings.toasts_on,
            focused: notify::focused_device(),
            covered: &covered,
        };
        let notify_open = parts.notify.tick(&tick);
        if self.summoned.swap(false, std::sync::atomic::Ordering::AcqRel) {
            // Win+N: the notifications under the bell of the display with the window in front.
            let focused = notify::focused_device().filter(|d| devices.contains(d));
            if let Some(device) = focused.or_else(|| devices.first().cloned()) {
                shared.calendar.close();
                let front = shared.dock.front_window().map(|w| w.hwnd);
                shared.notify.toggle_list(front, &device);
            }
        }
        dismiss_panels(&mut self.dismiss, &mut shared);
        let was_cal_open = shared.calendar.is_open();
        let calendar_open = shared.calendar.tick();
        let kept: Vec<&str> = devices.iter().map(String::as_str).collect();
        shared.notify.keep_clocks(&kept);
        shared.calendar.keep_clocks(&kept);
        let _ = was_open;
        if shared.calendar.is_open() && !was_cal_open {
            // A month read long ago, or a read that failed, is looked at again.
            shared.live.ask_again();
        }
        if host::take_clock() {
            // The clock jumped or the machine woke: the top bars were woken by the host window;
            // the notifications' "4 min" redraw too, and the calendar is read again.
            shared.live.refresh();
            self.wake.tops();
            if notify_open {
                ctx.request_repaint_of(notify::id());
            }
            if calendar_open {
                ctx.request_repaint_of(calpanel::id());
            }
        }
        // The badge in the top bars' clock follows the notifications.
        let badge = shared.notify.badge();
        if badge != self.badge {
            self.badge = badge;
            self.wake.tops();
        }
        // The preview panel and the notifications window are woken only while they exist: a
        // repaint asked for a viewport nobody declares makes egui open a blank default window
        // for it ("egui window"), which then spins the event loop.
        let (tops, docks) = shared.bars.views().partition::<Vec<_>, _>(|(v, _, _)| v.kind == Kind::Top);
        self.wake.set(tops.iter().map(|(v, _, _)| v.id).collect(), docks.iter().map(|(v, _, _)| v.id).collect());
        self.wake.set_others(shared.preview.current_id(), notify_open.then(notify::id), calendar_open.then(calpanel::id));
        let tray_tops = shared.bars.views().filter(|(v, _, primary)| {
            v.kind == Kind::Top && (*primary || self.settings.tray == TrayMonitors::All)
        });
        self.wake.set_tray(tray_tops.map(|(v, _, _)| v.id).collect());
        self.declare(ctx, &shared, notify_open, calendar_open);
        // New windows exist only after this frame; look again on the next one.
        let dock_style = shared.dock.style();
        let bars_ready = shared.bars.adopt(dock_style);
        let preview_ready = shared.preview.adopt();
        let notify_ready = shared.notify.adopt() && shared.calendar.adopt();
        shared.update_preview();
        if !(bars_ready && preview_ready && notify_ready) {
            ctx.request_repaint();
        }
        shared.bars.settle();
        drop(shared);
        // Bar windows being placed and shown get frames until they all are, and one more.
        let adopting = !bars_ready;
        if wake_all || adopting || std::mem::replace(&mut self.adopting, adopting) {
            self.wake.bars();
        }
        font_atlas::end_frame(ctx);
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.bars.release();
        }
    }
}
