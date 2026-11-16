//! The shelf process's windows and their frames: the primary monitor's shelf in the first
//! window and, with "All displays", one per other monitor (`views`), all showing the same
//! stacks. One fan is open at a time, on the shelf it was opened from.
//!
//! Frames run only when something happened: the pointer over the shelf, a folder that changed, a
//! thumbnail that arrived, the settings, the desktop asking for its menu. At rest nothing asks
//! for a frame, so the process sleeps. Nothing animates at rest; a fan opening or closing asks
//! for frames for its 160 ms and stops.

use crate::arrange::{self, bar::{self, Bar}, place as arrange_place, Item};
use crate::draw::{self, Event, FanView, Scene, StackView};
use crate::drop;
use crate::folders::Folders;
use crate::geometry;
use crate::menu::{self, Action};
use crate::stacks::{self, Stacks};
use crate::views::{self, View};
use crate::widgets::Widgets;
use crate::{actions, displays, log, wallpaper, window};
use shellman_core::outside;
use shellman_core::monitors::Monitor;
use shellman_core::settings::desktop::{DesktopSettings, Edge, ShelfOn, Stack};
use shellman_core::thumbs::Thumbs;
use shellman_core::transfer::{self, COPY, LINK, MOVE};
use shellman_core::watch::Watcher;
use shellman_core::winlist::WindowWatcher;
use shellman_ui::appearance::Appearance;
use shellman_ui::glass::{self, GlassPanel, Island, Style};
use shellman_ui::menu::popup::{self, Anchor, PopupMenu};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};

/// How long a fan takes to open or close.
const FAN_TIME: f32 = 0.16;

pub struct Shelf {
    /// The window whose frame runs now (the first window's outside them).
    hwnd: isize,
    /// That window's viewport.
    current: egui::ViewportId,
    /// Every shelf window, the first window first.
    views: Vec<View>,
    /// The monitors have been looked at.
    laid_out: bool,
    look: Appearance,
    /// The settings folder: the look and `desktop.json`.
    config_watch: Watcher,
    settings: DesktopSettings,
    folders: Folders,
    stacks: Stacks,
    thumbs: Thumbs,
    /// The stack whose fan is open.
    open: Option<Stack>,
    /// The stack whose fan is drawn: the open one, or the one still closing.
    drawn: Option<Stack>,
    /// The window (viewport and handle) the fan is open or closing on.
    fan_on: Option<(egui::ViewportId, isize)>,
    /// The window in front before a fan took the focus, to give it back.
    before: Option<isize>,
    /// The fan had the focus at some point (so losing it means closing).
    fan_focused: bool,
    /// A drag out of the fan is on.
    dragging: bool,
    /// The window the drag started from.
    drag_on: Option<egui::ViewportId>,
    /// The desktop's right-click menu.
    menu: PopupMenu<Action>,
    /// Today and Now playing, top right.
    widgets: Widgets,
    /// Arrange mode's bar (Reset, Cancel, Done).
    bar: Bar,
    /// Where the desktop last asked for its menu (arrange mode's bar goes on that monitor).
    menu_at: Option<(i32, i32)>,
    /// Start in arrange mode once the windows are laid out (`--arrange`).
    arrange_at_start: bool,
    _windows: WindowWatcher,
}

impl Shelf {
    pub fn new(ctx: &egui::Context, hwnd: isize, panel: GlassPanel, look: Appearance, drop: Arc<Mutex<drop::Shared>>, arrange_at_start: bool) -> Self {
        let config_watch = Watcher::quick(ctx.clone());
        config_watch.set_dirs(shellman_core::config::config_dir().into_iter().collect());
        let windows = WindowWatcher::start(window::request_check);
        windows.on_front_moved(window::request_check);
        let folders = Folders::resolve();
        log::line(format!("started; folders {folders:?}"));
        let settings = DesktopSettings::load();
        let mut widgets = Widgets::new();
        widgets.apply(&settings, ctx);
        Self {
            hwnd,
            current: egui::ViewportId::ROOT,
            views: vec![View::first(hwnd, panel, drop)],
            laid_out: false,
            look,
            config_watch,
            settings,
            folders,
            stacks: Stacks::new(ctx),
            thumbs: Thumbs::new(ctx.clone()),
            open: None,
            drawn: None,
            fan_on: None,
            before: None,
            fan_focused: false,
            dragging: false,
            drag_on: None,
            menu: PopupMenu::new(),
            widgets,
            bar: Bar::new(),
            menu_at: None,
            arrange_at_start,
            _windows: windows,
        }
    }

