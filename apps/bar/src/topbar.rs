//! The top bar: the launcher button and the app in front on the left, status and the clock on
//! the right. Tray icons join the right side once shellman owns the tray.
//!
//! It repaints when the app in front changes, on its own input, and once a minute for the clock;
//! otherwise it is idle.

use crate::appbar::AppBar;
use crate::tray::{view::TrayTextures, Tray};
use crate::workarea::WorkArea;
use crate::shape;
use shellman_core::settings::bar::BarSettings;
use shellman_core::system::{app_name, clock, status};
use shellman_core::winlist::AppWindow;
use shellman_ui::theme::{self, colors, icon_font};
use shellman_ui::widgets::draw_text;
use shellman_ui::window;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP, VK_LWIN};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

pub const TITLE: &str = "shellman top bar";
pub const HEIGHT: f32 = 32.0;
/// Status is read again after this long, or right after the user changes it.
const STATUS_TTL: Duration = Duration::from_secs(30);

const VOLUME: char = '\u{E767}';
const MUTED: char = '\u{E74F}';
const GLOBE: char = '\u{E774}';
const LOCAL_NET: char = '\u{E839}';
const BATTERY: char = '\u{E83F}';

/// Space kept free for the dock below maximized windows, in points: its height and two gaps.
const DOCK_RESERVE: f32 = 72.0;

#[derive(Default)]
pub struct TopBar {
    hwnd: Option<isize>,
    /// While Explorer runs, it reserves the bar's space; without it, the work area is set here.
    _appbar: Option<AppBar>,
    _workarea: Option<WorkArea>,
    status: Option<(Instant, Status)>,
    /// The tray, when shellman owns it (Explorer's taskbar is not running).
    tray: Option<Tray>,
    tray_textures: TrayTextures,
    /// Read on first use, and again whenever the settings folder changes.
    settings: Option<BarSettings>,
}

struct Status {
    volume: Option<status::Volume>,
    network: status::Network,
    battery: Option<status::Battery>,
}

impl TopBar {
    /// Read the bar's settings again, when something saved them (the settings app).
    pub fn reload_settings(&mut self) {
        self.settings = None;
    }

    /// Show the bar as a second window of the bar process, across the top of the primary screen.
    pub fn show(&mut self, parent: &egui::Context, front: Option<&AppWindow>, glass: bool) {
        let ppp = parent.pixels_per_point();
        let screen_px = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        let builder = egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_position([0.0, 0.0])
            .with_inner_size([screen_px.0 as f32 / ppp, HEIGHT]);
        parent.show_viewport_immediate(egui::ViewportId::from_hash_of("top-bar"), builder, |ctx, _| {
            self.adopt_window(ctx, screen_px, glass);
            self.draw(ctx, front, glass);
            ctx.request_repaint_after(clock::until_next_minute());
        });
    }

    /// Once the window exists: glass, square corners, never focused, and its space reserved.
    fn adopt_window(&mut self, ctx: &egui::Context, screen_px: (i32, i32), glass: bool) {
        if self.hwnd.is_some() {
            return;
        }
        let Some(hwnd) = shape::own_window(TITLE) else {
            ctx.request_repaint();
            return;
        };
        self.hwnd = Some(hwnd);
        if glass {
            window::frost_window(HWND(hwnd as *mut _));
        }
        shape::square_corners(hwnd);
        shape::never_activate(hwnd);
        let screen = RECT {
            left: 0,
            top: 0,
            right: screen_px.0,
            bottom: screen_px.1,
        };
        let height = (HEIGHT * ctx.pixels_per_point()).round() as i32;
        // Become the tray first: without Explorer, app-bar requests then come to shellman,
        // which declines them, and the work area is set directly instead.
        self.tray = Tray::start(ctx.clone());
        self._appbar = AppBar::top(hwnd, screen, height);
        if self._appbar.is_none() {
            let dock = (DOCK_RESERVE * ctx.pixels_per_point()).round() as i32;
            self._workarea = Some(WorkArea::reserve(height, dock));
        }
    }

