//! The window: a rail of pages at the left, a title pill on top ("Task Manager / <page>", the
//! search, how often it updates, the window buttons), and the page's islands below it.

use crate::actions::Actions;
use crate::histories::Histories;
use crate::instance::Instance;
use crate::model::List;
use crate::{performance, processes};
use egui::{pos2, vec2, Align2, Rect, Sense};
use shellman_core::procs::end::Native;
use shellman_core::procs::native::NativeSource;
use shellman_core::procs::sampler::Sampler;
use shellman_core::procs::{Details, Key, System};
use shellman_core::thumbs::Thumbs;
use shellman_ui::appearance::Appearance;
use shellman_ui::chips::{self, Chip};
use shellman_ui::glass::islands::{self, Island};
use shellman_ui::glass::{GlassPanel, Style};
use shellman_ui::theme::{self, colors, glyph, icon_font, semibold_font, AccentChoice};
use std::collections::HashMap;
use std::time::Duration;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindowVisible};

/// Space between the window's edge and the islands, and between islands.
pub const MARGIN: f32 = 12.0;
/// Corner radius of an island.
pub const RADIUS: f32 = 22.0;
const RAIL_W: f32 = 56.0;
const RAIL_ICON: f32 = 34.0;
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
}

/// One entry of the rail.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RailItem {
    Page(Page),
    /// Opens Settings on its Startup apps page.
    Startup,
}

const RAIL: [(RailItem, char, &str); 3] = [
    (RailItem::Page(Page::Processes), glyph::VIEW_LIST, "Processes"),
    (RailItem::Page(Page::Performance), '\u{E9D2}', "Performance"),
    (RailItem::Startup, glyph::POWER, "Startup apps (in Settings)"),
];

impl Page {
    fn label(self) -> &'static str {
        match self {
            Page::Processes => "Processes",
            Page::Performance => "Performance",
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
    st: State,
    _instance: Instance,
}

impl TasksApp {
    pub fn new(ctx: &egui::Context, look: Appearance, panel: GlassPanel, hwnd: isize, instance: Instance) -> Self {
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
            _ => Page::Processes,
        };
        Self {
            page: start_page,
            query: String::new(),
            panel,
            sampler,
            look,
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
            },
            _instance: instance,
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
            let left = screen.left() + MARGIN + RAIL_W + MARGIN;
            let title = Rect::from_min_max(pos2(left, screen.top() + MARGIN), pos2(screen.right() - MARGIN, screen.top() + MARGIN + TITLE_H));
            list.push(Island::pill(title));
            self.title(ui, title);
            let top = title.bottom() + MARGIN;
            self.rail(ui, &mut list, screen, top);
            let area = Rect::from_min_max(pos2(left, top), pos2(screen.right() - MARGIN, screen.bottom() - MARGIN));
            match self.page {
                Page::Processes => processes::show(ui, area, &mut self.st, &mut list),
                Page::Performance => performance::show(ui, area, &mut self.st, &mut list),
            }
            if !self.panel.composed() {
                ui.painter().set(under, egui::Shape::Vec(islands::solid_shapes(&list, &self.panel.style())));
            }
        });
        self.panel.set_islands(&list, ctx.pixels_per_point());
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
        if self.page == Page::Processes {
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
            .hint_text("Search name, PID or command line")
            .frame(false)
            .margin(egui::Margin::symmetric(2, 2))
            .vertical_align(egui::Align::Center)
            .font(egui::FontId::proportional(13.0));
        let resp = ui.put(inner, edit);
        // Ctrl+F or typing "/" puts the cursor here.
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
            resp.request_focus();
        }
        if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.query.clear();
        }
        self.st.list.set_query(&self.query);
    }

    /// The pill of pages: one icon each, the current one filled with the accent.
    fn rail(&mut self, ui: &mut egui::Ui, list: &mut Vec<Island>, screen: Rect, top: f32) {
        let c = colors();
        let n = RAIL.len() as f32;
        let wanted = 28.0 + n * RAIL_ICON + (n - 1.0) * 4.0;
        let height = wanted.min(screen.bottom() - MARGIN - top);
        let rail = Rect::from_min_size(pos2(screen.left() + MARGIN, top), vec2(RAIL_W, height));
        list.push(Island::pill(rail));
        let (_, hover, _) = chips::soft_colors(c.light);
        let mut y = rail.top() + 14.0;
        for (item, g, tip) in RAIL {
            let cell = Rect::from_min_size(pos2(rail.center().x - RAIL_ICON / 2.0, y), vec2(RAIL_ICON, RAIL_ICON));
            y += RAIL_ICON + 4.0;
            if cell.bottom() > rail.bottom() {
                break;
            }
            let resp = ui.interact(cell, egui::Id::new(("rail", tip)), Sense::click());
            let current = item == RailItem::Page(self.page);
            let (fill, fg) = if current {
                (c.accent_fill, c.on_accent)
            } else if resp.hovered() {
                (hover, c.text)
            } else {
                (egui::Color32::TRANSPARENT, c.muted)
            };
            ui.painter().rect_filled(cell, 12.0, fill);
            ui.painter().text(cell.center(), Align2::CENTER_CENTER, g, icon_font(16.0), fg);
            if resp.on_hover_text(tip).clicked() {
                match item {
                    RailItem::Page(p) => self.page = p,
                    RailItem::Startup => {
                        shellman_core::sibling::start_with("settings", &["--page", "startup"]);
                    }
                }
            }
        }
    }
}

impl eframe::App for TasksApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.st.thumbs.poll(ctx);
        self.track_system(ctx);
        shellman_ui::app_icon::sync_window_icon(ctx, shellman_ui::app_icon::Kind::Shellman);
        self.take_sample();
        self.st.actions.poll();
        self.layout(ctx);
    }
}