    /// Look at the monitors and bring the windows in line: which monitors have a shelf (the
    /// setting; the primary alone while the shelf is off), each window on its monitor's edge.
    fn lay_out(&mut self, ctx: &egui::Context) {
        let monitors = crate::test_monitor::restrict(shellman_core::monitors::all());
        let on = if self.settings.shelf_shown() { self.settings.shelf.on } else { ShelfOn::Primary };
        let (primary, others) = displays::split(displays::targets(on, &monitors));
        let names: Vec<String> = monitors
            .iter()
            .map(|m| {
                let r = m.rect;
                format!("{} {}x{} at {},{} {}%", m.device, r.right - r.left, r.bottom - r.top, r.left, r.top, (m.scale * 100.0).round())
            })
            .collect();
        log::line(format!("monitors: {}; shelves on {on:?}", names.join("; ")));
        views::sync(&mut self.views, primary, others, ctx);
        self.widgets.lay_out(&monitors);
        arrange::forget_pieces();
        self.bar.follow(&monitors);
        self.laid_out = true;
        // Whether a fullscreen app covers each window's monitor.
        window::request_check();
    }

    fn reload(&mut self, ctx: &egui::Context) {
        let look = Appearance::load_or(self.look);
        if look != self.look {
            self.look = look;
            look.apply(ctx);
        }
        let settings = DesktopSettings::load();
        if settings != self.settings {
            let moved = settings.shelf.edge != self.settings.shelf.edge;
            // Which monitors have a shelf, and how tall each window is.
            if self.widgets.apply(&settings, ctx) {
                window::ask_layout();
            }
            let relaid = settings.shelf.on != self.settings.shelf.on
                || settings.shelf_shown() != self.settings.shelf_shown()
                || settings.stacks().len() != self.settings.stacks().len();
            self.settings = settings;
            if moved {
                self.close_fan(false);
            }
            if moved || relaid {
                window::ask_layout();
            }
        }
    }

    fn open_fan(&mut self, stack: Stack, ctx: &egui::Context) {
        // One fan at a time: one open on another monitor's shelf closes there.
        let switching = self.open.is_some();
        if let Some((view, hwnd)) = self.fan_on {
            if view != self.current {
                window::set_raised(hwnd, false);
            }
        }
        self.fan_on = Some((self.current, self.hwnd));
        // The click that opened this fan is no click outside it.
        let _ = outside::take_click();
        self.open = Some(stack);
        self.drawn = Some(stack);
        self.fan_focused = false;
        window::set_raised(self.hwnd, true);
        let ctx = ctx.clone();
        outside::start(move || ctx.request_repaint_of(egui::ViewportId::ROOT));
        // The focus, for Esc (the click on the shelf lets this process take it).
        unsafe {
            let front = GetForegroundWindow();
            if front.0 as isize != self.hwnd && !switching {
                self.before = (!front.is_invalid()).then_some(front.0 as isize);
            }
            let _ = SetForegroundWindow(HWND(self.hwnd as *mut _));
        }
    }

    /// Close the open fan. With `give_back`, the window that had the focus before gets it back,
    /// if the shelf still has it (a click on another window has already moved it there).
    fn close_fan(&mut self, give_back: bool) {
        if self.open.take().is_none() {
            return;
        }
        outside::stop();
        let before = self.before.take();
        let fan = self.fan_on.map_or(self.hwnd, |(_, h)| h);
        unsafe {
            if give_back && GetForegroundWindow().0 as isize == fan {
                if let Some(b) = before {
                    let _ = SetForegroundWindow(HWND(b as *mut _));
                }
            }
        }
    }

