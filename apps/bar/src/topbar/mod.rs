//! The top bar: three floating islands of glass along the top edge, the desktop showing between
//! them. On the left, the launcher mark and the app in front; in the middle, the date and time;
//! on the right, the tray and the status icons. Tray icons join the right island once shellman
//! owns the tray, on every monitor's bar (or the primary's only, by setting).
//!
//! Every monitor has a top bar window; they all draw this one `TopBar` (`bars.rs` owns the
//! windows and their places). The window spans the monitor and is all reserved space; between the
//! islands it is transparent, and a click there does nothing, except in the top-left corner,
//! which opens the launcher as the corner always has.
//!
//! It repaints when the app in front changes, on its own input, and once a minute for the clock;
//! otherwise it is idle.

mod clock;
mod layout;
mod left;
mod right;

pub use layout::{HEIGHT, ISLAND_H};

use crate::tray::{demo, view::{Place, TrayTextures}, Tray, TrayIcon};
use shellman_core::settings::bar::BarSettings;
use shellman_core::system::{clock as system_clock, status};
use shellman_core::winlist::AppWindow;
use shellman_ui::glass::{islands, Island};
use shellman_ui::theme::{self, colors};
use std::time::{Duration, Instant};
use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP, VK_LWIN};

/// Status is read again after this long, or right after the user changes it.
const STATUS_TTL: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct TopBar {
    status: Option<(Instant, Status)>,
    /// The tray, when shellman owns it (Explorer's taskbar is not running).
    tray: Option<Tray>,
    /// Stand-in icons for the test bar, which hosts no tray, so the look can be judged.
    demo: Vec<TrayIcon>,
    tray_textures: TrayTextures,
    /// Read on first use, and again whenever the settings folder changes.
    settings: Option<BarSettings>,
}

struct Status {
    volume: Option<status::Volume>,
    network: status::Network,
    battery: Option<status::Battery>,
}

/// The app in front, as the left island shows it.
pub struct Front<'a> {
    pub window: &'a AppWindow,
    /// Its dock icon, once the shell has given it.
    pub icon: Option<(egui::TextureId, egui::Vec2)>,
}

impl TopBar {
    /// `test` is the `--no-shell` bar: it has no tray, so it shows a few stand-in icons.
    pub fn new(tray: Option<Tray>, test: bool) -> Self {
        Self { tray, demo: if test { demo::icons() } else { Vec::new() }, ..Default::default() }
    }

    /// Read the bar's settings again, when something saved them (the settings app).
    pub fn reload_settings(&mut self) {
        self.settings = None;
    }

    /// One frame of one top bar window, on `place`'s monitor. `composed` says whether Windows
    /// draws the glass under it; if not, the islands are painted here. Returns the islands, for
    /// the window's glass.
    pub fn frame(&mut self, ctx: &egui::Context, front: Option<Front>, composed: bool, place: &Place) -> Vec<Island> {
        let islands = self.draw(ctx, front, composed, place);
        ctx.request_repaint_after(system_clock::until_next_minute());
        islands
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

    fn draw(&mut self, ctx: &egui::Context, front: Option<Front>, composed: bool, place: &Place) -> Vec<Island> {
        theme::set_light(crate::app::light(ctx));
        let c = colors();
        let style = crate::bars::top_style();
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ctx, |ui| {
                let bar = ui.max_rect();
                let row = layout::row(bar);
                // Room under the content for what the islands need painted here.
                let under = ui.painter().add(egui::Shape::Noop);
                let right = self.right_side(ui, row, bar.right() - layout::SIDE, place);
                let settings = *self.settings.get_or_insert_with(BarSettings::load);
                let centre = clock::draw(ui, bar, row, right.left(), &settings);
                let left = left::draw(ui, bar, row, front, centre.left() - layout::SPACE);
                let list: Vec<Island> = [left, centre, right].into_iter().map(Island::pill).collect();
                if !composed {
                    // No composed glass: the islands are painted here, without blur or shadow.
                    ui.painter().set(under, egui::Shape::Vec(islands::solid_shapes(&list, &style)));
                } else if !c.light {
                    // Over composed glass only a scrim is filled, so the bar's small muted text
                    // stays legible at the clearest glass settings (see `scrim`).
                    let fill = scrim(style.tint);
                    let shapes = list
                        .iter()
                        .map(|i| egui::Shape::rect_filled(i.rect, i.radius.round() as u8, fill))
                        .collect();
                    ui.painter().set(under, egui::Shape::Vec(shapes));
                }
                list
            })
            .inner
    }
}

/// What tops the glass's tint up to at least `COVER` of the backdrop hidden, in the tint's own
/// colour; transparent when the tint already hides that much (up to a glass of about 0.65). At
/// that cover the muted text keeps 3:1 over a white window in dark mode. Light mode needs none:
/// its material never shows darker than a light grey.
fn scrim(tint: egui::Color32) -> egui::Color32 {
    const COVER: f32 = 0.55;
    let a = tint.a() as f32 / 255.0;
    let extra = (1.0 - (1.0 - COVER) / (1.0 - a).max(0.001)).max(0.0);
    theme::with_alpha(tint.to_opaque(), (extra * 255.0).round() as u8)
}

/// A soft highlight behind a bar item while it is hovered or pressed.
pub fn highlight(ui: &egui::Ui, rect: egui::Rect, resp: &egui::Response, radius: u8) {
    let c = colors();
    let fill = if resp.is_pointer_button_down_on() {
        c.press_soft
    } else if resp.hovered() {
        c.hover_soft
    } else {
        return;
    };
    ui.painter().rect_filled(rect, egui::CornerRadius::same(radius), fill);
}

/// Summon shellman's launcher: its exe sits next to the bar's. Running it again signals the
/// resident one to show or hide, so a click is a toggle. Without the exe, Windows' Start opens,
/// as the Win key does.
pub fn open_start() {
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
