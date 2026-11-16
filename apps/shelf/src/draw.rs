//! Drawing the shelf and an open stack's fan on their glass, and what the pointer did there.
//! The glass itself (blur, rim, shadow) is the window's (`glass::GlassPanel`); this paints the
//! content: the drop zone, each stack's cards, name and count, and the fan's pills.

use crate::cards::{self, Fill};
use crate::geometry::{self, Layout};
use crate::stacks::Item;
use crate::text;
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui};
use shellman_core::settings::desktop::Stack;
use shellman_core::thumbs::{self, Thumbs};
use shellman_ui::glass::islands::shows_as;
use shellman_ui::theme::{self, colors, glyph, icon_font, semibold_font};
use shellman_ui::widgets::draw_text;
use std::path::PathBuf;

/// One stack as drawn.
pub struct StackView<'a> {
    pub kind: Stack,
    pub items: &'a [Item],
}

/// An open (or opening, or closing) fan.
pub struct FanView<'a> {
    pub kind: Stack,
    pub items: &'a [Item],
    pub more: usize,
    /// The pills, items first and "Open in Files" last.
    pub pills: Vec<Rect>,
    /// How far open, 0 to 1.
    pub t: f32,
}

pub struct Scene<'a> {
    pub layout: &'a Layout,
    pub stacks: Vec<StackView<'a>>,
    pub fan: Option<FanView<'a>>,
    /// A drag the shelf would take hovers over it.
    pub keep: bool,
    /// Now, in local seconds since 1970, and the local offset from UTC in seconds.
    pub now: i64,
    pub offset: i64,
}

/// What the pointer asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Toggle(Stack),
    Open(PathBuf),
    OpenFolder(Stack),
    DragOut(PathBuf),
}

/// The text colour, faint, as hover and the open stack's backing.
fn soft(alpha_light: u8, alpha_dark: u8) -> Color32 {
    let c = colors();
    if c.high_contrast {
        // No washes: hovered and open things are outlined instead (`theme::contrast_outline`).
        return Color32::TRANSPARENT;
    }
    shows_as(theme::with_alpha(c.text, if c.light { alpha_light } else { alpha_dark }))
}

fn hover_fill() -> Color32 {
    soft(15, 23)
}

/// The paper a document's icon sits on.
fn paper() -> Color32 {
    if colors().light {
        Color32::from_rgb(248, 249, 252)
    } else {
        Color32::from_rgb(226, 230, 238)
    }
}

/// Whether an item's picture is a photo-like preview (fills its card) rather than an icon (sits
/// on paper).
fn is_preview(item: &Item) -> bool {
    !item.is_dir() && thumbs::wants_thumb(&item.name) && !thumbs::is_program(&item.name)
}

pub fn paint(ui: &mut Ui, scene: &Scene, thumbs: &mut Thumbs) -> Vec<Event> {
    let mut events = Vec::new();
    drop_zone(ui, scene.layout.drop, scene.keep);
    for (view, rect) in scene.stacks.iter().zip(&scene.layout.stacks) {
        let active = scene.fan.as_ref().is_some_and(|f| f.kind == view.kind && f.t > 0.5);
        let resp = ui.interact(*rect, ui.id().with(("stack", view.kind.label())), Sense::click());
        // A button that opens the stack's fan: "selected" while it is open.
        theme::speak_selected(&resp, egui::WidgetType::Button, active, || text::spoken(&[view.kind.label(), &stack_line(view, scene)]));
        let back = rect.shrink2(vec2(0.0, 2.0));
        theme::focus_ring(ui, &resp, back, geometry::STACK_RADIUS);
        if resp.hovered() || active {
            ui.painter().rect_filled(back, CornerRadius::same(geometry::STACK_RADIUS as u8), hover_fill());
        }
        theme::contrast_outline(ui, back, geometry::STACK_RADIUS, resp.hovered(), active);
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        stack(ui, *rect, view, scene, thumbs);
        if resp.clicked() {
            events.push(Event::Toggle(view.kind));
        }
    }
    if let Some(fan) = &scene.fan {
        fan_pills(ui, fan, scene, thumbs, &mut events);
    }
    events
}

