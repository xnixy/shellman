//! The arrangement diagram: every monitor as a numbered rectangle at its place on the desktop,
//! scaled to fit. A click selects, a drag moves with snapping (`geom::place`); the page decides
//! what a finished drag means.

use egui::{Align2, CornerRadius, FontId, Pos2, Rect as ERect, Sense, Stroke, StrokeKind, Ui, Vec2};
use shellman_core::system::display::geom::{self, Rect};
use shellman_core::system::display::page::{Change, Monitor};
use shellman_ui::theme::{self, colors};

/// A drag in progress: where the monitor's corner wants to be, in desktop pixels.
pub struct Drag {
    pub index: usize,
    want: (f32, f32),
    /// Where it would land if let go now.
    pub landing: Option<(i32, i32)>,
}

#[derive(Default)]
pub struct Out {
    pub select: Option<usize>,
    /// A drag ended: the monitor and its new corner.
    pub moved: Option<(usize, (i32, i32))>,
}

const HEIGHT: f32 = 190.0;
const PAD: f32 = 14.0;
/// Snapping reach on screen, so it feels the same whatever the scale.
const SNAP_PX: f32 = 14.0;

pub fn show(ui: &mut Ui, plan: &[Change], monitors: &[Monitor], selected: usize, primary: &str, drag: &mut Option<Drag>) -> Out {
    let c = colors();
    let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), HEIGHT), Sense::hover());
    ui.painter().rect_filled(area, CornerRadius::same(10), c.control);
    let rects: Vec<Rect> = plan.iter().map(Change::rect).collect();
    let mut out = Out::default();
    if rects.is_empty() {
        return out;
    }
    let left = rects.iter().map(|r| r.x).min().unwrap_or(0);
    let top = rects.iter().map(|r| r.y).min().unwrap_or(0);
    let right = rects.iter().map(Rect::right).max().unwrap_or(1);
    let bottom = rects.iter().map(Rect::bottom).max().unwrap_or(1);
    let (tw, th) = ((right - left).max(1) as f32, (bottom - top).max(1) as f32);
    let inner = area.shrink(PAD);
    let scale = (inner.width() / tw).min(inner.height() / th) * 0.82;
    let origin = Pos2::new(inner.center().x - tw * scale / 2.0, inner.center().y - th * scale / 2.0);
    let to_screen = |x: i32, y: i32| Pos2::new(origin.x + (x - left) as f32 * scale, origin.y + (y - top) as f32 * scale);
    let snap = (SNAP_PX / scale).round() as i32;
    for (i, r) in rects.iter().enumerate() {
        let landing = drag.as_ref().filter(|d| d.index == i).and_then(|d| d.landing);
        let (x, y) = landing.unwrap_or((r.x, r.y));
        let rect = ERect::from_min_size(to_screen(x, y), Vec2::new(r.w as f32 * scale, r.h as f32 * scale));
        let resp = ui.interact(rect, ui.id().with(("monitor", i)), Sense::click_and_drag());
        if resp.clicked() || resp.drag_started() {
            out.select = Some(i);
        }
        if resp.drag_started() {
            *drag = Some(Drag { index: i, want: (r.x as f32, r.y as f32), landing: None });
        }
        if resp.dragged() {
            if let Some(d) = drag.as_mut().filter(|d| d.index == i) {
                let delta = resp.drag_delta();
                d.want = (d.want.0 + delta.x / scale, d.want.1 + delta.y / scale);
                d.landing = geom::place(&rects, i, (d.want.0.round() as i32, d.want.1.round() as i32), snap);
            }
        }
        if resp.drag_stopped() {
            if let Some(d) = drag.take() {
                out.moved = d.landing.map(|p| (i, p));
            }
        }
        paint(ui, rect, i == selected, plan[i].id == primary, &monitors[i], resp.hovered());
    }
    out
}

fn paint(ui: &mut Ui, rect: ERect, selected: bool, primary: bool, m: &Monitor, hovered: bool) {
    let c = colors();
    let fill = if selected {
        c.accent_fill
    } else if hovered {
        c.control_hover
    } else {
        theme::with_alpha(c.text, 22)
    };
    let (text, muted) = if selected { (c.on_accent, theme::with_alpha(c.on_accent, 190)) } else { (c.text, c.muted) };
    ui.painter().rect_filled(rect, CornerRadius::same(5), fill);
    ui.painter().rect_stroke(rect, CornerRadius::same(5), Stroke::new(1.0_f32, theme::with_alpha(c.text, 60)), StrokeKind::Inside);
    if primary {
        // The main display carries a bar along its top edge, like a taskbar's place.
        let bar = ERect::from_min_size(rect.min + Vec2::new(2.0, 2.0), Vec2::new(rect.width() - 4.0, 4.0));
        ui.painter().rect_filled(bar, CornerRadius::same(2), text);
    }
    let big = (rect.height() * 0.42).clamp(12.0, 34.0);
    ui.painter().text(rect.center() - Vec2::new(0.0, 4.0), Align2::CENTER_CENTER, m.number.to_string(), FontId::proportional(big), text);
    if rect.width() > 70.0 {
        ui.painter().text(
            Pos2::new(rect.center().x, rect.bottom() - 6.0),
            Align2::CENTER_BOTTOM,
            &m.name,
            FontId::proportional(10.5),
            muted,
        );
    }
}
