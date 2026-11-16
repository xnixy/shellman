//! The calendar panel: one glass island hanging under a top bar's clock island, on the monitor
//! whose clock was clicked (or whose "now" pill asked for an event's day). The month grid, the
//! chosen day's events, a quiet line to connect a calendar when none is (see `month`).
//!
//! The window is made the first time the panel opens and kept from then on: between uses it is
//! parked, cloaked, with no island (nothing shows, every click goes through), never hidden and
//! never resized to nothing. Its size is the island's plus room for its shadow (the island has
//! one height), so it is set once per monitor scale; it passes `glass::guard` first.

pub mod month;

use crate::notify::layout::{self, Area, EDGE, UNDER};
use crate::notify::target::Clock;
use crate::shape::surface;
use crate::{host, place, shape};
use egui::ViewportId;
use shellman_ui::glass::{self, Island, Style};
use std::collections::HashMap;
use windows::Win32::Foundation::RECT;

const TITLE: &str = "shellman calendar";
const WARM_FRAMES: u8 = 2;

pub fn id() -> ViewportId {
    ViewportId::from_hash_of("shellman calendar")
}

fn style() -> Style {
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

/// The panel's height in points: the island and the room for its shadow.
pub fn content_height() -> f32 {
    UNDER + month::HEIGHT + EDGE
}

pub struct CalPanel {
    ctx: egui::Context,
    open: bool,
    hwnd: Option<isize>,
    /// Asked to make the window before its first use (see `make_ahead`).
    ahead: bool,
    out: bool,
    warm: Option<u8>,
    /// Every bar's clock, by monitor device.
    clocks: HashMap<String, Clock>,
    /// The bar the panel hangs from.
    device: Option<String>,
    placed: Option<[i32; 4]>,
    /// The app in front when it opened: it closes when another comes forward.
    front_at_open: Option<isize>,
    pub view: month::View,
    /// "Connect a calendar" was clicked: the host opens Settings.
    pub asked_settings: bool,
}

impl CalPanel {
    pub fn new(ctx: egui::Context) -> Self {
        Self {
            ctx,
            open: false,
            hwnd: None,
            ahead: false,
            out: false,
            warm: None,
            clocks: HashMap::new(),
            device: None,
            placed: None,
            front_at_open: None,
            view: month::View::on(0),
            asked_settings: false,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn window_exists(&self) -> bool {
        self.hwnd.is_some()
    }

    pub fn set_clock(&mut self, device: &str, clock: Clock) {
        if self.clocks.get(device) != Some(&clock) {
            self.clocks.insert(device.to_owned(), clock);
        }
    }

    pub fn keep_clocks(&mut self, devices: &[&str]) {
        self.clocks.retain(|d, _| devices.contains(&d.as_str()));
    }

    /// Open on `day` under `device`'s clock (or move there), with `front` the app in front.
    pub fn open_on(&mut self, day: i64, front: Option<isize>, device: &str) {
        self.open = true;
        self.device = Some(device.to_owned());
        self.front_at_open = front;
        self.view = month::View::on(day);
        self.changed();
    }

    /// The clock on `device` was clicked: open there on `today`, or close.
    pub fn toggle(&mut self, today: i64, front: Option<isize>, device: &str) {
        if self.open {
            self.close();
        } else {
            self.open_on(today, front, device);
        }
    }

    pub fn close(&mut self) {
        if self.open {
            self.open = false;
            self.changed();
        }
    }

    /// An app came forward: the panel closes, unless it is the one in front when it opened.
    pub fn front_is(&mut self, front: Option<isize>) {
        if self.open && front.is_some() && front != self.front_at_open {
            self.close();
        }
    }

    fn changed(&self) {
        host::request_frame();
        if self.hwnd.is_some() {
            self.ctx.request_repaint_of(id());
        }
    }

    fn area(&self) -> Option<(Area, (i32, i32))> {
        let clock = self.device.as_ref().and_then(|d| self.clocks.get(d))?;
        Some((layout::area_of(clock.anchor, &crate::notify::areas())?, clock.anchor))
    }

    fn place(&mut self, hwnd: isize) {
        let Some((area, anchor)) = self.area() else { return };
        let Some(r) = layout::window_for(anchor, &area, content_height()) else { return };
        let Some((w, h)) = glass::guard::window((r[2] - r[0], r[3] - r[1])) else { return };
        let wanted = [r[0], r[1], r[0] + w, r[1] + h];
        if self.placed != Some(wanted) {
            place::put(hwnd, RECT { left: wanted[0], top: wanted[1], right: wanted[2], bottom: wanted[3] });
            self.placed = Some(wanted);
        }
    }

    /// In the host's frame: place or park the window; whether its viewport is to be declared.
    pub fn tick(&mut self) -> bool {
        if let Some(hwnd) = self.hwnd {
            if self.open {
                self.place(hwnd);
                if !self.out {
                    self.out = true;
                    self.warm = Some(0);
                    self.ctx.request_repaint_of(id());
                }
            } else if self.out {
                self.out = false;
                self.warm = None;
                glass::cloak(hwnd, true);
                surface::set_islands(hwnd, &[], 1.0, true, None);
            }
        }
        self.hwnd.is_some() || self.open || self.ahead
    }

    /// After the host declared the window: find it once, give it its manners and glass.
    pub fn adopt(&mut self) -> bool {
        if self.hwnd.is_some() || !(self.open || self.ahead) {
            return true;
        }
        let Some(hwnd) = shape::own_window(TITLE) else { return false };
        glass::cloak(hwnd, true);
        shape::never_activate(hwnd);
        shape::see_through(hwnd);
        surface::attach(hwnd, style());
        surface::set_islands(hwnd, &[], 1.0, true, None);
        self.place(hwnd);
        crate::notify::show_window(hwnd);
        self.hwnd = Some(hwnd);
        shellman_core::timing::mark_with("calendar: window made", || if self.open { "on first use" } else { "ahead" }.to_string());
        if self.open {
            self.out = true;
            self.warm = Some(0);
        }
        // Made ahead: it stays cloaked and parked, as after a close; the first open takes the
        // same path as every later one.
        self.ctx.request_repaint_of(id());
        true
    }

    /// Make the window now, hidden, so the first open does not wait for it.
    pub fn make_ahead(&mut self) {
        self.ahead = true;
    }

    /// The panel's screen rectangle, while it is out.
    pub fn rect(&self) -> Option<(i32, i32, i32, i32)> {
        self.placed.filter(|_| self.open).map(|p| (p[0], p[1], p[2], p[3]))
    }

    /// The clocks' and pills' rectangles: a click there toggles the panel itself.
    pub fn openers(&self) -> Vec<(i32, i32, i32, i32)> {
        self.clocks.values().flat_map(|c| std::iter::once(c.rect).chain(c.also)).collect()
    }

    /// The window's frame.
    pub fn frame(&mut self, ctx: &egui::Context, data: &month::Data) {
        let Some(hwnd) = self.hwnd else { return };
        if !self.open || !self.out {
            surface::set_islands(hwnd, &[], ctx.pixels_per_point(), true, None);
            egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |_| {});
            return;
        }
        surface::set_style(hwnd, style());
        let rect = month::rect(UNDER, ctx.screen_rect().width());
        let island = Island::new(rect, layout::RADIUS);
        let composed = surface::composed(hwnd);
        let asked = egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ctx, |ui| {
                let under = ui.painter().add(egui::Shape::Noop);
                let style = style();
                if !composed {
                    ui.painter().set(under, egui::Shape::Vec(glass::islands::solid_shapes(&[island], &style)));
                } else if !shellman_ui::theme::colors().light {
                    let fill = crate::topbar::scrim(style.tint);
                    ui.painter().set(under, egui::Shape::rect_filled(rect, layout::RADIUS as u8, fill));
                }
                month::draw(ui, rect, &mut self.view, data)
            })
            .inner;
        surface::set_islands(hwnd, &[island], ctx.pixels_per_point(), true, None);
        if let Some(a) = asked {
            self.asked(a, data.today);
            ctx.request_repaint();
        }
        if let Some(n) = self.warm {
            if n + 1 >= WARM_FRAMES {
                glass::cloak(hwnd, false);
                shellman_core::timing::mark("calendar: shown");
                self.warm = None;
            } else {
                self.warm = Some(n + 1);
                ctx.request_repaint();
            }
        }
    }

    fn asked(&mut self, asked: month::Asked, today: i64) {
        use shellman_core::calendar::month::step;
        use shellman_core::calendar::time::{civil_from_days, days_from_civil};
        match asked {
            month::Asked::Step(by) => {
                let (y, m) = step(self.view.year, self.view.month, by);
                let mut v = month::View::on(days_from_civil(y, m, 1));
                // Today stays chosen when its month comes back.
                let (ty, tm, _) = civil_from_days(today);
                if (ty, tm) == (y, m) {
                    v.selected = today;
                }
                self.view = v;
            }
            month::Asked::Today => self.view = month::View::on(today),
            month::Asked::Select(day) => {
                let (y, m, _) = civil_from_days(day);
                if (y, m) != (self.view.year, self.view.month) {
                    self.view = month::View::on(day);
                } else {
                    self.view.selected = day;
                }
            }
            month::Asked::Connect => {
                self.asked_settings = true;
                self.close();
            }
        }
    }
}
