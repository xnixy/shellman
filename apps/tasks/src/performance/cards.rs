//! The boxes of the Performance page, one function each.

use super::{spoken, stats, title, STATS_W};
use crate::app::State;
use crate::graph;
use egui::{pos2, vec2, Align2, FontId, Rect};
use shellman_core::procs::format;
use shellman_core::procs::history::Ring;
use shellman_ui::theme::{self, colors};
use std::sync::OnceLock;

const GRAPH_H: f32 = 150.0;
const HEAD: f32 = 34.0;
/// A core's small graph.
const CORE_W: f32 = 96.0;
const CORE_H: f32 = 46.0;

type Boxed<'a> = dyn FnMut(&mut egui::Ui, f32, &str, &mut dyn FnMut(&mut egui::Ui, Rect)) + 'a;

pub fn all(ui: &mut egui::Ui, st: &State, boxed: &mut Boxed) {
    cpu(ui, st, boxed);
    memory(ui, st, boxed);
    for g in &st.system.gpus {
        gpu(ui, st, g, boxed);
    }
    for d in &st.system.disks {
        disk(ui, st, d, boxed);
    }
    for n in &st.system.nets {
        net(ui, st, n, boxed);
    }
}

/// The graph's rect and the numbers' rect, side by side under the title; the numbers go under
/// the graph when the box is narrow.
fn split(rect: Rect) -> (Rect, Rect) {
    let body = Rect::from_min_max(pos2(rect.left(), rect.top() + HEAD), rect.max);
    if body.width() > STATS_W * 2.2 {
        let graph = Rect::from_min_size(body.min, vec2(body.width() - STATS_W - 24.0, GRAPH_H));
        let stats = Rect::from_min_max(pos2(graph.right() + 24.0, body.top()), body.max);
        (graph, stats)
    } else {
        let graph = Rect::from_min_size(body.min, vec2(body.width(), GRAPH_H));
        (graph, Rect::from_min_max(pos2(body.left(), graph.bottom() + 14.0), body.max))
    }
}

/// How tall a box with `rows` rows of numbers is, for its width.
fn height(ui: &egui::Ui, rows: usize, extra: f32) -> f32 {
    let wide = ui.available_width() - 40.0 > STATS_W * 2.2;
    let numbers = rows as f32 * 44.0;
    let body = if wide { GRAPH_H.max(numbers) } else { GRAPH_H + 14.0 + numbers };
    40.0 + HEAD + body + extra
}

fn cpu(ui: &mut egui::Ui, st: &State, boxed: &mut Boxed) {
    static NAME: OnceLock<String> = OnceLock::new();
    let name = NAME.get_or_init(|| shellman_core::procs::cpu_name().unwrap_or_default());
    let s = &st.system;
    let cores = s.cores.len().max(1);
    let per_row = ((ui.available_width() - 40.0 + 8.0) / (CORE_W + 8.0)).floor().max(1.0) as usize;
    let core_rows = cores.div_ceil(per_row);
    let extra = 14.0 + 20.0 + core_rows as f32 * (CORE_H + 8.0);
    let pairs = [
        ("Utilisation", format::percent(s.cpu)),
        ("Speed", s.mhz.map(format::mhz).unwrap_or_else(|| "—".into())),
        ("Processes", format::thousands(s.processes as u64)),
        ("Threads", format::thousands(s.threads as u64)),
        ("Handles", format::thousands(s.handles as u64)),
        ("Up time", format::duration(s.uptime_secs)),
    ];
    let spoken_name = format!("{}. {cores} logical processors", spoken("CPU", name, &pairs));
    boxed(ui, height(ui, 3, extra), &spoken_name, &mut |ui, rect| {
        let p = ui.painter().with_clip_rect(rect.expand(4.0));
        title(&p, rect, "CPU", name);
        let (g, numbers) = split(Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() - extra)));
        graph::draw(&p, g, &st.histories.cpu, 100.0, colors().accent_fill, true);
        stats(&p, numbers, &pairs);
        // Each logical processor, small.
        let top = rect.bottom() - extra + 14.0;
        let c = colors();
        p.text(pos2(rect.left(), top), Align2::LEFT_TOP, format!("{cores} logical processors"), FontId::proportional(12.0), c.muted);
        // The small graphs share the row's width out between them.
        let w = (rect.width() + 8.0) / per_row as f32 - 8.0;
        for (i, ring) in st.histories.cores.iter().enumerate() {
            let (row, col) = (i / per_row, i % per_row);
            let r = Rect::from_min_size(pos2(rect.left() + col as f32 * (w + 8.0), top + 20.0 + row as f32 * (CORE_H + 8.0)), vec2(w, CORE_H));
            graph::draw(&p, r, ring, 100.0, c.accent_fill, false);
        }
    });
}

