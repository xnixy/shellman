//! The installer's window, in the islands language: a frameless window where only the islands
//! show, each on glass. A header pill is the title bar ("shellman / Install"); under it the
//! page's islands, and a footer island with the actions, 12 apart everywhere. Install: choices,
//! then the work, then the outcome. Uninstall: what will happen, then the work, then the outcome.

use crate::payload::Payload;
use crate::winsys;
use crate::work::{self, Choice, Current, Msg, Outcome};
use egui::{pos2, vec2, Align, Align2, Color32, Layout, Margin, Rect, RichText, Sense, UiBuilder};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_setup::ops::UninstallPlan;
use shellman_ui::appearance::Appearance;
use shellman_ui::chips::{self, Chip};
use shellman_ui::glass::islands::{self, Island};
use shellman_ui::glass::{GlassPanel, Style};
use shellman_ui::theme::{self, colors, glyph, semibold_font};
use shellman_ui::{app_icon, window};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

/// Gaps: window edge to island, and between islands.
const GAP: f32 = 12.0;
const RADIUS: f32 = 22.0;
const HEADER_H: f32 = 46.0;
const FOOTER_H: f32 = 56.0;
/// Inside a card island.
const PAD: Margin = Margin { left: 22, right: 22, top: 16, bottom: 16 };
const SIZE: [f32; 2] = [660.0, 620.0];

pub enum Mode {
    Install(Payload),
    Uninstall,
}

/// How the window ended, for `main`.
#[derive(Clone, Copy, Default)]
pub struct Ended {
    pub ok: bool,
    pub deferred: bool,
}

enum Page {
    Choose,
    Working,
    Done(Result<Outcome, String>),
}

struct App {
    mode: Mode,
    panel: GlassPanel,
    page: Page,
    prefix: String,
    was: Current,
    choice: Choice,
    wt: bool,
    plan: Option<UninstallPlan>,
    purge: bool,
    log: Vec<String>,
    progress: Option<f32>,
    rx: Option<Receiver<Msg>>,
    show_log: bool,
    ended: Arc<Mutex<Ended>>,
    /// `SHELLMAN_INSTALLER_TEST_START`: press Install / Uninstall at once (to look at the later
    /// pages in a test; use it only with the setup test variables set).
    autostart: bool,
}

pub fn run(mode: Mode, prefix: PathBuf) -> Ended {
    let ended = Arc::new(Mutex::new(Ended::default()));
    let keep = ended.clone();
    let title = match mode {
        Mode::Install(_) => "Install shellman",
        Mode::Uninstall => "Uninstall shellman",
    };
    // Larger with Windows' text size, as far as the screen allows (egui's zoom does the rest).
    let z = theme::env::text_scale();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([SIZE[0] * z, SIZE[1] * z])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_transparent(true)
            .with_decorations(false),
        centered: true,
        // Nothing of the installer's is kept between runs.
        persistence_path: Some(std::env::temp_dir().join(format!("shellman-installer-{}.ron", std::process::id()))),
        ..Default::default()
    };
    let _ = eframe::run_native(
        "shellman-installer",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            cc.egui_ctx.set_theme(theme::env::preference());
            theme::env::follow_text_scale(&cc.egui_ctx);
            theme::set_light(cc.egui_ctx.theme() == egui::Theme::Light);
            Appearance::load_or(Appearance::default()).apply(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx);
            let handle = cc.window_handle().map_err(|e| e.to_string())?;
            let RawWindowHandle::Win32(win32) = handle.as_raw() else { return Err("not a Win32 window".into()) };
            let hwnd = win32.hwnd.get();
            window::frameless(hwnd);
            let panel = GlassPanel::attach_window(hwnd, Style::from_theme(RADIUS));
            Ok(Box::new(App::new(mode, prefix, panel, keep)))
        }),
    );
    let _ = std::fs::remove_file(std::env::temp_dir().join(format!("shellman-installer-{}.ron", std::process::id())));
    let e = *ended.lock().unwrap_or_else(|p| p.into_inner());
    e
}

