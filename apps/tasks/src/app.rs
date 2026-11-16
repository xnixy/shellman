//! The window: a rail of pages at the left, a title pill on top ("Task Manager / <page>", the
//! search, how often it updates, the window buttons), and the page's islands below it.

use crate::actions::Actions;
use crate::histories::Histories;
use crate::instance::Instance;
use crate::model::List;
use crate::{performance, processes, services};
use egui::{pos2, vec2, Align2, Rect};
use shellman_core::procs::end::Native;
use shellman_core::procs::native::NativeSource;
use shellman_core::procs::sampler::Sampler;
use shellman_core::procs::{Details, Key, System};
use shellman_core::thumbs::Thumbs;
use shellman_ui::appearance::{Appearance, RailApp};
use shellman_ui::chips::{self, Chip};
use shellman_ui::glass::islands::{self, Island};
use shellman_ui::glass::{GlassPanel, Style};
use shellman_ui::rail;
use shellman_ui::theme::{self, colors, glyph, icon_font, semibold_font, AccentChoice};
use std::collections::HashMap;
use std::time::Duration;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindowVisible};

/// Space between the window's edge and the islands, and between islands.
pub const MARGIN: f32 = 12.0;
/// Corner radius of an island.
pub const RADIUS: f32 = 22.0;

const TITLE_H: f32 = 46.0;
const BUTTON_H: f32 = 30.0;
const SEARCH_W: f32 = 240.0;
/// The update speeds offered, and the one to start with.
const INTERVALS: [(u64, &str); 3] = [(500, "0.5 s"), (1000, "1 s"), (2000, "2 s")];
const START_INTERVAL: Duration = Duration::from_millis(1000);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Processes,
    Performance,
    Services,
}

/// One entry of the rail.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RailItem {
    Page(Page),
    /// Opens Settings on its Startup apps page.
    Startup,
}

/// The rail: what each row is, its key, glyph and name.
const RAIL: [(RailItem, &str, char, &str); 4] = [
    (RailItem::Page(Page::Processes), "processes", glyph::VIEW_LIST, "Processes"),
    (RailItem::Page(Page::Performance), "performance", '\u{E9D2}', "Performance"),
    (RailItem::Page(Page::Services), "services", glyph::SETTINGS, "Services"),
    (RailItem::Startup, "startup", glyph::POWER, "Startup apps"),
];

impl Page {
    fn label(self) -> &'static str {
        match self {
            Page::Processes => "Processes",
            Page::Performance => "Performance",
            Page::Services => "Services",
        }
    }
}

/// What the pages share.
pub struct State {
    pub list: List,
    pub actions: Actions,
    pub control: Native,
    pub histories: Histories,
    pub system: System,
    pub thumbs: Thumbs,
    /// The details read for processes, by key, read again when the selection changes.
    pub details: HashMap<Key, Details>,
    pub own_pid: u32,
    /// How long the latest reading took.
    pub took: Duration,
    /// Set by a page to switch pages after the frame.
    pub go_to: Option<Page>,
    /// Set by a page to empty the search box after the frame (it filters another page's rows).
    pub clear_search: bool,
    pub services: services::Page,
}

/// Ending what Windows refuses runs the helper behind the UAC prompt, and wakes the window when
/// it is answered.
fn elevator(ctx: &egui::Context) -> crate::actions::Elevator {
    let wake = ctx.clone();
    std::sync::Arc::new(move |keys| {
        let outcome = shellman_core::procs::end::end_elevated(&keys);
        wake.request_repaint();
        outcome
    })
}

pub struct TasksApp {
    page: Page,
    query: String,
    panel: GlassPanel,
    sampler: Sampler,
    look: Appearance,
    /// Wakes the window when the shared look changes (the glass, or the sidebar's fold).
    look_watch: shellman_core::watch::Watcher,
    st: State,
    _instance: Instance,
    /// The least window size at Windows' text size, and whether the window has been grown to it
    /// (a size restored from a run at a smaller text size is too small).
    min: [f32; 2],
    grown: bool,
}

impl TasksApp {
    pub fn new(ctx: &egui::Context, look: Appearance, panel: GlassPanel, hwnd: isize, instance: Instance, min: [f32; 2]) -> Self {
        let wake = ctx.clone();
        // Nothing is read while the window is minimised or hidden.
        let paused = move || unsafe {
            let h = HWND(hwnd as *mut _);
            hwnd != 0 && (IsIconic(h).as_bool() || !IsWindowVisible(h).as_bool())
        };
        // A delay, however short, asks egui for one frame; `request_repaint` would ask for two.
        let sampler = Sampler::start(NativeSource::new, START_INTERVAL, paused, move || {
            wake.request_repaint_after(Duration::from_millis(1))
        });
        let start_page = match std::env::args().skip_while(|a| a != "--page").nth(1).as_deref() {
            Some("performance") => Page::Performance,
            Some("services") => Page::Services,
            _ => Page::Processes,
        };
        let look_watch = shellman_core::watch::Watcher::quick(ctx.clone());
        if let Some(dir) = shellman_ui::appearance::file().and_then(|f| f.parent().map(|p| p.to_path_buf())) {
            look_watch.set_dirs(vec![dir]);
        }
        Self {
            page: start_page,
            query: String::new(),
            panel,
            sampler,
            look,
            look_watch,
            st: State {
                list: List::default(),
                actions: Actions::new(elevator(ctx)),
                control: Native,
                histories: Histories::default(),
                system: System::default(),
                thumbs: Thumbs::new(ctx.clone()),
                details: HashMap::new(),
                own_pid: std::process::id(),
                took: Duration::ZERO,
                go_to: None,
                clear_search: false,
                services: services::Page::default(),
            },
            _instance: instance,
            min,
            grown: false,
        }
    }