fn memory(ui: &mut egui::Ui, st: &State, boxed: &mut Boxed) {
    let m = st.system.memory;
    let about = format!("{} installed", format::bytes(m.total));
    let pairs = [
        ("In use", format::bytes(m.used())),
        ("Available", format::bytes(m.available)),
        ("Committed", format!("{} / {}", format::bytes(m.committed), format::bytes(m.commit_limit))),
        ("Cached", format::bytes(m.cached)),
        ("In use", if m.total > 0 { format!("{:.0}%", m.used() as f64 / m.total as f64 * 100.0) } else { "—".into() }),
    ];
    boxed(ui, height(ui, 3, 0.0), &spoken("Memory", &about, &pairs), &mut |ui, rect| {
        let p = ui.painter().with_clip_rect(rect.expand(4.0));
        title(&p, rect, "Memory", &about);
        let (g, numbers) = split(rect);
        graph::draw(&p, g, &st.histories.memory, 100.0, colors().accent_fill, true);
        stats(&p, numbers, &pairs);
    });
}

fn gpu(ui: &mut egui::Ui, st: &State, g: &shellman_core::procs::Gpu, boxed: &mut Boxed) {
    let empty = Ring::default();
    let rings = st.histories.gpus.get(&g.name);
    let of = |used: u64, total: u64| if total > 0 { format!("{} / {}", format::bytes(used), format::bytes(total)) } else { format::bytes(used) };
    let pairs = [
        ("Utilisation", format::percent(g.utilisation)),
        ("Temperature", "Not available".into()),
        ("Dedicated memory", of(g.dedicated_used, g.dedicated_total)),
        ("Shared memory", of(g.shared_used, g.shared_total)),
    ];
    boxed(ui, height(ui, 2, 0.0), &spoken("GPU", &g.name, &pairs), &mut |ui, rect| {
        let p = ui.painter().with_clip_rect(rect.expand(4.0));
        title(&p, rect, "GPU", &g.name);
        let (graph_rect, numbers) = split(rect);
        graph::draw(&p, graph_rect, rings.map_or(&empty, |r| &r[0]), 100.0, colors().accent_fill, true);
        stats(&p, numbers, &pairs);
    });
}

/// "0 C: D:" as "Disk 0 (C: D:)".
fn disk_title(name: &str) -> String {
    match name.split_once(' ') {
        Some((n, letters)) if !letters.trim().is_empty() => format!("Disk {n} ({})", letters.trim()),
        _ => format!("Disk {}", name.trim()),
    }
}

fn disk(ui: &mut egui::Ui, st: &State, d: &shellman_core::procs::Disk, boxed: &mut Boxed) {
    let empty = Ring::default();
    let rings = st.histories.disks.get(&d.name);
    let name = disk_title(&d.name);
    let pairs = [
        ("Active time", format::percent(d.active)),
        ("", String::new()),
        ("Read", format::rate_any(d.read)),
        ("Write", format::rate_any(d.write)),
    ];
    boxed(ui, height(ui, 2, 0.0), &spoken(&name, "Active time", &pairs), &mut |ui, rect| {
        let p = ui.painter().with_clip_rect(rect.expand(4.0));
        title(&p, rect, &name, "Active time");
        let (g, numbers) = split(rect);
        graph::draw(&p, g, rings.map_or(&empty, |r| &r[0]), 100.0, colors().accent_fill, true);
        stats(&p, numbers, &pairs);
    });
}

fn net(ui: &mut egui::Ui, st: &State, n: &shellman_core::procs::Net, boxed: &mut Boxed) {
    let empty = Ring::default();
    let rings = st.histories.nets.get(&n.name);
    let pairs = [
        ("Receive", format::bits(n.receive)),
        ("Send (dashed)", format::bits(n.send)),
        ("IPv4 address", if n.address.is_empty() { "—".into() } else { n.address.clone() }),
    ];
    boxed(ui, height(ui, 2, 0.0), &spoken(&n.name, &n.description, &pairs), &mut |ui, rect| {
        let p = ui.painter().with_clip_rect(rect.expand(4.0));
        title(&p, rect, &n.name, &n.description);
        let (g, numbers) = split(rect);
        let (send, receive) = rings.map_or((&empty, &empty), |r| (&r[0], &r[1]));
        let max = graph::scale(send, 125_000.0).max(graph::scale(receive, 125_000.0));
        let c = colors();
        graph::draw(&p, g, receive, max, c.accent_fill, true);
        // Sent on top of received, as a second, quieter line.
        let pts = graph::points(send, g.shrink2(vec2(1.0, 3.0)), max);
        if pts.len() > 1 {
            p.add(egui::Shape::dashed_line(&pts, egui::Stroke::new(1.5_f32, theme::with_alpha(c.text, 170)), 5.0, 3.0));
        }
        p.text(pos2(g.right() - 10.0, g.top() + 8.0), Align2::RIGHT_TOP, format!("{} scale", format::bits(max as f64)), FontId::proportional(11.0), c.muted);
        stats(&p, numbers, &pairs);
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn disks_are_titled_like_task_manager() {
        assert_eq!(super::disk_title("0 C: D:"), "Disk 0 (C: D:)");
        assert_eq!(super::disk_title("1"), "Disk 1");
    }
}