    fn status(&mut self) -> &Status {
        let stale = self.status.as_ref().is_none_or(|(at, _)| at.elapsed() > STATUS_TTL);
        if stale {
            let fresh = Status {
                volume: status::volume(),
                network: status::network(),
                battery: status::battery(),
            };
            self.status = Some((Instant::now(), fresh));
        }
        &self.status.as_ref().unwrap().1
    }

    fn draw(&mut self, ctx: &egui::Context, front: Option<&AppWindow>, glass: bool) {
        theme::set_light(ctx.theme() == egui::Theme::Light);
        let c = colors();
        let fill = if glass { c.shell } else { c.bg };
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(fill))
            .show(ctx, |ui| {
                let bar = ui.max_rect();
                let right = self.right_side(ui, bar);
                left_side(ui, bar, front, right);
            });
    }

    /// Status pill and clock, from the right edge. Returns where they begin.
    fn right_side(&mut self, ui: &mut egui::Ui, bar: egui::Rect) -> f32 {
        let c = colors();
        let settings = *self.settings.get_or_insert_with(BarSettings::load);
        let clock_text = clock::text(&settings.clock);
        let font = egui::FontId::proportional(13.0);
        let clock_w = ui.fonts(|f| f.layout_no_wrap(clock_text.clone(), font.clone(), c.text)).size().x;
        let clock_rect = egui::Rect::from_min_max(
            egui::pos2(bar.right() - 14.0 - clock_w, bar.top()),
            egui::pos2(bar.right() - 14.0, bar.bottom()),
        );
        draw_text(ui, clock_rect, &clock_text, font, c.text, true);

        // Status pill: network, volume, battery.
        let st = self.status();
        let mut glyphs: Vec<(char, Option<String>)> = Vec::new();
        glyphs.push(match st.network {
            status::Network::Internet => (GLOBE, None),
            status::Network::Local => (LOCAL_NET, None),
            status::Network::Offline => (GLOBE, Some("offline".into())),
        });
        let vol = st.volume.as_ref().map(|v| (v.level, v.muted));
        if let Some((_, muted)) = vol {
            glyphs.push((if muted { MUTED } else { VOLUME }, None));
        }
        if let Some(b) = &st.battery {
            glyphs.push((BATTERY, Some(format!("{}%", b.percent))));
        }
        let width: f32 = glyphs
            .iter()
            .map(|(_, t)| 22.0 + t.as_ref().map_or(0.0, |t| t.len() as f32 * 7.0 + 4.0))
            .sum::<f32>()
            + 12.0;
        let pill = egui::Rect::from_min_max(
            egui::pos2(clock_rect.left() - 10.0 - width, bar.top() + 4.0),
            egui::pos2(clock_rect.left() - 10.0, bar.bottom() - 4.0),
        );
        let resp = ui.interact(pill, egui::Id::new("status"), egui::Sense::click());
        // Soft pills like Files' toolbar: one for the tray, one for status.
        let pill_radius = egui::CornerRadius::same((pill.height() / 2.0) as u8);
        ui.painter().rect_filled(pill, pill_radius, if resp.hovered() { c.control_hover } else { c.control });
        if let Some(tray) = &self.tray {
            let icons = tray.icons();
            if !icons.is_empty() {
                let right = pill.left() - 8.0;
                let width = icons.len() as f32 * 28.0 + 8.0;
                let group = egui::Rect::from_min_max(egui::pos2(right - width, pill.top()), egui::pos2(right, pill.bottom()));
                ui.painter().rect_filled(group, pill_radius, c.control);
                crate::tray::view::draw(ui, &icons, &mut self.tray_textures, bar, right - 4.0);
            }
        }
        let mut x = pill.left() + 6.0;
        for (g, text) in &glyphs {
            ui.painter().text(
                egui::pos2(x + 11.0, pill.center().y),
                egui::Align2::CENTER_CENTER,
                *g,
                icon_font(14.0),
                c.text,
            );
            x += 22.0;
            if let Some(t) = text {
                ui.painter().text(egui::pos2(x, pill.center().y), egui::Align2::LEFT_CENTER, t, egui::FontId::proportional(12.0), c.muted);
                x += t.len() as f32 * 7.0 + 4.0;
            }
        }
        // Scroll for volume, click to mute.
        if let Some((level, _)) = vol {
            let tip = format!("Volume {:.0}%", level * 100.0);
            let resp = resp.on_hover_text(tip);
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if resp.hovered() && scroll != 0.0 {
                status::set_volume(level + scroll.signum() * 0.02);
                self.status = None;
            }
            if resp.clicked() {
                status::toggle_mute();
                self.status = None;
            }
        }
        pill.left()
    }
}

