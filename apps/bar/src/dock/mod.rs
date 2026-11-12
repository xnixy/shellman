//! The dock: the Launcher, Files, pinned apps, running apps and the Recycle Bin, on a floating glass pill.
//!
//! One model, several views: there is one `Dock` (window list, pins, icon cache) and a dock window
//! per monitor drawing it, so every dock shows the same apps in the same order. A click on any of
//! them brings the window forward wherever it is.

mod draw;
mod items;
mod menu;

use crate::fullscreen;
use crate::wake::Wake;
use items::{Item, Section};
use shellman_core::settings::dock::{DockSettings as Pins, Pin};
use shellman_core::thumbs::Thumbs;
use shellman_core::winlist::{self, AppWindow, WindowList, WindowWatcher};
use shellman_ui::theme;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    IsIconic, PostMessageW, SetForegroundWindow, ShowWindow, SW_MINIMIZE, SW_RESTORE, WM_CLOSE,
};

/// Wheel movement, in points, that counts as one step through an app's windows. A notch is more
/// than this; a touchpad adds up to it.
const SCROLL_STEP: f32 = 30.0;

pub struct Dock {
    glass: bool,
    /// Repaints every dock and top bar.
    wake: Wake,
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
    /// Wheel movement not yet enough for a step, and the icon it happened over.
    scroll: (usize, f32),
}

impl Dock {
    pub fn new(ctx: egui::Context, glass: bool, wake: Wake) -> Self {
        let bars = wake.clone();
        let watcher = WindowWatcher::start(move || {
            bars.bars();
            // A hidden window gets no repaints, so a fullscreen app ending is noticed by message.
            fullscreen::request_check();
        });
        watcher.on_front_moved(fullscreen::request_check);
        let mut dock = Self {
            glass,
            wake,
            list: WindowList::default(),
            watcher,
            thumbs: Thumbs::new(ctx),
            pins: Pins::load(),
            files: items::files_pin(),
            items: Vec::new(),
            running_order: Vec::new(),
            windows: Vec::new(),
            front: None,
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
            self.wake.bars();
            if resized {
                // The dock's height, and so the space it reserves, follows the icons.
                crate::host::request_layout();
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

    /// The dock's width in points, which every dock window takes on.
    pub fn width(&self) -> f32 {
        draw::layout(&self.items, self.pins.icon_size).1
    }

    /// The dock's height in points, which follows the icon size.
    pub fn height(&self) -> f32 {
        self.pins.icon_size.height()
    }

    /// Bring a window forward, restoring it if minimized.
    pub fn show_window(hwnd: isize) {
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
        if item.section == Section::Launcher {
            crate::topbar::open_start();
            return;
        }
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
    fn context_menu(&mut self, i: usize, hwnd: isize) {
        let item = self.items[i].clone();
        let kind = if item.section == Section::Launcher {
            menu::Kind::Launcher
        } else {
            let pinned = item.pinnable().then(|| item.section == Section::Pinned);
            menu::Kind::App { pinned, windows: item.windows.len() }
        };
        match menu::show(hwnd, kind) {
            menu::Choice::Settings => {
                shellman_core::sibling::start("settings");
            }
            menu::Choice::TogglePin => {
                if item.section == Section::Pinned {
                    self.pins.remove(&item.app_id);
                } else {
                    self.pins.add(&item.app_id, item.exe.as_deref());
                }
                self.pins.save();
                self.rebuild();
                self.wake.bars();
            }
            menu::Choice::Close => {
                for &w in &item.windows {
                    close_window(w);
                }
            }
            menu::Choice::Nothing => {}
        }
    }

    /// The windows of an app's icon, front-most first, for its preview.
    pub fn windows_of(&self, app_id: &str) -> Vec<AppWindow> {
        let Some(item) = self.items.iter().find(|i| i.app_id == app_id) else { return Vec::new() };
        item.windows.iter().filter_map(|h| self.windows.iter().find(|w| w.hwnd == *h).cloned()).collect()
    }

    /// An app's icon, once the shell has it.
    pub fn icon(&mut self, ctx: &egui::Context, source: &std::path::Path) -> Option<(egui::TextureId, egui::Vec2)> {
        shellman_ui::app_icon::get(ctx, source, true, &mut self.thumbs)
    }

    fn draw(&mut self, ui: &mut egui::Ui, hwnd: isize) -> Hover {
        let events = draw::paint(ui, self.glass, self.pins.icon_size, &mut self.thumbs, &self.items, self.front);
        let hover = Hover {
            target: events.hovered.map(|(i, center_x)| Target {
                app_id: self.items[i].app_id.clone(),
                has_windows: !self.items[i].windows.is_empty(),
                center_x,
            }),
            inside: ui.ctx().pointer_hover_pos().is_some(),
            dismiss: events.clicked.is_some() || events.middle.is_some() || events.menu.is_some(),
        };
        if let Some(i) = events.clicked {
            self.activate(&self.items[i]);
        }
        // Middle click opens another window of the app; for the Launcher, it is a click.
        if let Some(i) = events.middle {
            if self.items[i].section == Section::Launcher {
                crate::topbar::open_start();
            } else if let Some(target) = self.items[i].launch_target() {
                shellman_core::fsops::open(&target);
            }
        }
        if let Some((i, delta)) = events.scrolled {
            self.scroll_step(i, delta);
        }
        if let Some(i) = events.menu {
            self.context_menu(i, hwnd);
        }
        hover
    }
}

/// Ask a window to close, as its own close button would.
pub fn close_window(hwnd: isize) {
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}

/// What the pointer is over in a dock window, for the preview panel.
pub struct Hover {
    /// The icon under the pointer.
    pub target: Option<Target>,
    /// The pointer is over this dock window at all (also between icons).
    pub inside: bool,
    /// An icon was clicked this frame: whatever was previewed should go.
    pub dismiss: bool,
}

pub struct Target {
    pub app_id: String,
    pub has_windows: bool,
    /// The icon cell's centre, in points from the dock window's left edge.
    pub center_x: f32,
}

impl Dock {
    /// The app window in front, if it is one the dock knows.
    pub fn front_window(&self) -> Option<&AppWindow> {
        let front = self.front?;
        self.windows.iter().find(|w| w.hwnd == front)
    }

    /// Catch up with the window watcher and finished icons. Any bar's frame calls this first,
    /// so whichever window repaints first brings the shared model up to date for all of them.
    pub fn catch_up(&mut self, ctx: &egui::Context) {
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
    }

    /// One frame of one dock window: `hwnd` is that window, and `salt` (its monitor) keeps its
    /// icons' hover state apart from the other docks'.
    pub fn frame(&mut self, ctx: &egui::Context, hwnd: isize, salt: &str) -> Hover {
        theme::set_light(ctx.theme() == egui::Theme::Light);
        self.catch_up(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ctx, |ui| ui.push_id(salt, |ui| self.draw(ui, hwnd)))
            .inner
            .inner
    }
}
