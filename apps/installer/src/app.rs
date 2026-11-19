//! The installer's window, in Glasshouse's language: a frameless window that is one sheet of
//! glass with no sidebar. A header bar is the title bar ("shellman / Install", centred, with the
//! minimise and close buttons flush in the corner); under it the page's boxes (a small title
//! above each, rows inside), and the actions at the bottom right with the main one last. Install:
//! choices, then the work, then the outcome. Uninstall: what will happen, then the work, then
//! the outcome.

use crate::payload::Payload;
use crate::winsys;
use crate::work::{self, Choice, Current, Msg, Outcome};
use egui::{pos2, vec2, Align, Align2, Color32, CornerRadius, Layout, Margin, Rect, RichText, Sense, Stroke, StrokeKind, UiBuilder};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_setup::ops::UninstallPlan;
use shellman_ui::appearance::Appearance;
use shellman_ui::chips;
use shellman_ui::glass::islands::{self, Island};
use shellman_ui::glass::{GlassPanel, Style};
use shellman_ui::sheet::{self, Sheet};
use shellman_ui::theme::{self, colors, glyph, semibold_font};
use shellman_ui::{app_icon, window};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

/// The window's corner radius.
const RADIUS: f32 = sheet::RADIUS;
/// Space between the window's side edges and the boxes.
const SIDE: f32 = 24.0;
/// Space between one box and the next.
const GAP: f32 = 20.0;
/// The bottom strip holding the actions.
const FOOTER_H: f32 = 64.0;
/// A box's padding at the sides (rows' text starts this far in; hairlines between rows run
/// through it to the box's edges), and above and below a row's text.
const PAD_X: f32 = 18.0;
const ROW_PAD: f32 = 11.0;
const SIZE: [f32; 2] = [660.0, 660.0];

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
    /// Install: Firefox is there, so the wizard offers to style it.
    firefox_here: bool,
    /// Uninstall: the profiles with the look, whether to take it out, and whether Firefox was
    /// found open when Uninstall was pressed.
    ff_profiles: Vec<PathBuf>,
    remove_firefox: bool,
    ff_open: bool,
    /// Blurman is installed: the wizard says so instead of offering it; the uninstall window
    /// offers to remove it.
    blurman_here: bool,
    remove_blurman: bool,
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

/// The rows of a box: the hairline goes above every row but the first.
struct Rows {
    first: bool,
    /// The box's title, which names a field in it for a screen reader.
    label: egui::Id,
}

impl Rows {
    /// One row holding anything: its room above and below, and the hairline above it.
    fn row<R>(&mut self, ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
        if !self.first {
            separator(ui);
        }
        self.first = false;
        ui.add_space(ROW_PAD);
        let r = add(ui);
        ui.add_space(ROW_PAD);
        r
    }

    /// A row with a switch: `title` and a muted `detail` on the left.
    fn switch(&mut self, ui: &mut egui::Ui, title: &str, detail: &str, on: &mut bool) -> egui::Response {
        self.row(ui, |ui| theme::setting_row(ui, title, detail, on))
    }

    /// A row of wrapped text.
    fn text(&mut self, ui: &mut egui::Ui, text: &str, color: Color32) {
        self.row(ui, |ui| para(ui, text, color));
    }
}

/// A hairline across the whole box, through its padding to its edges.
fn separator(ui: &mut egui::Ui) {
    let c = colors();
    let inner = ui.max_rect();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    let edge = if c.high_contrast { c.text } else { c.hairline };
    ui.painter().hline((inner.left() - PAD_X)..=(inner.right() + PAD_X), rect.center().y, Stroke::new(1.0_f32, edge));
}

/// Glasshouse's boxed list: `title` small, upper case and grey above a box in the card fill with
/// a hairline, holding `add`'s rows. `min_height` keeps the box at least that tall. Returns the
/// box's rectangle.
fn boxed(ui: &mut egui::Ui, title: &str, min_height: f32, add: impl FnOnce(&mut egui::Ui, &mut Rows)) -> Rect {
    let mut label = egui::Id::NULL;
    if !title.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(PAD_X - 4.0);
            label = ui.label(theme::group_heading(title)).id;
        });
        ui.add_space(6.0);
    }
    let frame = shellman_ui::sheet::card_frame(Margin::symmetric(PAD_X as i8, 2));
    let out = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height((min_height - 4.0).max(0.0));
        add(ui, &mut Rows { first: true, label });
    });
    ui.add_space(GAP);
    out.response.rect
}

