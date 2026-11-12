//! The top bar: a strip of glass along the top edge. The launcher mark and the app in front sit
//! on the left; the clock, status and the tray on the right. Tray icons join the right side once
//! shellman owns the tray, on the primary monitor's bar only, so they keep stable places.
//!
//! Every monitor has a top bar window; they all draw this one `TopBar` (`bars.rs` owns the
//! windows and their places).
//!
//! It repaints when the app in front changes, on its own input, and once a minute for the clock;
//! otherwise it is idle.

mod left;
mod right;


use crate::tray::{demo, view::TrayTextures, Tray, TrayIcon};
use shellman_core::settings::bar::BarSettings;
use shellman_core::system::{clock, status};
use shellman_core::winlist::AppWindow;
use shellman_ui::theme::{self, colors, Colors};
use std::time::{Duration, Instant};
use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP, VK_LWIN};

pub const HEIGHT: f32 = 32.0;
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

impl TopBar {
    /// `test` is the `--no-shell` bar: it has no tray, so it shows a few stand-in icons.
    pub fn new(tray: Option<Tray>, test: bool) -> Self {
        Self { tray, demo: if test { demo::icons() } else { Vec::new() }, ..Default::default() }
    }

    /// Read the bar's settings again, when something saved them (the settings app).
    pub fn reload_settings(&mut self) {
        self.settings = None;
    }

    /// One frame of one top bar window. `with_tray` is true for the primary monitor's bar.
    pub fn frame(&mut self, ctx: &egui::Context, front: Option<&AppWindow>, glass: bool, with_tray: bool) {
        self.draw(ctx, front, glass, with_tray);
        ctx.request_repaint_after(clock::until_next_minute());
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

    fn draw(&mut self, ctx: &egui::Context, front: Option<&AppWindow>, glass: bool, with_tray: bool) {
        theme::set_light(crate::app::light(ctx));
        let c = colors();
        // Over composed glass (the dock's material, under the window) only a scrim is filled
        // here, so the bar's small muted text stays legible at the clearest glass settings (at
        // glass 1 over a white window the date all but vanished); without glass, the bar paints
        // a solid surface.
        let style = crate::bars::top_style();
        let mut surface = shellman_ui::glass::content_frame(glass, &style).corner_radius(0);
        if glass {
            surface = surface.fill(scrim(style.tint));
        }
        egui::CentralPanel::default()
            .frame(surface)
            .show(ctx, |ui| {
                let bar = ui.max_rect();
                paint_edge(ui, bar, &c);
                let limit = self.right_side(ui, bar, with_tray);
                left::draw(ui, bar, front, limit - 14.0);
            });
    }
}

/// What tops the glass's tint up to at least `COVER` of the backdrop hidden, in the tint's own
/// colour; transparent when the tint already hides that much.
fn scrim(tint: egui::Color32) -> egui::Color32 {
    const COVER: f32 = 0.85;
    let a = tint.a() as f32 / 255.0;
    let extra = (1.0 - (1.0 - COVER) / (1.0 - a).max(0.001)).max(0.0);
    theme::with_alpha(tint.to_opaque(), (extra * 255.0).round() as u8)
}

/// The one hairline along the bar's lower edge.
fn paint_edge(ui: &egui::Ui, bar: egui::Rect, c: &Colors) {
    let line = 1.0 / ui.ctx().pixels_per_point();
    let edge = egui::Rect::from_min_max(egui::pos2(bar.left(), bar.bottom() - line), bar.max);
    ui.painter().rect_filled(edge, 0.0, c.bar_edge);
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
