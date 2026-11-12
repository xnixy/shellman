//! A test aid: with `SHELLMAN_STRESS_GLYPHS=1`, every bar window lays out a few new glyphs at
//! random sizes each frame and asks for the next frame, so the font atlases of windows on
//! monitors of different scales grow fast and at different times. Nothing is drawn.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("SHELLMAN_STRESS_GLYPHS").is_some_and(|v| v == "1"))
}

fn next() -> u64 {
    static SEED: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);
    let mut x = SEED.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    SEED.store(x, Ordering::Relaxed);
    x
}

pub fn glyphs(ctx: &egui::Context) {
    if !on() {
        return;
    }
    let ranges: [(u32, u32); 4] = [(0x21, 0x24F), (0x370, 0x3FF), (0x400, 0x4FF), (0xE700, 0xF000)];
    for _ in 0..16 {
        let (lo, hi) = ranges[(next() % ranges.len() as u64) as usize];
        let c = char::from_u32(lo + (next() % u64::from(hi - lo)) as u32).unwrap_or('x');
        let size = 6.0 + (next() % 220) as f32 * 0.25;
        let font = if lo >= 0xE700 {
            shellman_ui::theme::icon_font(size)
        } else if next() % 2 == 0 {
            shellman_ui::theme::semibold_font(size)
        } else {
            egui::FontId::proportional(size)
        };
        ctx.fonts(|f| f.layout_no_wrap(c.to_string(), font, egui::Color32::WHITE));
    }
    // Not every frame: windows repainting without pause starve the host's timers.
    ctx.request_repaint_after(std::time::Duration::from_millis(25));
}