    fn act(&mut self, event: Event, ctx: &egui::Context) {
        match event {
            Event::Toggle(s) if self.open == Some(s) => self.close_fan(true),
            Event::Toggle(s) => self.open_fan(s, ctx),
            Event::Open(path) => {
                self.close_fan(false);
                actions::open(&path);
            }
            Event::OpenFolder(s) => {
                self.close_fan(false);
                if let Some(dir) = self.stacks.folder(s) {
                    actions::open_in_files(&dir);
                }
            }
            Event::DragOut(path) => {
                if !self.dragging {
                    self.dragging = true;
                    let image = crate::drag_image::render(ctx, &path, ctx.pixels_per_point());
                    log::line(format!("drag out {} (picture {})", path.display(), image.is_some()));
                    let ctx = ctx.clone();
                    transfer::drag::start(self.hwnd, vec![path], image, COPY | MOVE | LINK, move |e| {
                        log::line(format!("drag ended: effect {} cancelled {}", e.effect, e.cancelled));
                        ctx.request_repaint()
                    });
                }
            }
        }
    }

    fn run(&mut self, action: Action) {
        let desktop = self.folders.desktop.clone();
        let Some(dir) = desktop else {
            log::line("no Desktop folder");
            return;
        };
        match action {
            Action::NewFolder | Action::NewText | Action::NewNote => {
                let what = match action {
                    Action::NewFolder => actions::New::Folder,
                    Action::NewText => actions::New::Text,
                    _ => actions::New::Note,
                };
                match actions::create(&dir, what) {
                    Ok(p) => log::line(format!("new {}", p.display())),
                    Err(e) => log::line(format!("new {what:?} in {}: {e}", dir.display())),
                }
                self.stacks.refresh(Stack::Desktop);
            }
            Action::Paste => {
                std::thread::spawn(move || actions::paste(&dir));
            }
            Action::OpenInFiles => actions::open_in_files(&dir),
            Action::Terminal => {
                std::thread::spawn(move || match shellman_core::terminal::open_at(&dir) {
                    Ok(how) => log::line(format!("terminal: {how} in {}", dir.display())),
                    Err(e) => log::line(format!("terminal: {e}")),
                });
            }
            Action::Wallpaper => {
                std::thread::spawn(wallpaper::change);
            }
            Action::EditDesktop => self.begin_arrange(),
            Action::DesktopSettings => actions::settings("desktop"),
            Action::DisplaySettings => actions::settings("display"),
        }
    }

    /// The shelf's edge now: arrange mode's while arranging.
    fn edge(&self) -> Edge {
        arrange::edge(&self.settings)
    }

    /// Start arrange mode, its bar on the monitor the menu was opened on (or the primary).
    fn begin_arrange(&mut self) {
        if arrange::active() {
            return;
        }
        self.close_fan(false);
        let monitors = crate::test_monitor::restrict(shellman_core::monitors::all());
        let at = self.menu_at;
        let on: Option<Monitor> = at
            .and_then(|(x, y)| monitors.iter().find(|m| m.rect.left <= x && x < m.rect.right && m.rect.top <= y && y < m.rect.bottom))
            .or_else(|| monitors.iter().find(|m| m.primary))
            .or(monitors.first())
            .cloned();
        let Some(on) = on else {
            log::line("arrange: no monitor");
            return;
        };
        log::line(format!("arrange: started, bar on {}", on.device));
        arrange::begin(&self.settings);
        self.bar.show(on);
    }

    /// What the bar asked for.
    fn arrange_action(&mut self, action: bar::Action, ctx: &egui::Context) {
        match action {
            bar::Action::Reset => {
                log::line("arrange: reset");
                arrange::reset();
            }
            bar::Action::Cancel => {
                log::line("arrange: cancelled");
                arrange::cancel();
                self.bar.hide();
            }
            bar::Action::Done => {
                self.bar.hide();
                let Some((arranged, edge)) = arrange::end() else { return };
                // Saved over what is in the file now (Settings may have changed something since).
                let mut s = DesktopSettings::try_load().unwrap_or_else(|_| self.settings.clone());
                s.arrange = arranged;
                s.shelf.edge = edge;
                s.save();
                log::line(format!("arrange: saved {} monitor(s), shelf on the {edge:?} edge", s.arrange.len()));
                // What the file watcher will read back, so it changes nothing.
                self.widgets.apply(&s, ctx);
                self.settings = s;
            }
        }
        ctx.request_repaint();
    }