/// The dashed well at the top: "Drop", or "Keep" while a drag hovers.
fn drop_zone(ui: &Ui, rect: Rect, keep: bool) {
    let c = colors();
    let p = ui.painter();
    let line = if keep { c.text } else { theme::with_alpha(c.muted, 190) };
    if keep {
        p.rect_filled(rect, CornerRadius::same(geometry::DROP_RADIUS as u8), hover_fill());
    }
    let mut outline: Vec<Pos2> =
        cards::outline(rect.size() - vec2(1.5, 1.5), geometry::DROP_RADIUS).iter().map(|v| rect.center() + *v).collect();
    if let Some(first) = outline.first().copied() {
        outline.push(first);
    }
    p.extend(Shape::dashed_line(&outline, Stroke::new(1.5_f32, line), 5.0, 4.0));
    let top = rect.center().y - 22.0;
    let icon = if keep { c.text } else { c.muted };
    p.text(pos2(rect.center().x, top + 11.0), Align2::CENTER_CENTER, glyph::DOWNLOADS, icon_font(20.0), icon);
    let (word, font, col) = if keep {
        ("Keep", semibold_font(12.0), c.text)
    } else {
        ("Drop", FontId::proportional(12.0), c.muted)
    };
    p.text(pos2(rect.center().x, top + 35.0), Align2::CENTER_CENTER, word, font, col);
    let resp = ui.interact(rect, egui::Id::new("shelf drop zone"), Sense::hover());
    theme::speak(&resp, egui::WidgetType::Label, || if keep { "Drop here to keep it on the shelf".to_owned() } else { "Drop files here".to_owned() });
}

/// A stack: three overlapped cards of its newest items, its name and its count.
fn stack(ui: &Ui, rect: Rect, view: &StackView, scene: &Scene, thumbs: &mut Thumbs) {
    let c = colors();
    let p = ui.painter();
    let box_center = pos2(rect.center().x, rect.top() + 12.0 + geometry::CARDS / 2.0);
    // Back to front: the third newest tilted left, the second tilted right, the newest upright.
    let places = [
        (2, vec2(-9.0, -4.0), -13.0_f32, 42.0, 11.0, 0.8),
        (1, vec2(9.0, -5.0), 11.0, 42.0, 11.0, 0.92),
        (0, vec2(0.0, 0.0), 0.0, 48.0, 12.0, 1.0),
    ];
    if view.items.is_empty() {
        let r = Rect::from_center_size(box_center, vec2(44.0, 44.0));
        p.rect_stroke(r, CornerRadius::same(12), Stroke::new(1.0_f32, theme::with_alpha(c.muted, 120)), egui::StrokeKind::Inside);
    }
    for (i, offset, degrees, side, radius, opacity) in places {
        let Some(item) = view.items.get(i) else { continue };
        let center = box_center + offset;
        card(p, item, center, side, radius, degrees.to_radians(), opacity, thumbs);
    }
    let label_y = rect.top() + 12.0 + geometry::CARDS + 9.0 + 9.0;
    p.text(pos2(rect.center().x, label_y), Align2::CENTER_CENTER, view.kind.label(), semibold_font(12.0), c.text);
    let line = stack_line(view, scene);
    p.text(pos2(rect.center().x, label_y + 17.0), Align2::CENTER_CENTER, line, FontId::proportional(11.0), c.muted);
}

/// The muted line under a stack's name: when the newest came (Recent) or how many it holds.
fn stack_line(view: &StackView, scene: &Scene) -> String {
    match view.kind {
        Stack::Recent => view.items.first().map_or_else(|| "Nothing yet".to_owned(), |i| text::day(i.time + scene.offset, scene.now)),
        _ => text::count(view.items.len()),
    }
}

/// One card: the item's preview filling it, or its icon on paper.
#[allow(clippy::too_many_arguments)]
fn card(p: &egui::Painter, item: &Item, center: Pos2, side: f32, radius: f32, angle: f32, opacity: f32, thumbs: &mut Thumbs) {
    let rim = Color32::from_white_alpha(60);
    let size = vec2(side, side);
    let texture = thumbs.get(&item.path, true).map(|t| (t.id(), t.size_vec2()));
    match texture {
        Some((id, tex)) if is_preview(item) => {
            cards::card(p, center, size, radius, angle, Fill::Picture(id, cards::cover_uv(tex)), rim, opacity);
        }
        _ => {
            cards::card(p, center, size, radius, angle, Fill::Color(paper()), rim, opacity);
            if let Some((id, tex)) = texture {
                let fit = side * 0.62 / tex.x.max(tex.y).max(1.0);
                cards::picture(p, center, tex * fit, angle, id, opacity);
            }
        }
    }
}

