//! Hover labels for islands windows: a small glass pill beside an icon, naming it, shown after
//! the pointer rests on it a moment.
//!
//! egui's own tooltip opens under the pointer, on top of whatever is there (on a rail, the next
//! icon), and is a plain popup rather than glass. A hover label sits beside the icon instead (to
//! the right of a rail on the left, or to the left when there is no room), on a nearly solid fill
//! so it reads over anything, and it asks to be glass too: call [`islands`] once a frame, after
//! everything is drawn, and hand what it returns to the panel with the other islands.
//!
//! ```ignore
//! let resp = ui.interact(icon_rect, id, Sense::click());
//! shellman_ui::hover::label(ui, &resp, "Downloads");
//! // ...later in the frame, with the other islands:
//! list.extend(shellman_ui::hover::islands(ctx));
//! panel.set_islands(&list, ctx.pixels_per_point());
//! ```
//!
//! It costs nothing while nothing is hovered: the delay is one `request_repaint_after`.

use crate::glass::islands::{shows_as, Island};
use crate::theme::{self, colors};
use egui::{Color32, CornerRadius, FontId, Id, LayerId, Order, Rect, Stroke, Vec2};

/// How long the pointer rests on an icon before its label shows, in seconds.
pub const DELAY: f64 = 0.35;
/// From the icon to the label.
const GAP: f32 = 8.0;
const PAD_X: f32 = 12.0;
const PAD_Y: f32 = 6.0;
const FONT: f32 = 13.0;

/// Where a label of `size` goes for an icon at `anchor` inside `screen`: to its right, centred
/// on it; to its left when the right has no room; and always inside the screen.
pub fn place(anchor: Rect, size: Vec2, screen: Rect) -> Rect {
    let right = anchor.right() + GAP;
    let x = if right + size.x <= screen.right() - 2.0 {
        right
    } else {
        anchor.left() - GAP - size.x
    };
    let x = x.clamp(screen.left() + 2.0, (screen.right() - 2.0 - size.x).max(screen.left() + 2.0));
    let y = (anchor.center().y - size.y / 2.0)
        .clamp(screen.top() + 2.0, (screen.bottom() - 2.0 - size.y).max(screen.top() + 2.0));
    Rect::from_min_size(egui::pos2(x, y), size)
}

/// Whether a label that began hovering at `since` shows at `now`, and if not, how long until
/// it does.
pub fn due(since: f64, now: f64) -> Result<(), f64> {
    let left = DELAY - (now - since);
    if left <= 1e-9 {
        Ok(())
    } else {
        Err(left)
    }
}

/// The label of `resp`'s icon: `text` (one line, or a name and a muted second line after a
/// `\n`) beside it, once the pointer has rested on it. Returns where it is, when it shows.
pub fn label(ui: &egui::Ui, resp: &egui::Response, text: &str) -> Option<Rect> {
    label_at(ui.ctx(), resp.id, resp.rect, resp.hovered(), text)
}

