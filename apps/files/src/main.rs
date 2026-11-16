#![windows_subsystem = "windows"]
//! Files: a file manager in Blurman's frosted style. With `--pick <request>` it is instead the
//! file dialog another app asked for (see `pick`).

mod app;
mod args;
mod pick;
mod placement;
mod settings;
mod tags;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use shellman_pick::exit;
use shellman_ui::glass::{GlassPanel, Style};
use shellman_ui::{theme, window};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

fn main() {
    // A warming run (see `core::warm`): nothing to do but have run.
    if shellman_core::warm::asked() {
        return;
    }
    shellman_core::timing::init("files");
    let start = args::parse(std::env::args_os().skip(1));
    let session = match &start {
        args::Start::Pick(path) => match shellman_pick::read_request(path) {
            Ok(req) => Some(pick::Session {
                req,
                path: path.clone(),
                exit: Arc::new(AtomicI32::new(exit::CANCELLED)),
            }),
            // The app asking falls back to another dialog on this code.
            Err(_) => std::process::exit(exit::FAILED),
        },
        _ => None,
    };
    let code = session.as_ref().map(|s| s.exit.clone());
    let ran = run(start, session);
    if let Some(code) = code {
        let code = if ran.is_ok() { code.load(Ordering::SeqCst) } else { exit::FAILED };
        std::process::exit(code);
    }
}

fn run(start: args::Start, session: Option<pick::Session>) -> eframe::Result {
    // OLE on the window's thread, for dragging files out, being a drop target, and the
    // clipboard's data objects. The window library's own drop target is off (below): it gives
    // dropped files only once dropped, with no say in the effect and no point to drop on, so
    // Files registers its own (see `app::dnd`).
    unsafe {
        let _ = windows::Win32::System::Ole::OleInitialize(None);
    }
    let settings = settings::load_settings();
    shellman_core::timing::mark("settings loaded");
    let picking = session.is_some();
    let owner = session.as_ref().map_or(0, |s| s.req.owner);
    let size = if picking { placement::PICK_SIZE } else { placement::DEFAULT_SIZE };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Files")
        .with_inner_size([size.0, size.1])
        .with_min_inner_size([placement::MIN_SIZE.0, placement::MIN_SIZE.1])
        .with_transparent(true)
        .with_decorations(false)
        .with_drag_and_drop(false);
    let at = if picking { pick_rect(owner) } else { browse_rect(&settings) };
    if let Some(at) = at {
        // The builder takes points, so the pixel rectangle is divided by the scale of the
        // monitor it lands on.
        let scale = placement::scale_at(at.left, at.top);
        viewport = viewport
            .with_position([at.left as f32 / scale, at.top as f32 / scale])
            .with_inner_size([at.width() as f32 / scale, at.height() as f32 / scale]);
    }
    let options = eframe::NativeOptions {
        viewport,
        // Placement is ours (above). The window library's own memory of the last position kept
        // pixels from one monitor and applied them on another, and it loads whatever file it
        // finds, so its state lives in a file of ours that never holds a window. A picker keeps
        // none, so it never writes over the browsing window's.
        persist_window: false,
        persistence_path: if picking {
            None
        } else {
            shellman_core::config::config_dir().map(|d| d.join("files-ui.ron"))
        },
        ..Default::default()
    };
    shellman_core::timing::mark("eframe start (window, GL context)");
    eframe::run_native(
        "shellman-files",
        options,
        Box::new(move |cc| {
            shellman_ui::timing::gl_ready(cc.gl.as_deref());
            theme::install_fonts(&cc.egui_ctx);
            theme::set_light(
                std::env::var_os("SHELLMAN_FORCE_LIGHT").is_some()
                    || cc.egui_ctx.theme() == egui::Theme::Light,
            );
            // The shared look, which the settings app edits; Files only follows it.
            let look = shellman_ui::appearance::Appearance::load_or(Default::default());
            look.apply(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx);
            // Windows' text size makes everything larger.
            theme::env::follow_text_scale(&cc.egui_ctx);
            let mut hwnd = 0isize;
            let mut panel = None;
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    hwnd = win32.hwnd.get();
                    // A picker belongs to the asking app's window: above it, and with it.
                    pick::host::set_owner(hwnd, owner);
                    let _timing = shellman_core::timing::span("files: place, frame and glass the window");
                    if let Some(at) = at {
                        placement::move_to(hwnd, at);
                    }
                    // No frame and no window backdrop: the window is transparent, and its
                    // islands are pieces of glass under it (see `app::layout`). The resize edges
                    // and Snap stay.
                    window::frameless(hwnd);
                    panel = Some(GlassPanel::attach_window(hwnd, Style::from_theme(app::RADIUS)));
                }
            }
            let _built = shellman_core::timing::span("app built");
            Ok(Box::new(app::FileMan::new(
                cc.egui_ctx.clone(),
                settings,
                look,
                start,
                session,
                hwnd,
                panel,
            )))
        }),
    )
}

/// Where a browsing window opens: where it was left, or else centred on the monitor under the
/// pointer.
fn browse_rect(settings: &settings::Settings) -> Option<placement::Rect> {
    let (work, scale) = placement::pointer_monitor()?;
    Some(placement::place(work, scale, settings.window, &placement::work_areas()))
}

/// Where a picker opens: centred over the asking app's window, on its monitor, or else in the
/// middle of the monitor under the pointer.
fn pick_rect(owner: i64) -> Option<placement::Rect> {
    // Real pixels first: before this, the owner's rectangle comes back scaled to 96 dpi.
    placement::aware();
    let owner_rect = pick::host::owner_rect(owner);
    let (work, scale) = match owner_rect {
        Some(r) => placement::monitor_of(r)?,
        None => placement::pointer_monitor()?,
    };
    let size = (
        (placement::PICK_SIZE.0 * scale) as i32,
        (placement::PICK_SIZE.1 * scale) as i32,
    );
    Some(placement::centre_over(owner_rect, work, size))
}