    /// The local offset from UTC now, in seconds.
    fn offset() -> i64 {
        use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
        let mut tz = TIME_ZONE_INFORMATION::default();
        let id = unsafe { GetTimeZoneInformation(&mut tz) };
        // 2: daylight saving time is in effect.
        let bias = tz.Bias + if id == 2 { tz.DaylightBias } else { tz.StandardBias };
        -(bias as i64) * 60
    }

    /// The shelf's frame in the window of `self.views[view]`: the islands to show this time,
    /// and while a fan is on its way open or shut, the part of the window it moves in (in the
    /// window's pixels). That part shows and takes clicks as a whole meanwhile, so the window's
    /// region holds still while the pills move in it; cutting it anew around every pill on
    /// every frame made Windows clip and redraw the window mid-animation (slow, and the pills
    /// tore).
    fn shelf_frame(&mut self, ctx: &egui::Context, view: usize, placed: displays::Placement) -> (Vec<Island>, Option<RECT>) {
        let stacks = self.settings.stacks();
        let window = ctx.screen_rect();
        let edge = self.edge();
        let layout = geometry::layout_at(window, placed.shelf_top, edge, stacks.len());
        let arranging = arrange::active();
        // The fan eases open and shut, on the shelf it was opened from.
        let mine = self.fan_on.is_some_and(|(v, _)| v == self.current);
        let t = ctx.animate_bool_with_time(egui::Id::new("fan").with(self.current), mine && self.open.is_some(), FAN_TIME);
        if mine && t <= 0.0 && self.open.is_none() && self.drawn.take().is_some() {
            window::set_raised(self.hwnd, false);
            self.fan_on = None;
        }
        let eased = 1.0 - (1.0 - t).powi(3);
        let fan_kind = if mine { self.drawn.filter(|k| stacks.contains(k)) } else { None };
        let fan_items = fan_kind.map(|k| stacks::fan(self.stacks.items(k), geometry::FAN_ITEMS));
        let mut moving = None;
        let pills = match (fan_kind, &fan_items) {
            (Some(k), Some((shown, _))) => {
                let i = stacks.iter().position(|s| *s == k).unwrap_or(0);
                let anchor = layout.stacks[i].center().y;
                let at = |t: f32| geometry::fan_at(layout.shelf, anchor, shown.len(), edge, placed.area, t);
                if t < 1.0 {
                    moving = geometry::swept(&at(0.0), &at(1.0), ctx.pixels_per_point());
                }
                at(eased)
            }
            _ => Vec::new(),
        };
        let drop = self.views[view].drop.clone();
        let keep = drop.lock().map(|d| d.hovering).unwrap_or(false);
        let now_utc = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        let offset = Self::offset();
        let mut islands = vec![Island::new(layout.shelf, geometry::RADIUS)];
        let opacity = eased.clamp(0.0, 1.0);
        islands.extend(pills.iter().map(|r| {
            let mut i = Island::pill(*r);
            i.opacity = opacity;
            i
        }));
        let style = Style::from_theme(geometry::RADIUS);
        let composed = self.views[view].panel.as_ref().is_some_and(GlassPanel::composed);
        let events = egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ctx, |ui| {
                let under = ui.painter().add(egui::Shape::Noop);
                let scene = Scene {
                    layout: &layout,
                    stacks: stacks.iter().map(|k| StackView { kind: *k, items: self.stacks.items(*k) }).collect(),
                    fan: match (fan_kind, fan_items) {
                        (Some(kind), Some((items, more))) => Some(FanView { kind, items, more, pills: pills.clone(), t: opacity }),
                        _ => None,
                    },
                    keep,
                    now: now_utc + offset,
                    offset,
                };
                let mut events = draw::paint(ui, &scene, &mut self.thumbs);
                if arranging {
                    // A piece to drag, not to use: on top of the stacks, which do nothing now.
                    events.clear();
                    if let Some(m) = self.views[view].monitor.clone() {
                        let key = arrange_place::key(&m);
                        let resp = ui
                            .interact(layout.shelf, egui::Id::new("arrange shelf").with(self.current), egui::Sense::drag())
                            .on_hover_cursor(egui::CursorIcon::Grab);
                        let a = arrange::arrangement(&self.settings, &key);
                        let rect = arrange_place::shelf_rect(arrange_place::work(&m), edge, stacks.len(), a.shelf);
                        if resp.drag_started() {
                            arrange::start_drag(&key, Item::Shelf, rect, m.scale);
                        }
                        arrange::ring(ui.painter(), layout.shelf, geometry::RADIUS, arrange::is_selected(&key, Item::Shelf));
                        arrange::report(arrange::Piece {
                            key,
                            item: Item::Shelf,
                            display: arrange_place::display_name(&m),
                            rect,
                            work: arrange_place::work(&m),
                        });
                    }
                }
                if !composed {
                    ui.painter().set(under, egui::Shape::Vec(glass::islands::solid_shapes(&islands, &style)));
                }
                events
            })
            .inner;
        for e in events {
            self.act(e, ctx);
        }
        // What counts as the shelf for drops and for clicks outside an open fan.
        let origin = (placed.rect.left, placed.rect.top);
        let scale = ctx.pixels_per_point();
        let shelf_px = geometry::rect_to_screen(layout.shelf, origin, scale);
        if mine {
            let mut inside = vec![shelf_px];
            inside.extend(pills.iter().map(|r| geometry::rect_to_screen(*r, origin, scale)));
            outside::set_rects(&inside);
        }
        if let Ok(mut d) = drop.lock() {
            // Nothing is dropped on a shelf being arranged.
            d.zone = (!arranging).then_some(shelf_px);
            d.dest = self.folders.drop_target(&self.settings.shelf.drop_to).map(|p| {
                let name = self.settings.shelf.drop_to.label();
                (p, name)
            });
        }
        (islands, moving.map(|(left, top, right, bottom)| RECT { left, top, right, bottom }))
    }
}

