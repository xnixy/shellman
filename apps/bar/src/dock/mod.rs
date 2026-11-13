//! The dock: the Launcher, Files, pinned apps and running apps on a floating island of glass
//! (`shellman_ui::glass`) with large rounded corners, real blur and a soft shadow, and the Recycle
//! Bin on a small island of its own just to the right.
//!
//! One model, several views: there is one `Dock` (window list, pins, icon cache) and a dock window
//! per monitor drawing it, so every dock shows the same apps in the same order. A click on any of
//! them brings the window forward wherever it is.

mod bin;
mod drag;
mod draw;
mod drop;
mod drop_target;
mod items;
mod menu;
mod metrics;
mod shown;

pub use metrics::ABOVE;

use crate::fullscreen;
use crate::shape::surface;
use crate::wake::Wake;
use items::{Item, Section};
use metrics::Metrics;
use shown::Shown;
use shellman_ui::glass::{self, Island, Style};
use shellman_ui::menu::popup::PopupMenu;
use shellman_core::settings::dock::{DockSettings as Pins, Pin};
use shellman_core::thumbs::Thumbs;
use shellman_core::winlist::{self, AppWindow, WindowList, WindowWatcher};
use shellman_ui::theme;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, IsIconic, PostMessageW, SetForegroundWindow, ShowWindow, SW_MINIMIZE, SW_RESTORE, WM_CLOSE,
};

/// Wheel movement, in points, that counts as one step through an app's windows. A notch is more
/// than this; a touchpad adds up to it.
const SCROLL_STEP: f32 = 30.0;

pub struct Dock {
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
    /// The icons' right-click menu, and the icon it is open for.
    menu: PopupMenu<menu::Choice>,
    menu_for: Option<Item>,
    /// An icon being picked up and moved (see `drag`).
    drag: drag::Machine,
    /// Files dragged over the dock windows from other apps (see `drop`).
    drops: drop_target::Drops,
}

