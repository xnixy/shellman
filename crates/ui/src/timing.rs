//! Opening-speed milestones that need the frame: when a frame with real content was painted and
//! when it reached the screen (see `shellman_core::timing`). Free unless `SHELLMAN_TIMING=1`.

use shellman_core::timing;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

static LOGGED: Mutex<Option<HashMap<&'static str, u64>>> = Mutex::new(None);

/// The window and its OpenGL context exist (the app's creator runs): logged with the driver
/// that renders it, which on a machine with two graphics adapters says which one draws.
pub fn gl_ready(gl: Option<&egui_glow::glow::Context>) {
    use egui_glow::glow::HasContext;
    timing::mark_with("window and GL context ready", || match gl {
        Some(gl) => unsafe {
            format!(
                "{} / {} / {}",
                gl.get_parameter_string(egui_glow::glow::VENDOR),
                gl.get_parameter_string(egui_glow::glow::RENDERER),
                gl.get_parameter_string(egui_glow::glow::VERSION)
            )
        },
        None => "no GL".into(),
    });
}

/// Call from a frame that shows `what` with its real content (a folder listing, a filled list).
/// Logged once per `generation` (a showing's count, or 0 for once per process): `<what> painted`
/// when the frame's GPU commands are issued, just before the buffers swap, and
/// `<what> presented` after the compositor has composed it.
pub fn content(ctx: &egui::Context, what: &'static str, generation: u64) {
    if !timing::enabled() {
        return;
    }
    {
        let Ok(mut logged) = LOGGED.lock() else { return };
        let map = logged.get_or_insert_with(HashMap::new);
        if map.get(what) == Some(&generation) {
            return;
        }
        map.insert(what, generation);
    }
    timing::mark_with("content frame built", || what.to_owned());
    // A paint callback runs in the middle of painting this very frame.
    let callback = egui_glow::CallbackFn::new(move |_info, _painter| {
        timing::mark_with("content painted", || what.to_owned());
        timing::mark_presented(what);
    });
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("shellman-timing"));
    ctx.layer_painter(layer).add(egui::PaintCallback {
        rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1.0, 1.0)),
        callback: Arc::new(callback),
    });
}
