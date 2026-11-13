//! The shelf process's windows and their frames: the primary monitor's shelf in the first
//! window and, with "All displays", one per other monitor (`views`), all showing the same
//! stacks. One fan is open at a time, on the shelf it was opened from.
//!
//! Frames run only when something happened: the pointer over the shelf, a folder that changed, a
//! thumbnail that arrived, the settings, the desktop asking for its menu. At rest nothing asks
//! for a frame, so the process sleeps. Nothing animates at rest; a fan opening or closing asks
//! for frames for its 160 ms and stops.

use crate::draw::{self, Event, FanView, Scene, StackView};
use crate::drop;
use crate::folders::Folders;
use crate::geometry;
use crate::menu::{self, Action};
use crate::stacks::{self, Stacks};
use crate::views::{self, View};
use crate::{actions, displays, log, wallpaper, window};
use shellman_core::outside;
use shellman_core::settings::desktop::{DesktopSettings, ShelfOn, Stack};
use shellman_core::thumbs::Thumbs;
use shellman_core::transfer::{self, COPY, LINK, MOVE};
use shellman_core::watch::Watcher;
use shellman_core::winlist::WindowWatcher;
use shellman_ui::appearance::Appearance;
use shellman_ui::glass::{self, GlassPanel, Island, Style};
use shellman_ui::menu::popup::{self, Anchor, PopupMenu};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::HWND;
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
    _windows: WindowWatcher,
}

impl Shelf {
    pub fn new(ctx: &egui::Context, hwnd: isize, panel: GlassPanel, look: Appearance, drop: Arc<Mutex<drop::Shared>>) -> Self {
        let config_watch = Watcher::quick(ctx.clone());
        config_watch.set_dirs(shellman_core::config::config_dir().into_iter().collect());
        let windows = WindowWatcher::start(window::request_check);
        windows.on_front_moved(window::request_check);
        let folders = Folders::resolve();
        log::line(format!("started; folders {folders:?}"));
        Self {
            hwnd,
            current: egui::ViewportId::ROOT,
            views: vec![View::first(hwnd, panel, drop)],
            laid_out: false,
            look,
            config_watch,
            settings: DesktopSettings::load(),
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
            _windows: windows,
        }
    }

    /// Look at the monitors and bring the windows in line: which monitors have a shelf (the
    /// setting; the primary alone while the shelf is off), each window on its monitor's edge.
    fn lay_out(&mut self, ctx: &egui::Context) {
        let monitors = shellman_core::monitors::all();
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
            Action::DesktopSettings => actions::settings("desktop"),
            Action::DisplaySettings => actions::settings("display"),
        }
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

    /// The shelf's frame in the window of `self.views[view]`: the islands to show this time.
    fn shelf_frame(&mut self, ctx: &egui::Context, view: usize, placed: displays::Placement) -> Vec<Island> {
        let stacks = self.settings.stacks();
        let window = ctx.screen_rect();
        let layout = geometry::layout(window, placed.area, self.settings.shelf.edge, stacks.len());
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
        let pills = match (fan_kind, &fan_items) {
            (Some(k), Some((shown, _))) => {
                let i = stacks.iter().position(|s| *s == k).unwrap_or(0);
                let anchor = layout.stacks[i].center().y;
                geometry::fan_at(layout.shelf, anchor, shown.len(), self.settings.shelf.edge, placed.area, eased)
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
                let events = draw::paint(ui, &scene, &mut self.thumbs);
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
            d.zone = Some(shelf_px);
            d.dest = self.folders.drop_target(&self.settings.shelf.drop_to).map(|p| {
                let name = self.settings.shelf.drop_to.label();
                (p, name)
            });
        }
        islands
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
        if self.views[view].placed.is_none() {
            let (edge, stacks) = (self.settings.shelf.edge, self.settings.stacks().len());
            self.views[view].place(edge, stacks);
        }
        let shown = self.settings.shelf_shown() && !window::covered(hwnd) && self.views[view].monitor.is_some();
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
        let islands = match (shown, placed) {
            (true, Some(p)) => self.shelf_frame(ctx, view, p),
            _ => {
                if self.views[view].placed.is_some() && placed.is_none() {
                    ctx.request_repaint();
                }
                if let Ok(mut d) = self.views[view].drop.lock() {
                    d.zone = None;
                }
                Vec::new()
            }
        };
        if self.dragging && self.drag_on.is_none() {
            self.drag_on = Some(id);
        }
        if let Some(panel) = &self.views[view].panel {
            panel.set_style(Style::from_theme(geometry::RADIUS));
            // Only the islands show and take clicks; the rest of the window lets the desktop through.
            panel.set_islands_only(&islands, ctx.pixels_per_point(), None);
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
        self.stacks.set_active(self.settings.shelf_shown(), &self.folders);
        self.stacks.poll();
        self.thumbs.poll(ctx);
        // The glass menu's window, once there has been a menu.
        popup::declare(ctx);
        // The desktop asked for its menu: an open fan closes first.
        if let Some(at) = window::take_menu() {
            self.close_fan(false);
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
            });
        }
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