/// Launcher button in the corner, then the app in front and its window title.
fn left_side(ui: &mut egui::Ui, bar: egui::Rect, front: Option<&AppWindow>, limit: f32) {
    let c = colors();
    let button = egui::Rect::from_min_size(bar.min, egui::vec2(HEIGHT + 8.0, HEIGHT));
    let resp = ui
        .interact(button, egui::Id::new("launcher"), egui::Sense::click())
        .on_hover_text("Start (right-click for settings)");
    if resp.hovered() {
        let round = egui::Rect::from_center_size(button.center(), egui::Vec2::splat(HEIGHT - 6.0));
        ui.painter().circle_filled(round.center(), round.width() / 2.0, c.control_hover);
    }
    // Four small squares: the launcher's mark, drawn so it doesn't depend on a font glyph.
    for (dx, dy) in [(-3.5, -3.5), (3.5, -3.5), (-3.5, 3.5), (3.5, 3.5)] {
        let sq = egui::Rect::from_center_size(button.center() + egui::vec2(dx, dy), egui::Vec2::splat(5.5));
        ui.painter().rect_filled(sq, egui::CornerRadius::same(2), c.accent_text);
    }
    if resp.clicked() {
        open_start();
    }
    if resp.secondary_clicked() {
        shellman_core::sibling::start("settings");
    }
    let Some(w) = front else { return };
    let name = app_name::display_name(&w.app_id, w.exe.as_deref());
    let font = egui::FontId::proportional(13.5);
    let name_w = ui.fonts(|f| f.layout_no_wrap(name.clone(), font.clone(), c.text)).size().x;
    let x = button.right() + 8.0;
    let name_rect = egui::Rect::from_min_max(egui::pos2(x, bar.top()), egui::pos2((x + name_w).min(limit - 16.0), bar.bottom()));
    draw_text(ui, name_rect, &name, font, c.text, false);
    if !w.title.is_empty() && w.title != name {
        let title_left = name_rect.right() + 12.0;
        let title_right = (title_left + 420.0).min(limit - 16.0);
        if title_right > title_left + 40.0 {
            let r = egui::Rect::from_min_max(egui::pos2(title_left, bar.top()), egui::pos2(title_right, bar.bottom()));
            draw_text(ui, r, &w.title, egui::FontId::proportional(12.5), c.muted, false);
        }
    }
}

/// Summon shellman's launcher: its exe sits next to the bar's. Running it again signals the
/// resident one to show or hide, so a click is a toggle. Without the exe, Windows' Start opens,
/// as the Win key does.
fn open_start() {
    let launcher = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("shellman-launcher.exe")))
        .filter(|path| path.exists());
    if let Some(path) = launcher {
        if std::process::Command::new(path).spawn().is_ok() {
            return;
        }
    }
    unsafe {
        keybd_event(VK_LWIN.0 as u8, 0, Default::default(), 0);
        keybd_event(VK_LWIN.0 as u8, 0, KEYEVENTF_KEYUP, 0);
    }
}