/// A window button (minimize or close) flush in the header, as Windows 11 draws its own and as
/// `chips::window_buttons_flush` draws them in the other apps: a thin glyph, a faint square wash
/// under the pointer (`round` is the shape of the wash: square, or the window's corner for close),
/// and close turning Windows' red with a white glyph. That set always has a maximize button,
/// which this one-size window has not, so the two it needs are drawn here. A disabled one (close
/// while the work runs) only shows a dimmed glyph.
fn caption(ui: &mut egui::Ui, rect: Rect, id: &str, g: char, enabled: bool, close: bool, name: &str, round: CornerRadius) -> egui::Response {
    let c = colors();
    let resp = ui.interact(rect, egui::Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    // Enabled or not as the flag says: a hover-only response still counts as enabled to egui.
    shellman_ui::access::describe(&resp, || egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
    let resp = resp.on_hover_text(name);
    theme::focus_ring(ui, &resp, rect, 6.0);
    let (hovered, pressed) = (enabled && resp.hovered(), resp.is_pointer_button_down_on());
    // Windows' own close red, and its pressed shade.
    let red = if pressed { Color32::from_rgb(200, 60, 49) } else { Color32::from_rgb(196, 43, 28) };
    let (fill, fg) = match (hovered, close, c.high_contrast) {
        // High contrast has no red and no washes: highlight under highlight text.
        (true, _, true) => (c.accent_fill, c.on_accent),
        (true, true, false) => (red, Color32::WHITE),
        (true, false, false) => (if pressed { c.press_soft } else { c.hover_soft }, c.text),
        (false, _, _) if !enabled => (Color32::TRANSPARENT, theme::with_alpha(c.muted, 90)),
        _ => (Color32::TRANSPARENT, c.text),
    };
    ui.painter().rect_filled(rect, round, fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, g, theme::icon_font(10.0), fg);
    resp
}

/// Body text that wraps within its box.
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
            firefox: false,
            blurman: false,
        };
        let blurman_here = crate::blurman::installed();
        let mut choice = choice;
        choice.blurman = matches!(mode, Mode::Install(_)) && !blurman_here;
        let (firefox_here, ff_profiles) = match mode {
            Mode::Install(_) => (work::firefox_installed(), Vec::new()),
            Mode::Uninstall => (false, work::firefox_profiles_with_look()),
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
            firefox_here,
            remove_firefox: !ff_profiles.is_empty(),
            ff_profiles,
            ff_open: false,
            blurman_here,
            remove_blurman: false,
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
            Mode::Uninstall => {
                let firefox = if self.remove_firefox { self.ff_profiles.clone() } else { Vec::new() };
                // Once shellman is gone nothing is left to take the look out: wait for Firefox.
                self.ff_open = work::firefox_open(&firefox);
                if self.ff_open {
                    return;
                }
                work::uninstall(PathBuf::from(&self.prefix), self.purge, firefox, self.remove_blurman, tx, wake)
            }
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

    /// The header bar, which is the title bar: "shellman / <page>" centred, and the window's
    /// buttons flush in the corner (no maximize: the window has one size).
    fn header(&self, ui: &mut egui::Ui, sheet: &Sheet) {
        let c = colors();
        let bar = sheet.header;
        let drag = ui.interact(bar, egui::Id::new("header-drag"), Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        let title = format!("shellman  /  {}", self.page_name());
        let galley = ui.fonts(|f| f.layout_no_wrap(title, semibold_font(15.0), c.text));
        ui.painter().galley(bar.center() - galley.size() / 2.0, galley, c.text);
        let working = matches!(self.page, Page::Working);
        let r = sheet.radius.round().clamp(0.0, 255.0) as u8;
        let w = chips::CAPTION_W;
        let close = Rect::from_min_size(pos2(bar.right() - w, bar.top()), vec2(w, bar.height()));
        // The work is not stopped half-way: no closing while it runs.
        let corner = CornerRadius { nw: 0, ne: r, sw: 0, se: 0 };
        if caption(ui, close, "close", glyph::CHROME_CLOSE, !working, true, "Close", corner).clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let min = close.translate(vec2(-w, 0.0));
        if caption(ui, min, "minimize", glyph::MINIMIZE, true, false, "Minimize", CornerRadius::ZERO).clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
    }

    /// Install: where, then what else.
    fn choose_install(&mut self, ui: &mut egui::Ui) {
        let version = shellman_setup::integrate::display_version(shellman_buildinfo::STAMP);
        boxed(ui, "Install folder", 0.0, |ui, rows| {
            let label = rows.label;
            rows.row(ui, |ui| {
                let field = ui.add_enabled(!self.was.installed, egui::TextEdit::singleline(&mut self.prefix).desired_width(f32::INFINITY).margin(Margin::symmetric(10, 7)));
                // Named by its title for a screen reader. (Not `accesskit_node_builder`: in egui
                // 0.31 calling it during the first frame stops the window from ever painting.)
                field.labelled_by(label);
            });
            let note = if self.was.installed {
                let old = self.was.stamp.as_deref().map(shellman_setup::integrate::display_version).unwrap_or_else(|| "?".into());
                format!("Installed: {old}. This installer: {version}. Updating keeps the folder.")
            } else {
                format!("Version {version}. For you only; no administrator rights needed.")
            };
            rows.text(ui, &note, colors().muted);
        });
        boxed(ui, "Options", 0.0, |ui, rows| {
            rows.switch(ui, "Start menu shortcuts", "Files, Settings, Task Manager and Terminal.", &mut self.choice.shortcuts);
            rows.switch(ui, "Open and Save dialogs use shellman Files", "In other apps, from their next start.", &mut self.choice.hook);
            let detail = if self.wt { "Console programs open in shellman Terminal." } else { "Needs Windows Terminal, which isn't installed." };
            ui.add_enabled_ui(self.wt || self.choice.terminal, |ui| {
                rows.switch(ui, "shellman Terminal is the default terminal", detail, &mut self.choice.terminal);
            });
            rows.switch(
                ui,
                "Make shellman my Windows shell",
                "From the next sign-in, instead of Explorer. Hold Shift while signing in to get Explorer.",
                &mut self.choice.shell,
            );
            if self.firefox_here {
                rows.switch(
                    ui,
                    "Also style Firefox",
                    "The Islands look in Firefox's default profile, with its live-look scripts (their loader asks for administrator rights once). Settings \u{203a} Appearance \u{203a} Firefox changes it.",
                    &mut self.choice.firefox,
                );
            }
            if self.blurman_here {
                rows.text(ui, "Blurman is already installed.", colors().muted);
            } else {
                rows.switch(
                    ui,
                    "Also install Blurman",
                    "Blur behind other apps' windows, from the Microsoft Store. An app of its own: uninstalling shellman keeps it.",
                    &mut self.choice.blurman,
                );
            }
        });
    }

    /// Uninstall: what will happen, then the one choice.
    fn choose_uninstall(&mut self, ui: &mut egui::Ui) {
        let c = colors();
        let Some(p) = &self.plan else {
            boxed(ui, "Something went wrong", 0.0, |ui, rows| rows.text(ui, "The install couldn't be read.", c.muted));
            return;
        };
        if p.files.is_empty() && !p.listed {
            boxed(ui, "Nothing to remove", 0.0, |ui, rows| rows.text(ui, &format!("shellman isn't installed in {}.", self.prefix), c.text));
            return;
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
        boxed(ui, "What happens", 0.0, |ui, rows| {
            for i in &items {
                rows.text(ui, i, c.text);
            }
            rows.text(ui, &format!("From {from}"), c.muted);
        });
        boxed(ui, "Also", 0.0, |ui, rows| {
            rows.switch(ui, "Also remove my settings", "Your pins, look and app settings. Kept otherwise, for a later install.", &mut self.purge);
            if !self.ff_profiles.is_empty() {
                let n = self.ff_profiles.len();
                let detail = if n == 1 { "From your Firefox profile; what it replaced is put back.".to_string() } else { format!("From {n} Firefox profiles; what it replaced is put back.") };
                if rows.switch(ui, "Also remove the look from Firefox", &detail, &mut self.remove_firefox).changed() {
                    self.ff_open = false;
                }
                if self.ff_open {
                    rows.text(ui, "Firefox is open. Close it, then press Uninstall again.", c.danger_text);
                }
            }
            if self.blurman_here {
                rows.switch(ui, "Also remove Blurman", "It is an app of its own, kept otherwise.", &mut self.remove_blurman);
            }
        });
    }

    /// The work: a box holding the headline, the progress bar and the log, down to the footer.
    fn working(&mut self, ui: &mut egui::Ui) {
        let what = if self.installing() { if self.was.installed { "Updating shellman…" } else { "Installing shellman…" } } else { "Removing shellman…" };
        let height = ui.available_height() - GAP;
        boxed(ui, "", height, |ui, rows| {
            rows.row(ui, |ui| {
                ui.label(RichText::new(what).font(semibold_font(15.0)).color(colors().text));
                ui.add_space(4.0);
                // A thin pill: the accent on a track in the field fill (egui draws the track in
                // the field colour of the visuals). High contrast outlines the track, which is
                // the window's own colour there.
                let c = colors();
                let bar = egui::ProgressBar::new(self.progress.unwrap_or(0.5)).desired_height(6.0).corner_radius(3.0).fill(c.accent_fill);
                let shown = ui.scope(|ui| {
                    ui.visuals_mut().extreme_bg_color = c.field;
                    ui.add(if self.progress.is_none() { bar.animate(true) } else { bar })
                });
                if c.high_contrast {
                    let r = shown.inner.rect;
                    ui.painter().rect_stroke(r, theme::shape::pill(r), Stroke::new(1.0_f32, c.text), StrokeKind::Outside);
                }
                ui.add_space(8.0);
                self.log_view(ui);
            });
        });
    }

    fn log_view(&self, ui: &mut egui::Ui) {
        let c = colors();
        egui::Frame::new().fill(c.field).stroke(egui::Stroke::new(1.0_f32, c.hairline)).corner_radius(theme::shape::corner(theme::shape::FIELD)).inner_margin(Margin::same(10)).show(ui, |ui| {
            egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).max_height(ui.available_height() - 4.0).show(ui, |ui| {
                for l in &self.log {
                    ui.add(egui::Label::new(RichText::new(l).monospace().size(11.5).color(c.muted)).truncate());
                }
            });
        });
    }

    fn done(&mut self, ui: &mut egui::Ui) {
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
            _ => return,
        };
        let height = if self.show_log { ui.available_height() - GAP } else { 0.0 };
        boxed(ui, "", height, |ui, rows| {
            rows.row(ui, |ui| {
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
        });
    }

    /// The bottom strip: a muted line, or the actions at the right (the main one last).
    fn footer(&mut self, ui: &mut egui::Ui, rect: Rect, ctx: &egui::Context) {
        let inner = Rect::from_min_max(pos2(rect.left() + SIDE, rect.top()), pos2(rect.right() - SIDE, rect.bottom()));
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
        self.panel.set_style(Style::from_theme(RADIUS));
        let mut list: Vec<Island> = Vec::new();
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
            let screen = ctx.screen_rect();
            let under = ui.painter().add(egui::Shape::Noop);
            let sheet = Sheet::new(ctx, screen, 0.0);
            list.push(sheet.island());
            sheet.paint(ui);
            self.header(ui, &sheet);
            let body = sheet.body;
            let footer = Rect::from_min_max(pos2(body.left(), body.bottom() - FOOTER_H), body.max);
            let page = Rect::from_min_max(pos2(body.left() + SIDE, body.top() + 8.0), pos2(body.right() - SIDE, footer.top()));
            ui.scope_builder(UiBuilder::new().max_rect(page), |ui| {
                ui.set_clip_rect(page);
                // The choices may outgrow the window (a larger text size); the work and the
                // outcome fit it.
                let scrolls = matches!(self.page, Page::Choose);
                let add = |app: &mut App, ui: &mut egui::Ui| match app.page {
                    Page::Choose if app.installing() => app.choose_install(ui),
                    Page::Choose => app.choose_uninstall(ui),
                    Page::Working => app.working(ui),
                    Page::Done(_) => app.done(ui),
                };
                if scrolls {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| add(self, ui));
                } else {
                    add(self, ui);
                }
            });
            self.footer(ui, footer, ctx);
            if !self.panel.composed() {
                // High contrast, `SHELLMAN_SOLID`: a plain solid window. (Over Remote Desktop
                // the sheet alone, nearly opaque.)
                ui.painter().set(under, egui::Shape::Vec(islands::under(screen, &list, &self.panel.style())));
            }
        });
        self.panel.set_islands(&list, ctx.pixels_per_point());
    }
}