/// Lay out `add` inside an island's rectangle, with the card margin.
fn inside<R>(ui: &mut egui::Ui, rect: Rect, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let inner = Rect::from_min_max(
        pos2(rect.left() + PAD.left as f32, rect.top() + PAD.top as f32),
        pos2(rect.right() - PAD.right as f32, rect.bottom() - PAD.bottom as f32),
    );
    ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
        ui.set_clip_rect(rect);
        add(ui)
    })
    .inner
}

/// An island's muted group label.
fn group(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let r = ui.label(RichText::new(text).size(12.5).color(colors().muted));
    ui.add_space(2.0);
    r
}

/// Body text that wraps within its island.
fn para(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.add(egui::Label::new(RichText::new(text).size(13.0).color(color)).wrap());
}

impl App {
    fn new(mode: Mode, prefix: PathBuf, panel: GlassPanel, ended: Arc<Mutex<Ended>>) -> App {
        let was = work::current(&prefix);
        let plan = match mode {
            Mode::Uninstall => work::env_for(&prefix).ok().map(|env| shellman_setup::ops::plan_uninstall(&env)),
            Mode::Install(_) => None,
        };
        let choice = Choice {
            prefix: prefix.clone(),
            shortcuts: !was.installed || was.shortcuts,
            shell: was.shell,
            terminal: was.terminal,
            hook: was.hook,
        };
        App {
            mode,
            panel,
            page: Page::Choose,
            prefix: prefix.display().to_string(),
            was,
            choice,
            wt: work::windows_terminal_installed(),
            plan,
            purge: false,
            log: Vec::new(),
            progress: None,
            rx: None,
            show_log: false,
            ended,
            autostart: std::env::var_os("SHELLMAN_INSTALLER_TEST_START").is_some(),
        }
    }

    fn installing(&self) -> bool {
        matches!(self.mode, Mode::Install(_))
    }

    /// The page's name in the header chip.
    fn page_name(&self) -> &'static str {
        match (&self.mode, &self.page) {
            (Mode::Install(_), Page::Done(_)) => "Done",
            (Mode::Install(_), _) if self.was.installed => "Update",
            (Mode::Install(_), _) => "Install",
            (Mode::Uninstall, Page::Done(_)) => "Done",
            (Mode::Uninstall, _) => "Uninstall",
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        let (tx, rx) = channel();
        let c = ctx.clone();
        let wake = move || c.request_repaint();
        match &self.mode {
            Mode::Install(p) => {
                self.choice.prefix = PathBuf::from(self.prefix.trim());
                work::install(p.clone(), self.choice.clone(), self.was.clone(), tx, wake);
            }
            Mode::Uninstall => work::uninstall(PathBuf::from(&self.prefix), self.purge, tx, wake),
        }
        self.rx = Some(rx);
        self.page = Page::Working;
    }

