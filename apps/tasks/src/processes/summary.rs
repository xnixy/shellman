//! The summary island: CPU, memory, GPU, disk and network, each a small tile with the last minute
//! as a line. A click on a tile opens the Performance page.

use crate::app::{Page, State};
use crate::graph;
use egui::{pos2, vec2, Align2, FontId, Rect, Sense};
use shellman_core::procs::format;
use shellman_core::procs::history::Ring;
use shellman_ui::chips;
use shellman_ui::theme::{colors, semibold_font};

struct Tile<'a> {
    label: &'static str,
    value: String,
    detail: String,
    ring: &'a Ring,
    max: f32,
}

pub fn show(ui: &mut egui::Ui, rect: Rect, st: &mut State) {
    let s = &st.system;
    let h = &st.histories;
    let m = s.memory;
    let tiles = [
        Tile { label: "CPU", value: format::percent(s.cpu), detail: s.mhz.map(format::mhz).unwrap_or_default(), ring: &h.cpu, max: 100.0 },
        Tile {
            label: "Memory",
            value: format!("{} / {}", format::bytes(m.used()), format::bytes(m.total)),
            detail: if m.total > 0 { format!("{:.0}% in use", m.used() as f64 / m.total as f64 * 100.0) } else { String::new() },
            ring: &h.memory,
            max: 100.0,
        },
        Tile {
            label: "GPU",
            value: s.gpu().map(format::percent).unwrap_or_else(|| "—".into()),
            detail: s.gpus.iter().max_by(|a, b| a.utilisation.total_cmp(&b.utilisation)).map(|g| short_gpu(&g.name)).unwrap_or_default(),
            ring: &h.gpu,
            max: 100.0,
        },
        Tile {
            label: "Disk",
            value: format::percent(s.disk_active()),
            detail: format::rate_any(s.disks.iter().map(|d| d.read + d.write).sum()),
            ring: &h.disk,
            max: 100.0,
        },
        Tile { label: "Network", value: format::bits(s.net_rate()), detail: String::new(), ring: &h.net, max: graph::scale(&h.net, 125_000.0) },
    ];
    let c = colors();
    let pad = 14.0;
    let gap = 10.0;
    let inner = rect.shrink(pad);
    let w = (inner.width() - gap * (tiles.len() - 1) as f32) / tiles.len() as f32;
    let (_, hover, _) = chips::soft_colors(c.light);
    for (i, t) in tiles.iter().enumerate() {
        let tile = Rect::from_min_size(pos2(inner.left() + i as f32 * (w + gap), inner.top()), vec2(w, inner.height()));
        let resp = ui.interact(tile, egui::Id::new(("tile", t.label)), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(tile.expand(4.0), 14.0, hover);
        }
        let p = ui.painter().with_clip_rect(tile);
        p.text(pos2(tile.left() + 4.0, tile.top() + 2.0), Align2::LEFT_TOP, t.label, FontId::proportional(12.0), c.muted);
        p.text(pos2(tile.right() - 4.0, tile.top() + 2.0), Align2::RIGHT_TOP, &t.detail, FontId::proportional(11.5), c.muted);
        p.text(pos2(tile.left() + 4.0, tile.top() + 18.0), Align2::LEFT_TOP, &t.value, semibold_font(16.0), c.text);
        let spark = Rect::from_min_max(pos2(tile.left(), tile.bottom() - 30.0), tile.max);
        graph::draw(&p, spark, t.ring, t.max, c.accent_fill, false);
        if resp.on_hover_text("Open in Performance").clicked() {
            st.go_to = Some(Page::Performance);
        }
    }
}

/// A GPU's name without its maker's prefix, for a small tile: "GeForce RTX 5090".
fn short_gpu(name: &str) -> String {
    let n = name.trim();
    for prefix in ["NVIDIA ", "AMD ", "Intel(R) ", "Intel "] {
        if let Some(rest) = n.strip_prefix(prefix) {
            return rest.to_owned();
        }
    }
    n.to_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn gpu_names_lose_the_maker() {
        assert_eq!(super::short_gpu("NVIDIA GeForce RTX 5090"), "GeForce RTX 5090");
        assert_eq!(super::short_gpu("AMD Radeon(TM) Graphics"), "Radeon(TM) Graphics");
        assert_eq!(super::short_gpu("Something"), "Something");
    }
}