impl Shelf {
    /// One shelf window's frame, in its own viewport's `ctx`.
    fn view_frame(&mut self, ctx: &egui::Context, view: usize) {
        crate::theme_for(ctx);
        if !self.views[view].adopt() {
            // Made at the end of this frame: look again in the next.
            ctx.request_repaint();
            return;
        }
        let Some(hwnd) = self.views[view].hwnd else { return };
        let id = self.views[view].id;
        self.hwnd = hwnd;
        self.current = id;
        let arranging = arrange::active();
        let stacks = self.settings.stacks().len();
        if let (true, Some(m)) = (arranging, self.views[view].monitor.clone()) {
            let key = arrange_place::key(&m);
            if let Some(moved) = arrange::dragged(&key, Item::Shelf) {
                let work = arrange_place::work(&m);
                let h = geometry::shelf_height(stacks);
                let top = arrange_place::snap_shelf(moved.top(), h, work, &arrange::others(&key, Item::Shelf));
                arrange::edit(&key, |a| a.shelf = Some(arrange_place::pin_for(top, work.top(), work.bottom(), h)));
                arrange::set_edge(arrange_place::edge_at(arrange::cursor_on(&m).x, work));
                ctx.request_repaint();
            }
        }
        let pin = self.views[view].monitor.as_ref().and_then(|m| arrange::arrangement(&self.settings, &arrange_place::key(m)).shelf);
        let want = (self.edge(), stacks, pin);
        if self.views[view].placed.is_none() || self.views[view].placed_for != Some(want) {
            self.views[view].place(want.0, want.1, want.2);
        }
        // Above the apps while arranging or while its fan is open; at desktop level otherwise.
        if arranging {
            window::set_raised(hwnd, true);
        } else if !self.fan_on.is_some_and(|(v, _)| v == id) {
            window::set_raised(hwnd, false);
        }
        let shown = self.settings.shelf_shown() && (arranging || !window::covered(hwnd)) && self.views[view].monitor.is_some();
        if self.dragging && self.drag_on == Some(id) && !ctx.input(|i| i.pointer.any_down()) {
            self.dragging = false;
            self.drag_on = None;
        }
        if self.open.is_some() && self.fan_on.is_some_and(|(v, _)| v == id) {
            let focused = ctx.input(|i| i.viewport().focused).unwrap_or(false);
            self.fan_focused |= focused;
            let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
            if esc {
                self.close_fan(true);
            } else if outside::take_click() || (self.fan_focused && !focused && !self.dragging) || !shown {
                self.close_fan(false);
            }
        }
        // The window must sit at the scale of the monitor it was placed on before the shelf is
        // laid out in its points; a frame at the old scale draws nothing.
        let placed = self.views[view].placed.filter(|p| (p.scale - ctx.pixels_per_point()).abs() < 0.01);
        let (islands, moving) = match (shown, placed) {
            (true, Some(p)) => self.shelf_frame(ctx, view, p),
            _ => {
                if self.views[view].placed.is_some() && placed.is_none() {
                    ctx.request_repaint();
                }
                if let Ok(mut d) = self.views[view].drop.lock() {
                    d.zone = None;
                }
                (Vec::new(), None)
            }
        };
        if self.dragging && self.drag_on.is_none() {
            self.drag_on = Some(id);
        }
        if let Some(panel) = &self.views[view].panel {
            panel.set_style(Style::from_theme(geometry::RADIUS));
            // Only the islands show and take clicks; the rest of the window lets the desktop through.
            panel.set_islands_only(&islands, ctx.pixels_per_point(), moving);
        }
    }
}