/// The open stack's pills.
fn fan_pills(ui: &Ui, fan: &FanView, scene: &Scene, thumbs: &mut Thumbs, events: &mut Vec<Event>) {
    let c = colors();
    let fade = |col: Color32| col.gamma_multiply(fan.t.clamp(0.0, 1.0));
    let p = ui.painter();
    let n = fan.items.len();
    for (i, rect) in fan.pills.iter().enumerate() {
        let resp = ui.interact(*rect, ui.id().with(("pill", fan.kind.label(), i)), Sense::click_and_drag());
        theme::speak(&resp, egui::WidgetType::Button, || match fan.items.get(i) {
            Some(item) => text::spoken(&[&item.name, &text::item_line(item.size, item.time + scene.offset, scene.now)]),
            None => {
                let (open, more) = text::open_in_files(fan.kind.label(), fan.more);
                text::spoken(&[&open, more.as_deref().unwrap_or("")])
            }
        });
        theme::focus_ring(ui, &resp, *rect, rect.height() / 2.0);
        if resp.hovered() && fan.t >= 1.0 {
            p.rect_filled(*rect, CornerRadius::same((rect.height() / 2.0) as u8), soft(12, 18));
            theme::contrast_outline(ui, *rect, rect.height() / 2.0, true, false);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if let Some(item) = fan.items.get(i) {
            let d = 44.0;
            let center = pos2(rect.left() + 9.0 + d / 2.0, rect.center().y);
            let texture = thumbs.get(&item.path, true).map(|t| (t.id(), t.size_vec2()));
            match texture {
                Some((id, tex)) if is_preview(item) => {
                    cards::card(p, center, vec2(d, d), d / 2.0, 0.0, Fill::Picture(id, cards::cover_uv(tex)), Color32::from_white_alpha(40), fan.t);
                }
                other => {
                    cards::tile(p, center, vec2(d, d), d / 2.0, 0.0, fade(soft(16, 26)));
                    if let Some((id, tex)) = other {
                        let fit = 26.0 / tex.x.max(tex.y).max(1.0);
                        cards::picture(p, center, tex * fit, 0.0, id, fan.t);
                    }
                }
            }
            let x = rect.left() + 9.0 + d + 14.0;
            let room = rect.right() - 22.0 - x;
            let name = Rect::from_min_size(pos2(x, rect.center().y - 19.0), vec2(room, 20.0));
            draw_text(ui, name, &item.name, semibold_font(14.0), fade(c.text), false);
            let line = text::item_line(item.size, item.time + scene.offset, scene.now);
            let sub = Rect::from_min_size(pos2(x, rect.center().y + 1.0), vec2(room, 18.0));
            draw_text(ui, sub, &line, FontId::proportional(12.0), fade(c.muted), false);
            if fan.t >= 1.0 {
                if resp.drag_started() {
                    events.push(Event::DragOut(item.path.clone()));
                } else if resp.clicked() {
                    events.push(Event::Open(item.path.clone()));
                }
            }
        } else if i == n {
            let (open, more) = text::open_in_files(fan.kind.label(), fan.more);
            let icon_at = pos2(rect.left() + 20.0 + 9.0, rect.center().y);
            p.text(icon_at, Align2::CENTER_CENTER, glyph::FOLDER, icon_font(17.0), fade(c.text));
            let x = rect.left() + 20.0 + 18.0 + 10.0;
            let g = p.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, open, semibold_font(14.0), fade(c.text));
            if let Some(more) = more {
                p.text(pos2(g.right() + 10.0, rect.center().y + 1.0), Align2::LEFT_CENTER, more, FontId::proportional(12.0), fade(c.muted));
            }
            if fan.t >= 1.0 && resp.clicked() {
                events.push(Event::OpenFolder(fan.kind));
            }
        }
    }
}
