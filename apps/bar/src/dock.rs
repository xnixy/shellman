//! The dock: one icon per running app, on a floating glass pill.

use crate::place;
use crate::shape::SYSTEM_RADIUS as RADIUS;
use shellman_core::thumbs::Thumbs;
use shellman_core::winlist::{self, AppWindow, WindowList, WindowWatcher};
use shellman_ui::theme::{self, colors, glyph, icon_font};
use std::path::PathBuf;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    IsIconic, SetForegroundWindow, ShowWindow, SW_MINIMIZE, SW_RESTORE,
};

pub const HEIGHT: f32 = 56.0;
const CELL: f32 = 52.0;
const ICON: f32 = 40.0;
const PAD: f32 = 8.0;

/// One dock icon: an app and its windows, front-most first.
struct Item {
    app_id: String,
    exe: Option<PathBuf>,
    windows: Vec<isize>,
}

impl Item {
    /// Where its icon comes from: the program file's own icon, as Explorer's taskbar shows it.
    /// Only without one, the app's entry in the Apps folder (whose Store icons come on a plate).
    fn icon_source(&self) -> Option<PathBuf> {
        let is_path = self.app_id.contains('\\');
        self.exe.clone().or_else(|| {
            (!is_path && !self.app_id.is_empty())
                .then(|| PathBuf::from(format!("shell:AppsFolder\\{}", self.app_id)))
        })
    }
}

pub struct Dock {
    glass: bool,
    list: WindowList,
    watcher: WindowWatcher,
    thumbs: Thumbs,
    items: Vec<Item>,
    front: Option<isize>,
    /// Size last given to the window, in points, so it is only resized when it changes.
    placed: Option<egui::Vec2>,
}

impl Dock {
    pub fn new(ctx: egui::Context, glass: bool) -> Self {
        let wake = ctx.clone();
        Self {
            glass,
            list: WindowList::default(),
            watcher: WindowWatcher::start(move || wake.request_repaint()),
            thumbs: Thumbs::new(ctx),
            items: Vec::new(),
            front: None,
            placed: None,
        }
    }

    /// Re-group windows into icons. Existing icons keep their places; new apps go at the end.
    fn regroup(&mut self) {
        let windows: Vec<AppWindow> = self.list.refresh();
        for item in &mut self.items {
            item.windows.clear();
        }
        for w in windows {
            match self.items.iter_mut().find(|i| i.app_id == w.app_id) {
                Some(item) => item.windows.push(w.hwnd),
                None => self.items.push(Item {
                    app_id: w.app_id.clone(),
                    exe: w.exe.clone(),
                    windows: vec![w.hwnd],
                }),
            }
        }
        self.items.retain(|i| !i.windows.is_empty());
        self.front = winlist::foreground();
    }

    /// Fit the window to the icons and keep it centred at the bottom.
    fn place(&mut self, ctx: &egui::Context) {
        let size = egui::vec2(PAD * 2.0 + CELL * self.items.len().max(1) as f32, HEIGHT);
        if self.placed == Some(size) {
            return;
        }
        self.placed = Some(size);
        let ppp = ctx.pixels_per_point();
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(place::dock_origin(size, ppp)));
    }

    /// Bring the app forward; if it is already in front, minimize it, or with several windows
    /// show the next one.
    fn activate(&self, item: &Item) {
        let front_idx = self.front.and_then(|f| item.windows.iter().position(|&w| w == f));
        let target = match front_idx {
            Some(_) if item.windows.len() == 1 => {
                unsafe {
                    let _ = ShowWindow(HWND(item.windows[0] as *mut _), SW_MINIMIZE);
                }
                return;
            }
            Some(i) => item.windows[(i + 1) % item.windows.len()],
            None => item.windows[0],
        };
        let hwnd = HWND(target as *mut _);
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(hwnd);
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        let c = colors();
        let full = ui.max_rect();
        let pill = full;
        let fill = if self.glass { c.shell } else { c.bg };
        ui.painter()
            .rect_filled(pill, egui::CornerRadius::same(RADIUS as u8), fill);
        ui.painter().rect_stroke(
            pill.shrink(0.5),
            egui::CornerRadius::same(RADIUS as u8),
            egui::Stroke::new(1.0_f32, c.hairline),
            egui::StrokeKind::Inside,
        );
        let mut clicked = None;
        let mut middle = None;
        for (i, item) in self.items.iter().enumerate() {
            let cell = egui::Rect::from_min_size(
                pill.min + egui::vec2(PAD + i as f32 * CELL, (HEIGHT - CELL) / 2.0),
                egui::vec2(CELL, CELL),
            );
            let resp = ui.interact(cell, egui::Id::new(("dock", i)), egui::Sense::click());
            let lift = ui.ctx().animate_bool(resp.id, resp.hovered()) * 3.0;
            let icon = egui::Rect::from_center_size(cell.center() - egui::vec2(0.0, 2.0 + lift), egui::Vec2::splat(ICON));
            match item.icon_source().and_then(|p| self.thumbs.get(&p, true).map(|t| (t.id(), t.size_vec2()))) {
                Some((id, size)) => {
                    let scale = (ICON / size.x).min(ICON / size.y);
                    egui::Image::from_texture((id, size))
                        .paint_at(ui, egui::Rect::from_center_size(icon.center(), size * scale));
                }
                None => {
                    ui.painter().rect_filled(icon, egui::CornerRadius::same(10), c.control);
                    ui.painter().text(icon.center(), egui::Align2::CENTER_CENTER, glyph::APP, icon_font(18.0), c.muted);
                }
            }
            // Running dots; the app in front gets an accent bar instead.
            let is_front = self.front.is_some_and(|f| item.windows.contains(&f));
            let y = cell.bottom() - 4.0;
            if is_front {
                let bar = egui::Rect::from_center_size(egui::pos2(cell.center().x, y), egui::vec2(16.0, 3.0));
                ui.painter().rect_filled(bar, egui::CornerRadius::same(2), c.accent);
            } else {
                let n = item.windows.len().min(3);
                for d in 0..n {
                    let x = cell.center().x + (d as f32 - (n - 1) as f32 / 2.0) * 7.0;
                    ui.painter().circle_filled(egui::pos2(x, y), 2.0, c.muted);
                }
            }
            let tip = item.exe.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned());
            let resp = match tip {
                Some(t) => resp.on_hover_text(t),
                None => resp,
            };
            if resp.clicked() {
                clicked = Some(i);
            }
            if resp.middle_clicked() {
                middle = Some(i);
            }
        }
        if let Some(i) = clicked {
            self.activate(&self.items[i]);
        }
        if let Some(exe) = middle.and_then(|i| self.items[i].exe.clone()) {
            shellman_core::fsops::open(&exe);
        }
    }
}

impl eframe::App for Dock {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        theme::set_light(ctx.theme() == egui::Theme::Light);
        self.thumbs.poll(ctx);
        if self.watcher.take_dirty() {
            self.regroup();
        }
        self.place(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ctx, |ui| self.draw(ui));
    }
}
