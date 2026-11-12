//! The name of a rail icon, as a small glass pill beside it. The window is transparent and only
//! its registered islands are glass, so egui's own tooltip (a loose frame floating over whatever
//! lies there, half over the rail and half over the page) would look like a stray smudge. This one
//! is an island of its own, placed to the right of the rail and kept inside the window.

use egui::{pos2, vec2, Align2, Context, Id, LayerId, Order, Rect, Ui, Vec2};
use shellman_ui::glass::islands::{self, Island};
use shellman_ui::glass::Style;
use shellman_ui::theme::{colors, semibold_font};
use std::time::Duration;

/// How long the pointer rests on an icon before its name shows, in seconds.
const DELAY: f64 = 0.35;
const HEIGHT: f32 = 32.0;
const PAD: f32 = 16.0;
/// Space between the icon (or its rail) and the pill.
const OFFSET: f32 = 8.0;

/// Where a pill of `size` goes: to the right of `anchor`, level with its middle, and kept inside
/// `bounds`.
pub fn place(anchor: Rect, size: Vec2, bounds: Rect) -> Rect {
    let x = (anchor.right() + OFFSET).min(bounds.right() - size.x).max(bounds.left());
    let y = (anchor.center().y - size.y / 2.0).clamp(bounds.top(), (bounds.bottom() - size.y).max(bounds.top()));
    Rect::from_min_size(pos2(x, y), size)
}

/// Whether the pointer has rested on the thing `key` names for long enough. Asks for a repaint
/// when it will have, so nothing polls while it waits.
pub fn ready(ctx: &Context, key: Id, hovered: bool) -> bool {
    if !hovered {
        ctx.data_mut(|d| d.remove::<f64>(key));
        return false;
    }
    let now = ctx.input(|i| i.time);
    let since = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(key, || now));
    let waited = now - since;
    if waited >= DELAY {
        return true;
    }
    ctx.request_repaint_after(Duration::from_secs_f64(DELAY - waited + 0.02));
    false
}

/// Show `text` as a pill beside `anchor`. Its island is added to `list` (so the window makes it
/// glass); `solid` is the style to paint it with when the window has no composed glass.
pub fn show(ui: &mut Ui, anchor: Rect, text: &str, bounds: Rect, list: &mut Vec<Island>, solid: Option<&Style>) {
    let c = colors();
    let font = semibold_font(13.0);
    let galley = ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, c.text));
    let rect = place(anchor, vec2(galley.size().x + 2.0 * PAD, HEIGHT), bounds);
    let island = Island::pill(rect);
    list.push(island);
    // Above everything, so neither the page nor the rail covers it.
    let painter = ui.ctx().layer_painter(LayerId::new(Order::Tooltip, Id::new("rail-tip")));
    match solid {
        Some(style) => {
            for shape in islands::solid_shapes(&[island], style) {
                painter.add(shape);
            }
        }
        // Over glass the pill may stand partly on the page's island, where a second pane of
        // glass would not show against the first; a backing of its own does.
        None => {
            let radius = (rect.height() / 2.0).round() as u8;
            painter.rect(rect, radius, c.popup, egui::Stroke::new(1.0_f32, c.hairline), egui::StrokeKind::Inside);
        }
    }
    painter.text(rect.center(), Align2::CENTER_CENTER, text, semibold_font(13.0), c.text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pill_sits_right_of_the_icon_level_with_it() {
        let bounds = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let icon = Rect::from_min_size(pos2(22.0, 100.0), vec2(34.0, 34.0));
        let r = place(icon, vec2(90.0, 32.0), bounds);
        assert_eq!(r.left(), icon.right() + OFFSET);
        assert_eq!(r.center().y, icon.center().y);
    }

    #[test]
    fn the_pill_is_never_clipped_by_the_window() {
        let bounds = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 300.0));
        let low = Rect::from_min_size(pos2(22.0, 290.0), vec2(34.0, 34.0));
        let r = place(low, vec2(90.0, 32.0), bounds);
        assert!(bounds.contains_rect(r));
        let far = Rect::from_min_size(pos2(180.0, 10.0), vec2(34.0, 34.0));
        assert!(bounds.contains_rect(place(far, vec2(90.0, 32.0), bounds)));
    }
}