    fn poll(&mut self) {
        let Some(rx) = &self.rx else { return };
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Line(l) => self.log.push(l),
                Msg::Progress(p) => self.progress = p,
                Msg::Done(r) => {
                    if let Ok(mut e) = self.ended.lock() {
                        *e = Ended { ok: r.is_ok(), deferred: r.as_ref().is_ok_and(|o| o.deferred) };
                    }
                    self.show_log = r.is_err();
                    self.page = Page::Done(r);
                }
            }
        }
    }

    /// The header pill, which is the title bar: logo, "shellman / <page>", and the window's
    /// buttons (no maximize: the window has one size).
    fn header(&self, ui: &mut egui::Ui, rect: Rect) {
        let c = colors();
        let drag = ui.interact(rect, egui::Id::new("header-drag"), Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        let y = rect.center().y;
        let (tex, _) = app_icon::kind_texture_px(ui.ctx(), app_icon::Kind::Shellman, 22.0 * ui.ctx().pixels_per_point());
        let icon = Rect::from_center_size(pos2(rect.left() + 28.0, y), vec2(22.0, 22.0));
        ui.painter().image(tex, icon, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        let name = ui.painter().text(pos2(icon.right() + 10.0, y), Align2::LEFT_CENTER, "shellman", semibold_font(17.0), c.text);
        let slash = ui.painter().text(pos2(name.right() + 10.0, y), Align2::LEFT_CENTER, "/", egui::FontId::proportional(17.0), c.muted);
        let label = self.page_name();
        let w = chips::chip_width(ui, label, false, Chip::Accent);
        let chip = Rect::from_center_size(pos2(slash.right() + 10.0 + w / 2.0, y), vec2(w, chips::CHIP_H));
        chips::chip(ui, chip, egui::Id::new("page-chip"), label, None, Chip::Accent);
        let working = matches!(self.page, Page::Working);
        let close = Rect::from_center_size(pos2(rect.right() - 8.0 - 15.0, y), vec2(30.0, 30.0));
        // The work is not stopped half-way: no closing while it runs.
        if chips::round_icon(ui, close, egui::Id::new("close"), glyph::CHROME_CLOSE, !working, true, "Close").on_hover_text("Close").clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let min = close.translate(vec2(-32.0, 0.0));
        if chips::round_icon(ui, min, egui::Id::new("minimize"), glyph::MINIMIZE, true, false, "Minimize").on_hover_text("Minimize").clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
    }

    /// Install: where, then what else. Returns the islands.
    fn choose_install(&mut self, ui: &mut egui::Ui, area: Rect) -> Vec<Rect> {
        let folder = Rect::from_min_size(area.min, vec2(area.width(), 128.0));
        let options = Rect::from_min_max(pos2(area.left(), folder.bottom() + GAP), area.max);
        let version = shellman_setup::integrate::display_version(shellman_buildinfo::STAMP);
        inside(ui, folder, |ui| {
            let label = group(ui, "Install folder");
            let field = ui.add_enabled(!self.was.installed, egui::TextEdit::singleline(&mut self.prefix).desired_width(f32::INFINITY).margin(Margin::symmetric(10, 7)));
            // Named by its label for a screen reader. (Not `accesskit_node_builder`: in egui 0.31
            // calling it during the first frame stops the window from ever painting.)
            field.labelled_by(label.id);
            ui.add_space(6.0);
            let note = if self.was.installed {
                let old = self.was.stamp.as_deref().map(shellman_setup::integrate::display_version).unwrap_or_else(|| "?".into());
                format!("Installed: {old}. This installer: {version}. Updating keeps the folder.")
            } else {
                format!("Version {version}. For you only; no administrator rights needed.")
            };
            ui.add(egui::Label::new(RichText::new(note).size(11.5).color(colors().muted)).wrap());
        });
        inside(ui, options, |ui| {
            ui.spacing_mut().item_spacing.y = 14.0;
            group(ui, "Options");
            theme::setting_row(ui, "Start menu shortcuts", "Files, Settings, Task Manager and Terminal.", &mut self.choice.shortcuts);
            theme::setting_row(ui, "Open and Save dialogs use shellman Files", "In other apps, from their next start.", &mut self.choice.hook);
            let detail = if self.wt { "Console programs open in shellman Terminal." } else { "Needs Windows Terminal, which isn't installed." };
            ui.add_enabled_ui(self.wt || self.choice.terminal, |ui| {
                theme::setting_row(ui, "shellman Terminal is the default terminal", detail, &mut self.choice.terminal);
            });
            theme::setting_row(
                ui,
                "Make shellman my Windows shell",
                "From the next sign-in, instead of Explorer. Hold Shift while signing in to get Explorer.",
                &mut self.choice.shell,
            );
        });
        vec![folder, options]
    }

    /// Uninstall: what will happen, then the one choice.
    fn choose_uninstall(&mut self, ui: &mut egui::Ui, area: Rect) -> Vec<Rect> {
        let settings = Rect::from_min_max(pos2(area.left(), area.bottom() - 84.0), area.max);
        let what = Rect::from_min_max(area.min, pos2(area.right(), settings.top() - GAP));
        let c = colors();
        let Some(p) = &self.plan else {
            inside(ui, area, |ui| {
                group(ui, "Something went wrong");
                para(ui, "The install couldn't be read.", c.muted);
            });
            return vec![area];
        };
        if p.files.is_empty() && !p.listed {
            inside(ui, area, |ui| {
                group(ui, "Nothing to remove");
                para(ui, &format!("shellman isn't installed in {}.", self.prefix), c.text);
            });
            return vec![area];
        }
        let mut items: Vec<String> = Vec::new();
        if p.shell {
            items.push("Explorer becomes your shell again, from the next sign-in.".into());
        }
        if p.terminal {
            items.push("The default terminal goes back to what it was.".into());
        }
        if p.hook {
            items.push("Apps get Windows' own Open and Save dialogs back.".into());
        }
        if p.live_shell {
            items.push("shellman keeps running until you sign out; its files are removed when you sign in again.".into());
        } else if !p.running.is_empty() {
            items.push(format!("{} running shellman app(s) are closed.", p.running.len()));
        }
        items.push(format!("{} files, the Start menu shortcuts and the Apps & features entry are removed.", p.files.len()));
        let from = self.prefix.clone();
        inside(ui, what, |ui| {
            group(ui, "What happens");
            for i in items {
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new("•").color(c.accent_text));
                    para(ui, &i, c.text);
                });
            }
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(format!("From {from}")).size(11.5).color(c.muted)).wrap());
        });
        inside(ui, settings, |ui| {
            theme::setting_row(ui, "Also remove my settings", "Your pins, look and app settings. Kept otherwise, for a later install.", &mut self.purge);
        });
        vec![what, settings]
    }

    fn working(&mut self, ui: &mut egui::Ui, area: Rect) -> Vec<Rect> {
        let what = if self.installing() { if self.was.installed { "Updating shellman…" } else { "Installing shellman…" } } else { "Removing shellman…" };
        inside(ui, area, |ui| {
            ui.label(RichText::new(what).font(semibold_font(15.0)).color(colors().text));
            ui.add_space(4.0);
            let bar = egui::ProgressBar::new(self.progress.unwrap_or(0.5)).desired_height(6.0).corner_radius(3.0);
            ui.add(if self.progress.is_none() { bar.animate(true) } else { bar });
            ui.add_space(8.0);
            self.log_view(ui);
        });
        vec![area]
    }

    fn log_view(&self, ui: &mut egui::Ui) {
        let c = colors();
        egui::Frame::new().fill(c.well).stroke(egui::Stroke::new(1.0_f32, c.hairline)).corner_radius(12.0).inner_margin(Margin::same(10)).show(ui, |ui| {
            egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).max_height(ui.available_height() - 4.0).show(ui, |ui| {
                for l in &self.log {
                    ui.add(egui::Label::new(RichText::new(l).monospace().size(11.5).color(c.muted)).truncate());
                }
            });
        });
    }

    fn done(&mut self, ui: &mut egui::Ui, area: Rect) -> Vec<Rect> {
        let c = colors();
        let installing = self.installing();
        let (ok, title, text) = match &self.page {
            Page::Done(Ok(o)) if !installing && o.deferred => (true, "Almost done", "Sign out and back in: Explorer starts, and shellman's files are removed then.".to_string()),
            Page::Done(Ok(_)) if !installing => (true, "shellman is removed", "Thanks for trying it.".to_string()),
            Page::Done(Ok(o)) if o.sign_out && self.choice.shell => (true, "shellman is installed", "It becomes your shell when you sign in again.".to_string()),
            Page::Done(Ok(o)) if o.sign_out => (true, "shellman is updated", "Explorer is your shell again when you sign in again.".to_string()),
            Page::Done(Ok(_)) if self.was.installed => (true, "shellman is updated", "Your apps were restarted where they could be.".to_string()),
            Page::Done(Ok(_)) => (true, "shellman is installed", "Find Files, Settings, Task Manager and Terminal in the Start menu.".to_string()),
            Page::Done(Err(e)) => (false, "Something went wrong", e.clone()),
            _ => return vec![area],
        };
        inside(ui, area, |ui| {
            ui.horizontal(|ui| {
                let g = if ok { glyph::CHECK } else { glyph::CHROME_CLOSE };
                let col = if ok { c.accent_text } else { c.danger_text };
                ui.label(RichText::new(g.to_string()).font(theme::icon_font(18.0)).color(col));
                ui.add_space(4.0);
                ui.label(RichText::new(title).font(semibold_font(17.0)).color(c.text));
            });
            ui.add_space(2.0);
            para(ui, &text, c.muted);
            ui.add_space(10.0);
            let toggle = ui.add(theme::secondary_button(if self.show_log { "Hide details" } else { "Show details" }));
            if toggle.clicked() {
                self.show_log = !self.show_log;
            }
            if self.show_log {
                ui.add_space(8.0);
                self.log_view(ui);
            }
        });
        vec![area]
    }

    /// The footer island: a muted line on the left, the actions on the right (the main one last).
    fn footer(&mut self, ui: &mut egui::Ui, rect: Rect, ctx: &egui::Context) {
        let inner = Rect::from_min_max(pos2(rect.left() + 22.0, rect.top()), pos2(rect.right() - 13.0, rect.bottom()));
        ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::right_to_left(Align::Center)), |ui| match &self.page {
            Page::Choose => {
                let can = match &self.mode {
                    Mode::Install(_) => !self.prefix.trim().is_empty(),
                    Mode::Uninstall => self.plan.as_ref().is_some_and(|p| !p.files.is_empty() || p.listed),
                };
                let go = match &self.mode {
                    Mode::Install(_) => ui.add_enabled(can, theme::primary_button(if self.was.installed { "Update" } else { "Install" })),
                    Mode::Uninstall => ui.add_enabled(can, theme::danger_button("Uninstall")),
                };
                if go.clicked() {
                    self.start(ctx);
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Page::Working => {
                ui.label(RichText::new("This takes a few seconds.").size(12.5).color(colors().muted));
            }
            Page::Done(r) => {
                let sign_out = r.as_ref().is_ok_and(|o| o.sign_out || o.deferred);
                if sign_out {
                    if ui.add(theme::primary_button("Sign out now")).clicked() {
                        winsys::sign_out();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.add(theme::secondary_button("Later")).clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                } else if ui.add(theme::primary_button("Close")).clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        });
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        if std::mem::take(&mut self.autostart) && matches!(self.page, Page::Choose) {
            self.start(ctx);
        }
        // The work is not stopped half-way.
        if matches!(self.page, Page::Working) && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        app_icon::sync_window_icon(ctx, app_icon::Kind::Shellman);
        let style = Style::from_theme(RADIUS);
        self.panel.set_style(style);
        let mut list: Vec<Island> = Vec::new();
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
            let screen = ctx.screen_rect();
            let under = ui.painter().add(egui::Shape::Noop);
            // The gaps between the islands move the window too.
            islands::drag_gaps(ui, screen);
            let header = Rect::from_min_max(pos2(screen.left() + GAP, screen.top() + GAP), pos2(screen.right() - GAP, screen.top() + GAP + HEADER_H));
            let footer = Rect::from_min_max(pos2(screen.left() + GAP, screen.bottom() - GAP - FOOTER_H), pos2(screen.right() - GAP, screen.bottom() - GAP));
            let area = Rect::from_min_max(pos2(header.left(), header.bottom() + GAP), pos2(header.right(), footer.top() - GAP));
            list.push(Island::pill(header));
            self.header(ui, header);
            let cards = match self.page {
                Page::Choose if self.installing() => self.choose_install(ui, area),
                Page::Choose => self.choose_uninstall(ui, area),
                Page::Working => self.working(ui, area),
                Page::Done(_) => self.done(ui, area),
            };
            list.extend(cards.into_iter().map(|r| Island::new(r, RADIUS)));
            list.push(Island::pill(footer));
            self.footer(ui, footer, ctx);
            if !self.panel.composed() {
                // High contrast, `SHELLMAN_SOLID`: a plain solid window. (Over Remote Desktop the
                // islands alone, nearly opaque: `solid_window` is None there.)
                let mut shapes: Vec<egui::Shape> = islands::solid_window(screen).into_iter().collect();
                shapes.extend(islands::solid_shapes(&list, &self.panel.style()));
                ui.painter().set(under, egui::Shape::Vec(shapes));
            }
        });
        self.panel.set_islands(&list, ctx.pixels_per_point());
    }
}