impl eframe::App for Shelf {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        crate::theme_for(ctx);
        if !self.config_watch.poll().is_empty() {
            self.reload(ctx);
        }
        if window::take_layout() || !self.laid_out {
            self.lay_out(ctx);
        }
        if std::mem::take(&mut self.arrange_at_start) {
            self.begin_arrange();
        }
        self.stacks.set_active(self.settings.shelf_shown(), &self.folders);
        self.stacks.poll();
        self.thumbs.poll(ctx);
        // The glass menu's window, once there has been a menu.
        popup::declare(ctx);
        // The desktop asked for its menu: an open fan closes first.
        if let Some(at) = window::take_menu() {
            self.close_fan(false);
            self.menu_at = Some(at);
            let can_paste = transfer::clip::state().can_paste();
            log::line(format!("menu at {at:?}"));
            self.menu.open(ctx, Anchor::at(at.0, at.1), menu::popup_entries(&menu::entries(can_paste)));
        }
        if let Some(action) = self.menu.take_pick() {
            log::line(format!("menu: {action:?}"));
            self.run(action);
        }
        // The first window (the primary's shelf), then every other monitor's, drawn now with it.
        self.view_frame(ctx, 0);
        for view in 1..self.views.len() {
            let (id, builder) = (self.views[view].id, self.views[view].builder());
            ctx.show_viewport_immediate(id, builder, |ctx, _| {
                self.view_frame(ctx, view);
                shellman_ui::font_atlas::end_frame(ctx);
                // The first shelf is the root window (eframe's own screen reader adapter); these
                // others get theirs here.
                shellman_ui::access::publish(ctx);
            });
        }
        if let Some(action) = self.bar.frame(ctx) {
            self.arrange_action(action, ctx);
        }
        if arrange::dragging() {
            ctx.request_repaint();
        }
        self.widgets.frame(ctx);
        self.hwnd = self.views[0].hwnd.unwrap_or(self.hwnd);
        self.current = egui::ViewportId::ROOT;
        let dropped: Vec<_> = self
            .views
            .iter()
            .flat_map(|v| v.drop.lock().map(|mut d| std::mem::take(&mut d.dropped)).unwrap_or_default())
            .collect();
        if !dropped.is_empty() {
            if let Some(dir) = self.folders.drop_target(&self.settings.shelf.drop_to) {
                std::thread::spawn(move || {
                    for (payload, effect) in dropped {
                        actions::dropped(payload, &dir, effect);
                    }
                });
            }
        }
        shellman_ui::font_atlas::end_frame(ctx);
    }
}