    fn track_system(&self, ctx: &egui::Context) {
        theme::set_light(ctx.theme() == egui::Theme::Light);
        if self.look.accent == AccentChoice::System {
            theme::set_accent(ctx, AccentChoice::System.resolve());
        }
    }

    /// Take the newest reading, if one came.
    fn take_sample(&mut self) {
        let Some(sample) = self.sampler.take() else {
            return;
        };
        self.st.histories.push(&sample.system);
        self.st.system = sample.system;
        self.st.took = sample.took;
        self.st.list.set_procs(sample.procs);
        self.st.actions.follow(&self.st.list);
        let alive = &self.st.list;
        self.st.details.retain(|k, _| alive.index(*k).is_some());
    }

    fn layout(&mut self, ctx: &egui::Context) {
        self.panel.set_style(Style::from_theme(RADIUS));
        let mut list: Vec<Island> = Vec::new();
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
            let screen = ctx.screen_rect();
            let under = ui.painter().add(egui::Shape::Noop);
            islands::drag_gaps(ui, screen);
            let rail_w = rail::width(ui, &rail_entries(), self.rail_mode(screen));
            let left = screen.left() + MARGIN + rail_w + MARGIN;
            let title = Rect::from_min_max(pos2(left, screen.top() + MARGIN), pos2(screen.right() - MARGIN, screen.top() + MARGIN + TITLE_H));
            list.push(Island::pill(title));
            self.title(ui, title);
            let top = title.bottom() + MARGIN;
            // The rail runs the window's whole height, level with the title pill.
            self.rail(ui, &mut list, screen, title.top());
            let area = Rect::from_min_max(pos2(left, top), pos2(screen.right() - MARGIN, screen.bottom() - MARGIN));
            match self.page {
                Page::Processes => processes::show(ui, area, &mut self.st, &mut list),
                Page::Performance => performance::show(ui, area, &mut self.st, &mut list),
                Page::Services => services::show(ui, area, &mut self.st, &mut list),
            }
            if !self.panel.composed() {
                ui.painter().set(under, egui::Shape::Vec(islands::under(screen, &list, &self.panel.style())));
            }
        });
        self.panel.set_islands(&list, ctx.pixels_per_point());
        if std::mem::take(&mut self.st.clear_search) {
            self.query.clear();
        }
        if let Some(page) = self.st.go_to.take() {
            self.page = page;
            ctx.request_repaint();
        }
    }

    /// "Task Manager / <page>", the search (on the Processes page), the update speed and the
    /// window buttons.
    fn title(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let c = colors();
        let y = rect.center().y;
        let name = ui.painter().text(pos2(rect.left() + 22.0, y), Align2::LEFT_CENTER, "Task Manager", semibold_font(15.0), c.text);
        let slash = ui.painter().text(pos2(name.right() + 10.0, y), Align2::LEFT_CENTER, "/", egui::FontId::proportional(15.0), c.muted);
        let label = self.page.label();
        let w = chips::chip_width(ui, label, false, Chip::Accent);
        let chip = Rect::from_center_size(pos2(slash.right() + 10.0 + w / 2.0, y), vec2(w, chips::CHIP_H));
        chips::chip(ui, chip, egui::Id::new("title-chip"), label, None, Chip::Accent);

        let buttons = Rect::from_min_max(pos2(rect.right() - 10.0 - BUTTON_H * 3.0, y - BUTTON_H / 2.0), pos2(rect.right() - 10.0, y + BUTTON_H / 2.0));
        chips::window_buttons(ui, buttons);
        let speed = self.speed_chip(ui, buttons.left() - 12.0, y);
        if matches!(self.page, Page::Processes | Page::Services) {
            let room = speed - 12.0 - (chip.right() + 16.0);
            let width = SEARCH_W.min(room);
            if width > 80.0 {
                let field = Rect::from_min_size(pos2(speed - 12.0 - width, y - chips::CHIP_H / 2.0), vec2(width, chips::CHIP_H));
                self.search(ui, field);
            }
        }
    }

    /// The chip that says how often the numbers update; a click takes the next speed. Returns its
    /// left edge.
    fn speed_chip(&mut self, ui: &mut egui::Ui, right: f32, y: f32) -> f32 {
        let now = self.sampler.interval().as_millis() as u64;
        let at = INTERVALS.iter().position(|(ms, _)| *ms == now).unwrap_or(1);
        let label = format!("Every {}", INTERVALS[at].1);
        let w = chips::chip_width(ui, &label, true, Chip::Soft);
        let rect = Rect::from_min_size(pos2(right - w, y - chips::CHIP_H / 2.0), vec2(w, chips::CHIP_H));
        let resp = chips::chip(ui, rect, egui::Id::new("speed"), &label, Some(glyph::REFRESH), Chip::Soft);
        if resp.on_hover_text("How often the numbers update. Click for the next speed.").clicked() {
            let next = INTERVALS[(at + 1) % INTERVALS.len()].0;
            self.sampler.set_interval(Duration::from_millis(next));
        }
        rect.left()
    }

    /// The search field: a soft pill with a magnifier; Esc clears it.
    fn search(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let c = colors();
        let (soft, _, line) = chips::soft_colors(c.light);
        ui.painter().rect(rect, rect.height() / 2.0, soft, line, egui::StrokeKind::Inside);
        ui.painter().text(pos2(rect.left() + 14.0, rect.center().y), Align2::CENTER_CENTER, glyph::SEARCH, icon_font(11.0), c.muted);
        let inner = Rect::from_min_max(pos2(rect.left() + 26.0, rect.top() + 4.0), pos2(rect.right() - 10.0, rect.bottom() - 4.0));
        let edit = egui::TextEdit::singleline(&mut self.query)
            .hint_text(if self.page == Page::Services { "Search name, description or PID" } else { "Search name, PID or command line" })
            .frame(false)
            .margin(egui::Margin::symmetric(2, 2))
            .vertical_align(egui::Align::Center)
            .font(egui::FontId::proportional(13.0));
        let resp = ui.put(inner, edit);
        shellman_ui::widgets::name_field(&resp, if self.page == Page::Services { "Search services" } else { "Search processes" }, Some(&self.query));
        // Ctrl+F or typing "/" puts the cursor here.
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
            resp.request_focus();
        }
        if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.query.clear();
        }
        self.st.list.set_query(&self.query);
    }

    /// The rail of pages (shared with the other apps): icon and name each, folded to icons by
    /// the user or by a narrow window.
    fn rail(&mut self, ui: &mut egui::Ui, list: &mut Vec<Island>, screen: Rect, top: f32) {
        let solid = (!self.panel.composed()).then(|| self.panel.style());
        let entries = rail_entries();
        let current = RAIL.iter().find(|(item, ..)| *item == RailItem::Page(self.page)).map(|(_, key, ..)| *key);
        let out = rail::show(ui, list, screen, top, &entries, current, self.rail_mode(screen), solid.as_ref(), RADIUS);
        if let Some(i) = out.clicked {
            match RAIL[i].0 {
                RailItem::Page(p) => self.page = p,
                RailItem::Startup => {
                    shellman_core::sibling::start_with("settings", &["--page", "startup"]);
                }
            }
        }
        if out.toggled {
            self.look = Appearance::toggle_rail_saved(RailApp::Tasks, self.look);
        }
    }

    fn rail_mode(&self, screen: Rect) -> rail::Mode {
        rail::mode(screen.width(), self.look.rail_folded(RailApp::Tasks))
    }

    /// Take in the shared look when Settings (or another app's fold) saves it.
    fn follow_look(&mut self, ctx: &egui::Context) {
        if !self.look_watch.poll().is_empty() {
            let look = Appearance::load_or(self.look);
            if look != self.look {
                self.look = look;
                look.apply(ctx);
            }
        }
    }
}

/// The rail's rows, in `RAIL`'s order.
fn rail_entries() -> Vec<rail::Entry<'static>> {
    RAIL.iter()
        .map(|(item, key, g, label)| match item {
            // It leaves for Settings, which the pointer is told before it goes.
            RailItem::Startup => rail::Row::new(*key, rail::Icon::Glyph(*g), *label).tip("Startup apps (in Settings)").into(),
            RailItem::Page(_) => rail::Entry::item(key, *g, label),
        })
        .collect()
}
impl eframe::App for TasksApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.st.thumbs.poll(ctx);
        if !self.grown {
            self.grown = theme::env::grow_to(ctx, self.min);
        }
        self.follow_look(ctx);
        self.track_system(ctx);
        shellman_ui::app_icon::sync_window_icon(ctx, shellman_ui::app_icon::Kind::Shellman);
        self.take_sample();
        self.st.actions.poll();
        if self.page == Page::Services {
            // The services are read only while their page shows, and not while minimised.
            let minimised = ctx.input(|i| i.viewport().minimized == Some(true));
            self.st.services.tick(ctx, &self.query, self.sampler.interval(), minimised);
        }
        self.layout(ctx);
    }
}