/// The same label for something that is not a widget (a link in a terminal's grid): `id` names
/// it, `anchor` is where it is, `hovered` whether the pointer rests on it now. Call it with
/// `hovered` false once the pointer leaves, so the delay starts over next time.
pub fn label_at(ctx: &egui::Context, id: Id, anchor: Rect, hovered: bool, text: &str) -> Option<Rect> {
    let key = Id::new("shellman-hover-label");
    if !hovered || ctx.dragged_id().is_some() || text.is_empty() {
        // Forget the start only when it was this icon's.
        let mine = ctx.data(|d| d.get_temp::<(Id, f64)>(key)).is_some_and(|(i, _)| i == id);
        if mine {
            ctx.data_mut(|d| d.remove::<(Id, f64)>(key));
        }
        return None;
    }
    let now = ctx.input(|i| i.time);
    let since = match ctx.data(|d| d.get_temp::<(Id, f64)>(key)) {
        Some((i, t)) if i == id => t,
        _ => {
            ctx.data_mut(|d| d.insert_temp(key, (id, now)));
            now
        }
    };
    if let Err(left) = due(since, now) {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(left + 0.01));
        return None;
    }
    let c = colors();
    let (first, second) = text.split_once('\n').unwrap_or((text, ""));
    let g1 = ctx.fonts(|f| f.layout_no_wrap(first.to_owned(), FontId::proportional(FONT), c.text));
    let g2 = (!second.is_empty())
        .then(|| ctx.fonts(|f| f.layout_no_wrap(second.to_owned(), FontId::proportional(FONT - 1.5), c.muted)));
    let w = g1.size().x.max(g2.as_ref().map_or(0.0, |g| g.size().x));
    let h = g1.size().y + g2.as_ref().map_or(0.0, |g| g.size().y + 1.0);
    let size = Vec2::new(w + PAD_X * 2.0, (h + PAD_Y * 2.0).max(28.0));
    let rect = place(anchor, size, ctx.screen_rect());
    let p = ctx.layer_painter(LayerId::new(Order::Tooltip, key));
    let radius = CornerRadius::same(if g2.is_some() { 14 } else { (size.y / 2.0) as u8 });
    p.rect(rect, radius, fill(c.light), Stroke::new(1.0_f32, c.hairline), egui::StrokeKind::Inside);
    let top = rect.center().y - h / 2.0;
    p.galley(egui::pos2(rect.left() + PAD_X, top), g1.clone(), c.text);
    if let Some(g2) = g2 {
        p.galley(egui::pos2(rect.left() + PAD_X, top + g1.size().y + 1.0), g2, c.muted);
    }
    ctx.data_mut(|d| {
        let mut list: Vec<Rect> = d.get_temp(Id::new("shellman-hover-islands")).unwrap_or_default();
        list.push(rect);
        d.insert_temp(Id::new("shellman-hover-islands"), list);
    });
    Some(rect)
}

/// A solid fill in the glass's own tint, so the label reads over cards and wallpaper
/// alike, and still looks like the glass around it.
fn fill(light: bool) -> Color32 {
    let base = if light { Color32::from_rgb(246, 247, 250) } else { Color32::from_rgb(30, 33, 42) };
    shows_as(theme::with_alpha(base, 255))
}

/// The labels shown this frame, as islands for the glass panel; call once a frame, after the
/// last [`label`]. Empty while none shows.
pub fn islands(ctx: &egui::Context) -> Vec<Island> {
    let rects: Vec<Rect> = ctx
        .data_mut(|d| d.remove_temp(Id::new("shellman-hover-islands")))
        .unwrap_or_default();
    rects
        .into_iter()
        .map(|r| Island::new(r, (r.height() / 2.0).min(14.0)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    fn screen() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), Vec2::new(800.0, 600.0))
    }

    #[test]
    fn a_label_sits_right_of_its_icon_centred_on_it() {
        let icon = Rect::from_min_size(pos2(12.0, 100.0), Vec2::splat(40.0));
        let r = place(icon, Vec2::new(90.0, 28.0), screen());
        assert_eq!(r.left(), icon.right() + GAP);
        assert_eq!(r.center().y, icon.center().y);
    }

    #[test]
    fn it_goes_left_when_the_right_is_full_and_never_leaves_the_screen() {
        let icon = Rect::from_min_size(pos2(760.0, 100.0), Vec2::splat(30.0));
        let r = place(icon, Vec2::new(90.0, 28.0), screen());
        assert_eq!(r.right(), icon.left() - GAP);
        // An icon at the very bottom: the label is pushed up into the screen.
        let low = Rect::from_min_size(pos2(12.0, 590.0), Vec2::splat(30.0));
        let r = place(low, Vec2::new(90.0, 28.0), screen());
        assert!(screen().contains_rect(r));
        // Wider than the room either side: still on screen.
        let r = place(icon, Vec2::new(2000.0, 28.0), screen());
        assert!(r.left() >= 0.0);
    }

    #[test]
    fn it_waits_before_showing() {
        assert!(due(10.0, 10.1).is_err());
        assert!((due(10.0, 10.1).unwrap_err() - (DELAY - 0.1)).abs() < 1e-9);
        assert!(due(10.0, 10.0 + DELAY).is_ok());
    }
}
