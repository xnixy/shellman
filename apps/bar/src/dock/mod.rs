//! The dock: Files, pinned apps, running apps and the Recycle Bin, on a floating glass pill.

mod draw;
mod items;
mod menu;

use crate::{fullscreen, place};
use items::{Item, Section};
use shellman_core::settings::dock::{DockSettings as Pins, Pin};
use shellman_core::thumbs::Thumbs;
use shellman_core::winlist::{self, AppWindow, WindowList, WindowWatcher};
use shellman_ui::theme;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    IsIconic, PostMessageW, SetForegroundWindow, ShowWindow, SW_MINIMIZE, SW_RESTORE, WM_CLOSE,
};

pub use draw::HEIGHT;

/// Wheel movement, in points, that counts as one step through an app's windows. A notch is more
/// than this; a touchpad adds up to it.
const SCROLL_STEP: f32 = 30.0;

pub struct Dock {
    glass: bool,
    /// The dock's own window, which owns the right-click menu.
    hwnd: isize,
    list: WindowList,
    watcher: WindowWatcher,
    thumbs: Thumbs,
    pins: Pins,
    files: Pin,
    items: Vec<Item>,
    /// Unpinned running apps in the order shown, so icons stay where they are.
    running_order: Vec<String>,
    /// The latest window list, front to back, for the top bar.
    windows: Vec<AppWindow>,
    front: Option<isize>,
    /// Size last given to the window, in points, so it is only resized when it changes.
    placed: Option<egui::Vec2>,
    /// Wheel movement not yet enough for a step, and the icon it happened over.
    scroll: (usize, f32),
}

impl Dock {
    pub fn new(ctx: egui::Context, glass: bool, hwnd: isize) -> Self {
        let wake = ctx.clone();
        let watcher = WindowWatcher::start(move || {
            wake.request_repaint();
            // A hidden window gets no repaints, so a fullscreen app ending is noticed by message.
            fullscreen::request_check();
        });
        watcher.on_front_moved(fullscreen::request_check);
        let mut dock = Self {
            glass,
            hwnd,
            list: WindowList::default(),
            watcher,
            thumbs: Thumbs::new(ctx),
            pins: Pins::load(),
            files: items::files_pin(),
            items: Vec::new(),
            running_order: Vec::new(),
            windows: Vec::new(),
            front: None,
            placed: None,
            scroll: (0, 0.0),
        };
        dock.rebuild();
        dock
    }

    /// Read the dock's settings again, when something saved them: the settings app (pins, icon
    /// size), or edits by hand and other bar instances.
    pub fn reload_pins(&mut self) {
        let pins = Pins::load();
        if pins != self.pins {
            let resized = pins.icon_size != self.pins.icon_size;
            self.pins = pins;
            self.rebuild();
            if resized {
                // Make `place` fit the window to the new size.
                self.placed = None;
            }
        }
    }

    /// Ask Windows for the windows and the one in front, then lay out the icons.
    fn regroup(&mut self) {
        self.windows = self.list.refresh();
        self.front = winlist::foreground();
        self.rebuild();
    }

    /// Lay out the icons from the windows and pins already known.
    fn rebuild(&mut self) {
        self.items = items::build(&self.pins.pins, &self.files, &self.windows, &self.running_order);
        self.running_order = self
            .items
            .iter()
            .filter(|i| i.section == Section::Running)
            .map(|i| i.app_id.clone())
            .collect();
    }

    /// Fit the window to the icons and keep it centred at the bottom.
    fn place(&mut self, ctx: &egui::Context) {
        let icons = self.pins.icon_size;
        let size = egui::vec2(draw::layout(&self.items, icons).1, icons.height());
        if self.placed == Some(size) {
            return;
        }
        self.placed = Some(size);
        let ppp = ctx.pixels_per_point();
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(place::dock_origin(size, ppp)));
    }

    /// Bring a window forward, restoring it if minimized.
    fn show_window(hwnd: isize) {
        let hwnd = HWND(hwnd as *mut _);
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(hwnd);
        }
    }

    /// Click: start the app if it has no window; bring it forward; if it is already in front,
    /// minimize it, or with several windows show the next one.
    fn activate(&self, item: &Item) {
        if item.windows.is_empty() {
            if let Some(target) = item.launch_target() {
                shellman_core::fsops::open(&target);
            }
            return;
        }
        let in_front = self.front.is_some_and(|f| item.windows.contains(&f));
        if in_front && item.windows.len() == 1 {
            unsafe {
                let _ = ShowWindow(HWND(item.windows[0] as *mut _), SW_MINIMIZE);
            }
        } else if let Some(w) = items::cycle_target(&item.windows, self.front, 1) {
            Self::show_window(w);
        }
    }

    /// Scroll over an icon: step through that app's windows, down for next and up for previous.
    fn scroll_step(&mut self, i: usize, delta: f32) {
        if self.scroll.0 != i {
            self.scroll = (i, 0.0);
        }
        self.scroll.1 += delta;
        while self.scroll.1.abs() >= SCROLL_STEP {
            let step = if self.scroll.1 < 0.0 { 1 } else { -1 };
            self.scroll.1 += SCROLL_STEP * step as f32;
            if let Some(w) = items::cycle_target(&self.items[i].windows, self.front, step) {
                Self::show_window(w);
                // The window takes a moment to become the front; step from where we are.
                self.front = Some(w);
            }
        }
    }

    /// Right click: the menu for one icon, and what it chose.
    fn context_menu(&mut self, i: usize) {
        let item = self.items[i].clone();
        let pinned = item.pinnable().then(|| item.section == Section::Pinned);
        match menu::show(self.hwnd, pinned, item.windows.len()) {
            menu::Choice::TogglePin => {
                if item.section == Section::Pinned {
                    self.pins.remove(&item.app_id);
                } else {
                    self.pins.add(&item.app_id, item.exe.as_deref());
                }
                self.pins.save();
                self.rebuild();
            }
            menu::Choice::Close => {
                for &w in &item.windows {
                    unsafe {
                        let _ = PostMessageW(Some(HWND(w as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                }
            }
            menu::Choice::Nothing => {}
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        let events = draw::paint(ui, self.glass, self.pins.icon_size, &mut self.thumbs, &self.items, self.front);
        if let Some(i) = events.clicked {
            self.activate(&self.items[i]);
        }
        // Middle click opens another window of the app.
        if let Some(target) = events.middle.and_then(|i| self.items[i].launch_target()) {
            shellman_core::fsops::open(&target);
        }
        if let Some((i, delta)) = events.scrolled {
            self.scroll_step(i, delta);
        }
        if let Some(i) = events.menu {
            self.context_menu(i);
        }
    }
}

impl Dock {
    /// The app window in front, if it is one the dock knows.
    pub fn front_window(&self) -> Option<&AppWindow> {
        let front = self.front?;
        self.windows.iter().find(|w| w.hwnd == front)
    }

    /// One frame of the dock, which is the bar process's main window.
    pub fn frame(&mut self, ctx: &egui::Context) {
        theme::set_light(ctx.theme() == egui::Theme::Light);
        self.thumbs.poll(ctx);
        if self.watcher.take_dirty() {
            self.regroup();
        } else if self.watcher.take_front_renamed() {
            if let Some(front) = self.front {
                let title = winlist::title_of(front);
                if let Some(w) = self.windows.iter_mut().find(|w| w.hwnd == front) {
                    w.title = title;
                }
            }
        }
        self.place(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ctx, |ui| self.draw(ui));
    }
}