impl Dock {
    /// `_glass` (whether Windows can frost a window) is not needed: each dock window's glass
    /// says for itself whether it is composed.
    pub fn new(ctx: egui::Context, _glass: bool, wake: Wake) -> Self {
        let watcher = WindowWatcher::start(move || {
            // The host reads the list again and wakes only the bars whose content changed.
            crate::host::request_frame();
            // A hidden window gets no repaints, so a fullscreen app ending is noticed by message.
            fullscreen::request_check();
        });
        watcher.on_front_moved(fullscreen::request_check);
        let mut dock = Self {
            drops: drop_target::Drops::new(wake.clone()),
            wake,
            list: WindowList::default(),
            watcher,
            // Cropped 256 px images, drawn crisp at any dock size and scale.
            thumbs: Thumbs::icons(ctx),
            pins: Pins::load(),
            files: items::files_pin(),
            items: Vec::new(),
            running_order: Vec::new(),
            windows: Vec::new(),
            front: None,
            scroll: (0, 0.0),
            menu: PopupMenu::new(),
            menu_for: None,
            drag: drag::Machine::default(),
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
        self.windows.retain(|w| !is_bar_window(w));
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

    fn metrics(&self) -> Metrics {
        Metrics::new(self.pins.icon_size)
    }

    /// The glass's height in points, which follows the icon size.
    pub fn height(&self) -> f32 {
        self.metrics().height
    }

    /// The look of the dock's glass in the current theme.
    pub fn style(&self) -> Style {
        let mut style = Style::from_theme(self.metrics().radius);
        // A shelf near the screen's edge: a shorter, tighter shadow than a floating panel's, so
        // little of it falls on the windows above.
        style.shadow.offset_y = 4.0;
        style.shadow.blur = 18.0;
        style
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
        if item.section == Section::Bin {
            bin::open();
            return;
        }
        if item.windows.is_empty() {
            if let Some(target) = item.launch_target() {
                launch(target);
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

    /// Right click: open the menu for one icon. What it chooses comes back in a later frame
    /// (`menu_picked`).
    fn context_menu(&mut self, ctx: &egui::Context, i: usize) {
        let item = self.items[i].clone();
        let kind = if item.section == Section::Launcher {
            menu::Kind::Launcher
        } else if item.section == Section::Bin {
            menu::Kind::Bin { can_empty: bin::can_empty() }
        } else {
            let pinned = item.pinnable().then(|| item.section == Section::Pinned);
            menu::Kind::App { pinned, windows: item.windows.len() }
        };
        let entries = menu::entries(&kind);
        if entries.is_empty() {
            return;
        }
        self.menu.open(ctx, menu::anchor(), entries);
        self.menu_for = Some(item);
    }

    /// Do what the icon's menu chose, if it chose something since the last frame.
    fn menu_picked(&mut self) {
        let Some(choice) = self.menu.take_pick() else { return };
        let Some(item) = self.menu_for.take() else { return };
        match choice {
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
            menu::Choice::OpenBin => {
                bin::open();
            }
            menu::Choice::EmptyBin => bin::confirm_and_empty(),
        }
    }

    /// The windows of an app's icon, front-most first, for its preview.
    pub fn windows_of(&self, app_id: &str) -> Vec<AppWindow> {
        let Some(item) = self.items.iter().find(|i| i.app_id == app_id) else { return Vec::new() };
        item.windows.iter().filter_map(|h| self.windows.iter().find(|w| w.hwnd == *h).cloned()).collect()
    }

    /// Each app's front-most window, by app id (the notifications bring an app forward with it).
    pub fn front_windows(&self) -> Vec<(String, isize)> {
        self.items.iter().filter_map(|i| Some((i.app_id.clone(), *i.windows.first()?))).collect()
    }

    /// An app's icon, once the shell has it.
    pub fn icon(&mut self, ctx: &egui::Context, source: &std::path::Path) -> Option<(egui::TextureId, egui::Vec2)> {
        shellman_ui::app_icon::get(ctx, source, true, &mut self.thumbs)
    }

    /// Act on what the pointer did this frame, and say what it is over for the preview panel.
    fn act(&mut self, ctx: &egui::Context, events: &draw::Events, inside: bool, glass_top: f32) -> Hover {
        let hover = Hover {
            target: events.hovered.map(|(i, center_x)| Target {
                app_id: self.items[i].app_id.clone(),
                has_windows: !self.items[i].windows.is_empty(),
                center_x,
                top: glass_top,
            }),
            inside,
            dismiss: events.clicked.is_some() || events.middle.is_some() || events.menu.is_some(),
        };
        if let Some(i) = events.clicked {
            self.activate(&self.items[i]);
        }
        // Middle click opens another window of the app; for the Launcher, it is a click.
        if let Some(i) = events.middle {
            if self.items[i].section == Section::Launcher {
                crate::topbar::open_start();
            } else if self.items[i].section == Section::Bin {
                bin::open();
            } else if let Some(target) = self.items[i].launch_target() {
                shellman_core::fsops::open(&target);
            }
        }
        if let Some((i, delta)) = events.scrolled {
            self.scroll_step(i, delta);
        }
        if let Some(i) = events.menu {
            self.context_menu(ctx, i);
        }
        hover
    }
}

/// A window of a shellman bar process (this one or another, such as a test bar). The bar has no
/// app windows; anything of its that looks like one is a fault (an older bar briefly opened blank
/// "egui window" windows several times a second, and every dock grew and shrank with them).
fn is_bar_window(w: &AppWindow) -> bool {
    w.exe.as_deref().and_then(|p| p.file_name()).is_some_and(|n| n.eq_ignore_ascii_case("shellman-bar.exe"))
}

/// Start what a pin or icon opens: a pinned folder opens in Files, anything else through the
/// shell.
fn launch(target: std::path::PathBuf) {
    if target.is_dir() {
        shellman_core::sibling::start_with("files", &[&target.to_string_lossy()]);
    } else {
        shellman_core::fsops::open(&target);
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
    /// The icon's centre, in points from the dock window's left edge.
    pub center_x: f32,
    /// The top of the dock's glass, in points from the dock window's top edge.
    pub top: f32,
}

impl Dock {
    /// The app window in front, if it is one the dock knows.
    pub fn front_window(&self) -> Option<&AppWindow> {
        let front = self.front?;
        self.windows.iter().find(|w| w.hwnd == front)
    }

    /// The icon of the app in front, as its dock icon shows it, sharp at `px` pixels (for the
    /// top bar). None while the shell has not given it yet, or when no dock icon has the window.
    pub fn front_icon(&mut self, ctx: &egui::Context, px: f32) -> Option<(egui::TextureId, egui::Vec2)> {
        let front = self.front?;
        let item = self.items.iter().find(|i| i.windows.contains(&front))?.clone();
        draw::texture(ctx, &item, px, &mut self.thumbs)
    }

    /// Catch up with the window watcher and finished icons. The host's frame and every bar's
    /// frame call this first, so whichever runs first brings the shared model up to date, and
    /// wakes the bars whose picture changed: only those.
    pub fn catch_up(&mut self, ctx: &egui::Context) {
        let icons = self.thumbs.poll(ctx);
        let renamed = self.watcher.take_front_renamed();
        let dirty = self.watcher.take_dirty();
        if !(icons || renamed || dirty) {
            return;
        }
        let before = Shown::of(&self.items, self.front, self.front_window());
        if dirty {
            self.regroup();
        } else if renamed {
            if let Some(front) = self.front {
                let title = winlist::title_of(front);
                if let Some(w) = self.windows.iter_mut().find(|w| w.hwnd == front) {
                    w.title = title;
                }
            }
        }
        let change = before.change(&Shown::of(&self.items, self.front, self.front_window()));
        // New icons can be on any bar: a dock's, or the top bar's app in front.
        if icons || change.docks {
            self.wake.docks();
        }
        if icons || change.tops {
            self.wake.tops();
        }
        if change.docks || change.tops {
            // The preview panel lists an app's windows.
            self.wake.preview();
        }
    }

    /// One frame of one dock window: `hwnd` is that window, and `salt` (its monitor) keeps its
    /// icons' hover state apart from the other docks'.
    pub fn frame(&mut self, ctx: &egui::Context, hwnd: isize, salt: &str) -> Hover {
        theme::set_light(crate::app::light(ctx));
        self.menu_picked();
        self.catch_up(ctx);
        let m = self.metrics();
        let style = self.style();
        surface::set_style(hwnd, style);
        let scale = ctx.pixels_per_point();
        let (w, h) = client_size(hwnd);
        let window = egui::vec2(w as f32, h as f32) / scale;
        let held = self.drag_frame(ctx, salt, window, &m);
        // While an icon is in hand the others show where it would land; otherwise the real order.
        let shown_items = match &held.preview {
            Some(items) => items.clone(),
            None => self.items.clone(),
        };
        let (xs, width) = metrics::layout(&shown_items, &m);
        // The apps island eases to its new width as the icons slide (see `draw`), and the bin's
        // island follows its right edge.
        let shown = ctx.animate_value_with_time(egui::Id::new(("dock width", salt)), width, draw::MOVE);
        let has_bin = shown_items.iter().any(|i| i.section == Section::Bin);
        let (glass, bin) = metrics::islands(window, shown, has_bin, &m);
        let islands: Vec<Island> = std::iter::once(glass).chain(bin).map(|r| Island::new(r, m.radius)).collect();
        let composed = surface::composed(hwnd);
        // Files dragged in from other apps: where the icons are, and what is aimed at.
        self.drops.register(hwnd);
        for (pins, slot) in self.drops.take_pins() {
            self.pin_dropped(pins, slot);
        }
        let lefts: Vec<f32> = shown_items
            .iter()
            .zip(&xs)
            .map(|(item, &x)| match bin.filter(|_| item.section == Section::Bin) {
                Some(b) => b.left() + x,
                None => glass.center().x + x - width / 2.0,
            })
            .collect();
        let gaps = metrics::pin_gaps(&shown_items, &lefts, &m);
        self.drops.publish(hwnd, drop_map(hwnd, &shown_items, &lefts, &gaps, glass, &m, scale));
        let dropping = self.drops.hover_on(hwnd).map(|h| draw::DropMark {
            key: h.key,
            gap: match h.aim {
                drop::Aim::Gap(slot) => gaps.iter().find(|g| g.1 == slot).map(|g| g.0),
                _ => None,
            },
            ok: h.ok,
        });
        let events = egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ctx, |ui| {
                // Room under the icons for the islands' own fill, when it is painted here.
                let under = ui.painter().add(egui::Shape::Noop);
                let scene = draw::Scene { items: &shown_items, front: self.front, metrics: m, xs: &xs, width, glass, bin, salt, tint: style.tint, lifted: held.lifted, dropping };
                let events = draw::paint(ui, &scene, &mut self.thumbs);
                if !composed {
                    // No composed glass: the islands are painted here, without blur or shadow.
                    ui.painter().set(under, egui::Shape::Vec(glass::islands::solid_shapes(&islands, &style)));
                }
                events
            })
            .inner;
        // The islands, and the label while it shows, are all of the window that is drawn and
        // clicked; the rest lets the desktop through.
        let label = events.label.map(|l| metrics::label_region(l, scale));
        surface::set_islands(hwnd, &islands, scale, true, label);
        let inside = ctx
            .pointer_hover_pos()
            .is_some_and(|p| glass.contains(p) || bin.is_some_and(|b| b.contains(p)));
        if let Some((i, origin, grab)) = events.pressed {
            if shown_items[i].pinnable() && !held.busy {
                self.drag.press(salt, &shown_items[i].app_id, origin, grab);
                // A press and its release can arrive in one frame (a quick click on an idle
                // bar). Nothing would then wake the window to see the release, and the stale
                // press would swallow the next one: it is a click, which egui reports itself.
                if !ctx.input(|i| i.pointer.primary_down()) {
                    self.drag.cancel();
                }
            }
        }
        if held.busy {
            // Nothing else reacts to the pointer while an icon is moved (or was, this frame).
            return Hover { target: None, inside, dismiss: true };
        }
        self.act(ctx, &events, inside, glass.top())
    }

    /// Advance a drag in this dock window by one frame: the pointer and Esc, then what the other
    /// icons should show and where the icon in hand is drawn. A drop saves the new order.
    fn drag_frame(&mut self, ctx: &egui::Context, salt: &str, window: egui::Vec2, m: &Metrics) -> Held {
        let mut held = Held::default();
        if !self.drag.is_for(salt) {
            return held;
        }
        let (pos, down) = ctx.input(|i| (i.pointer.hover_pos(), i.pointer.primary_down()));
        // Bars never take the keyboard focus, so Esc is read from the keyboard itself.
        let esc = unsafe { GetAsyncKeyState(i32::from(VK_ESCAPE.0)) } as u16 & 0x8000 != 0;
        let step = self.drag.update(pos, down, esc);
        let (xs, width) = metrics::layout(&self.items, m);
        let (glass, _) = metrics::islands(window, width, false, m);
        let aim = |this: &Self, id: &str, grab: f32, p: egui::Pos2| -> Aim {
            // Along the dock only, and not past the glass's ends.
            let left = (p.x - grab).clamp(glass.left(), (glass.right() - m.icon).max(glass.left()));
            let inside = glass.expand2(egui::vec2(0.0, 8.0)).contains(p);
            let center = left + m.icon / 2.0 - glass.center().x + width / 2.0;
            let slot = inside.then(|| items::drop_slot(&this.items, &xs, m.icon, id, center)).flatten();
            Aim { id: id.to_owned(), left, slot }
        };
        match step {
            drag::Step::Idle | drag::Step::Pending | drag::Step::Click => {}
            drag::Step::Dragging(_) => {
                held.busy = true;
                // Out of the window the icon stays where it was last seen.
                let hand = self.drag.held().map(|(id, grab)| (id.to_owned(), grab)).zip(self.drag.last());
                if let Some(a) = hand.map(|((id, grab), p)| aim(self, &id, grab, p)) {
                    if self.items.iter().any(|i| i.app_id == a.id) {
                        held.preview = a.slot.map(|k| items::preview(&self.items, &a.id, k));
                        held.lifted = Some(draw::Lifted { app_id: a.id, left: a.left });
                    } else {
                        // The app went away while in hand.
                        self.drag.cancel();
                    }
                }
                // Esc cannot wake a window by itself: look again soon.
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            drag::Step::Drop { id, grab, pos } => {
                held.busy = true;
                // Let go out of the window: nothing to drop on.
                if let Some(Aim { slot: Some(k), .. }) = pos.map(|p| aim(self, &id, grab, p)) {
                    self.drop_at(&id, k);
                }
            }
            drag::Step::Cancel => {
                held.busy = true;
            }
        }
        held
    }

    /// Put the app at pinned slot `k`, pinning it if it was only running, and save at once. The
    /// other docks follow when the bars are woken.
    fn drop_at(&mut self, app_id: &str, k: usize) {
        let Some(item) = self.items.iter().find(|i| i.app_id == app_id) else { return };
        let pin = Pin { app_id: item.app_id.clone(), exe: item.exe.clone() };
        if items::move_pin(&mut self.pins.pins, &self.files, pin, k) {
            self.pins.save();
            self.rebuild();
            self.wake.bars();
        }
    }
}

impl Dock {
    /// Folders, programs or shortcuts dropped between the pins: pin them there, in order, and
    /// save at once.
    fn pin_dropped(&mut self, pins: Vec<Pin>, slot: usize) {
        let mut changed = false;
        for (k, pin) in pins.into_iter().enumerate() {
            changed |= items::move_pin(&mut self.pins.pins, &self.files, pin, slot + k);
        }
        if changed {
            self.pins.save();
            self.rebuild();
            self.wake.bars();
        }
    }
}

/// The drop areas of one dock window's icons, in screen pixels: each icon's cell (half the
/// spacing to each side, the glass's height), and the boundaries a pin may be dropped on.
fn drop_map(hwnd: isize, items: &[Item], lefts: &[f32], gaps: &[(f32, usize)], glass: egui::Rect, m: &Metrics, scale: f32) -> drop::Map {
    let (ox, oy) = crate::bars::client_origin(hwnd).unwrap_or((0, 0));
    let px = |v: f32| (v * scale).round() as i32;
    let (top, bottom) = (oy + px(glass.top()), oy + px(glass.bottom()));
    let icons = items
        .iter()
        .zip(lefts)
        .map(|(item, &left)| drop::Spot {
            rect: [ox + px(left - 4.0), top, ox + px(left + m.icon + 4.0), bottom],
            key: item.app_id.clone(),
            icon: match item.section {
                Section::Launcher => drop::Icon::Launcher,
                Section::Files => drop::Icon::Files,
                Section::Bin => drop::Icon::Bin,
                Section::Pinned | Section::Running => drop::Icon::App {
                    app_id: item.app_id.clone(),
                    exe: item.exe.clone(),
                    front: item.windows.first().copied(),
                },
            },
        })
        .collect();
    drop::Map {
        icons,
        gaps: gaps.iter().map(|&(x, slot)| (ox + px(x), slot)).collect(),
        // The outer quarter of the icons on each side of a boundary.
        zone: px(m.icon / 4.0 + 4.0),
    }
}

/// What a frame of a drag changes in a dock window.
#[derive(Default)]
struct Held {
    /// The icons in the order they would have after a drop here, while one is in hand.
    preview: Option<Vec<Item>>,
    /// The icon in hand and where it is drawn.
    lifted: Option<draw::Lifted>,
    /// A drag is on or ended this frame: the pointer belongs to it.
    busy: bool,
}

/// Where the icon in hand is, and the pinned slot a drop there would take.
struct Aim {
    id: String,
    left: f32,
    slot: Option<usize>,
}

/// A window's client size, in pixels.
fn client_size(hwnd: isize) -> (i32, i32) {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(HWND(hwnd as *mut _), &mut rc);
    }
    (rc.right - rc.left, rc.bottom - rc.top)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn window(exe: Option<&str>) -> AppWindow {
        AppWindow { hwnd: 1, title: "egui window".into(), app_id: String::new(), exe: exe.map(PathBuf::from), minimized: false }
    }

    #[test]
    fn bar_windows_are_never_icons() {
        assert!(is_bar_window(&window(Some(r"X:\shellman\target\debug\shellman-bar.exe"))));
        assert!(is_bar_window(&window(Some(r"C:\Apps\SHELLMAN-BAR.EXE"))));
        assert!(!is_bar_window(&window(Some(r"C:\Apps\shellman-files.exe"))));
        assert!(!is_bar_window(&window(None)));
    }
}
