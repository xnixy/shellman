//! Opens shellman's glass popup menu once at a point, to look at and capture, and prints what
//! was picked. Not part of the shell.
//!
//! `popup_menu <x> <y> [--up] [--light | --dark] [--glass <0..1>] [--sub] [--close-after <ms>] [--linger <ms>]`
//!
//! `x y` are physical screen pixels. `--sub` opens the "New" submenu; `--close-after` closes the
//! menu by itself (for captures). Prints `picked <id>` or `closed`, then exits.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_ui::glass;
use shellman_ui::menu::popup::{self, Anchor, Entry, PopupMenu};
use shellman_ui::theme::{self, glyph};
use std::time::{Duration, Instant};

struct Args {
    at: Anchor,
    light: Option<bool>,
    glass: Option<f32>,
    sub: bool,
    close_after: Option<Duration>,
    /// Stay this long after the menu closed (to measure what a closed menu costs).
    linger: Option<Duration>,
}

fn args() -> Option<Args> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let x = a.first()?.parse().ok()?;
    let y = a.get(1)?.parse().ok()?;
    let flag = |f: &str| a.iter().any(|s| s == f);
    let value = |f: &str| a.iter().position(|s| s == f).and_then(|i| a.get(i + 1)).and_then(|v| v.parse::<f32>().ok());
    Some(Args {
        at: if flag("--up") { Anchor::above(x, y) } else { Anchor::at(x, y) },
        light: if flag("--light") { Some(true) } else if flag("--dark") { Some(false) } else { None },
        glass: value("--glass"),
        sub: flag("--sub"),
        close_after: value("--close-after").map(|ms| Duration::from_millis(ms.max(0.0) as u64)),
        linger: value("--linger").map(|ms| Duration::from_millis(ms.max(0.0) as u64)),
    })
}

fn entries() -> Vec<Entry<&'static str>> {
    vec![
        Entry::submenu(
            "&New",
            vec![
                Entry::heading("New on the desktop"),
                Entry::item("folder", "&Folder").icon(glyph::NEW_FOLDER),
                Entry::item("text", "&Text document").icon(glyph::FILE),
                Entry::item("note", "&Markdown note").icon(glyph::DOC),
            ],
        )
        .icon(glyph::ADD),
        Entry::item("paste", "&Paste").icon(glyph::PASTE).hint("Ctrl V").enabled(false),
        Entry::separator(),
        Entry::item("files", "Open in &Files").icon(glyph::FOLDER),
        Entry::item("grid", "Show &grid").checked(true),
        Entry::separator(),
        Entry::heading("Danger zone"),
        Entry::item("empty", "&Empty Recycle Bin").icon(glyph::DELETE).danger(),
    ]
}

struct Example {
    args: Args,
    menu: PopupMenu<&'static str>,
    opened: Option<Instant>,
    sub_done: bool,
    closed_at: Option<Instant>,
}

impl eframe::App for Example {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        popup::declare(ctx);
        let Some(opened) = self.opened else {
            self.menu.open(ctx, self.args.at, entries());
            self.opened = Some(Instant::now());
            ctx.request_repaint();
            return;
        };
        if self.closed_at.is_some_and(|_| self.args.linger.is_none()) {
            return;
        }
        if let Some(id) = self.menu.take_pick() {
            println!("picked {id}");
            self.closed_at = Some(Instant::now());
            self.args.linger = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let age = opened.elapsed();
        if self.args.sub && !self.sub_done && self.menu.is_open() && age > Duration::from_millis(300) {
            self.sub_done = true;
            popup::debug_open_submenu(0);
        }
        if let Some(limit) = self.args.close_after {
            if age >= limit {
                self.menu.close();
            } else {
                ctx.request_repaint_after(limit - age);
            }
        }
        if self.args.sub && !self.sub_done {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        // The window is made on the first frames: wait for it before judging "closed".
        if !self.menu.is_open() && age > Duration::from_millis(500) {
            let since = *self.closed_at.get_or_insert_with(|| {
                println!("closed");
                Instant::now()
            });
            match self.args.linger.map(|l| l.saturating_sub(since.elapsed())) {
                Some(left) if !left.is_zero() => ctx.request_repaint_after(left),
                _ => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        } else if !self.menu.is_open() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
}

fn main() -> eframe::Result {
    let Some(args) = args() else {
        eprintln!("usage: popup_menu <x> <y> [--up] [--light|--dark] [--glass <0..1>] [--sub] [--close-after <ms>]");
        std::process::exit(2);
    };
    let options = eframe::NativeOptions {
        // A one-pixel host, cloaked: only the menu window is seen.
        viewport: egui::ViewportBuilder::default()
            .with_title("shellman popup example")
            .with_inner_size([1.0, 1.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_mouse_passthrough(true)
            .with_taskbar(false),
        ..Default::default()
    };
    eframe::run_native(
        "shellman-popup-example",
        options,
        Box::new(move |cc| {
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(w) = handle.as_raw() {
                    glass::cloak(w.hwnd.get(), true);
                }
            }
            theme::install_fonts(&cc.egui_ctx);
            theme::set_light(args.light.unwrap_or(cc.egui_ctx.theme() == egui::Theme::Light));
            if let Some(g) = args.glass {
                theme::set_glass(&cc.egui_ctx, g);
            }
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(Example { args, menu: PopupMenu::new(), opened: None, sub_done: false, closed_at: None }))
        }),
    )
}
