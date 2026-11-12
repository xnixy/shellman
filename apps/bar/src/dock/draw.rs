//! Painting the dock: the icons on the glass with their running marks, the dividers, and the
//! name label above. It only reports what the pointer did; `Dock` decides what that means.
//!
//! Motion is short and only runs while something changes: icons slide to their new places and
//! the glass follows when apps come and go, a new icon fades in, and hover and press ease in and
//! out. Each is an egui animation, which asks for frames only until it is done, so a still dock
//! draws nothing.

use super::items::{Item, Section};
use super::metrics::{self, Metrics};
use egui::{Color32, Id, Pos2, Rect, Vec2};
use shellman_core::thumbs::Thumbs;
use shellman_ui::app_icon::{self, Kind};
use shellman_ui::theme::{colors, glyph, icon_font, mix, with_alpha};

/// How long icons and the glass take to move, and a new icon to fade in.
pub const MOVE: f32 = 0.16;
/// How long hover and press take to ease in or out.
const EASE: f32 = 0.12;
/// Icons whose longest side is under this many pixels are old small ones (see `paint_icon`).
const SMALL_PX: f32 = 48.0;

/// What the pointer did over the icons this frame; each is an index into the items.
#[derive(Default)]
pub struct Events {
    pub clicked: Option<usize>,
    pub middle: Option<usize>,
    pub menu: Option<usize>,
    /// Scrolled over an icon: its index and the wheel's vertical delta.
    pub scrolled: Option<(usize, f32)>,
    /// The icon under the pointer, and its centre in points from the window's left.
    pub hovered: Option<(usize, f32)>,
    /// The name label, where it was drawn this frame (it fades out after the pointer leaves).
    pub label: Option<Rect>,
}

pub struct Scene<'a> {
    pub items: &'a [Item],
    pub front: Option<isize>,
    pub metrics: Metrics,
    /// Each icon's left edge from the glass's left, and the glass's width (not animated).
    pub xs: &'a [f32],
    pub width: f32,
    /// The glass, in the window's points, as it is drawn this frame.
    pub glass: Rect,
    /// Keeps this dock window's animations apart from the other monitors'.
    pub salt: &'a str,
    /// The glass's tint, which the marks are drawn from.
    pub tint: Color32,
}

/// The palette of the dock's marks: the glass's own tint moved towards the text colour, so they
/// show whatever the accent (black included), in light and dark.
///
/// They are nearly opaque on purpose. Windows blends a window over its glass in linear light
/// (measured: a 1 px white line at 16% alpha came out as a 40% grey), so a faint translucent
/// white turns out far brighter than drawn; a solid colour comes out as drawn.
struct Ink {
    text: Color32,
    /// Behind a hovered icon, and a pressed one.
    hover: Color32,
    press: Color32,
    /// A running app's dot, the front app's pill, and the dividers.
    dot: Color32,
    front: Color32,
    divider: Color32,
    /// Behind an old small icon, or one still loading.
    plate: Color32,
}

fn ink(tint: Color32) -> Ink {
    let c = colors();
    let base = tint.to_opaque();
    let toward = |t: f32, alpha: u8| with_alpha(mix(base, c.text, t), alpha);
    Ink {
        text: c.text,
        hover: toward(0.11, 190),
        press: toward(0.18, 210),
        dot: toward(0.55, 255),
        front: toward(0.92, 255),
        divider: toward(0.22, 255),
        plate: toward(0.07, 200),
    }
}

pub fn paint(ui: &mut egui::Ui, scene: &Scene, thumbs: &mut Thumbs) -> Events {
    let ctx = ui.ctx().clone();
    let m = scene.metrics;
    let ink = ink(scene.tint);
    let mut events = Events::default();
    let glass = scene.glass;
    // Each icon's left edge, eased towards its place, from the glass's centre: when an app comes
    // or goes the others slide over while the glass grows or shrinks around them.
    let lefts: Vec<f32> = scene
        .items
        .iter()
        .zip(scene.xs)
        .map(|(item, &x)| {
            let id = Id::new(("dock x", scene.salt, &item.app_id));
            glass.center().x + ctx.animate_value_with_time(id, x - scene.width / 2.0, MOVE)
        })
        .collect();
    let top = glass.bottom() - m.pad - m.icon;
    for (i, item) in scene.items.iter().enumerate() {
        if metrics::divider_before(scene.items, i) {
            let x = (lefts[i - 1] + m.icon + lefts[i]) / 2.0;
            let half = m.icon * 0.28;
            let mid = top + m.icon / 2.0;
            ui.painter().line_segment(
                [egui::pos2(x, mid - half), egui::pos2(x, mid + half)],
                egui::Stroke::new(1.0_f32, ink.divider),
            );
        }
        let icon = Rect::from_min_size(egui::pos2(lefts[i], top), Vec2::splat(m.icon));
        // The cell takes the clicks: the icon and half the space to each side, the glass's full
        // height, so sliding along the dock never falls between two icons.
        let cell = Rect::from_x_y_ranges(icon.x_range().expand(4.0), glass.y_range());
        let id = Id::new(("dock icon", scene.salt, &item.app_id));
        let resp = ui.interact(cell, id, egui::Sense::click());
        let appear = appear(&ctx, id.with("appear"));
        let hover = ctx.animate_bool_with_time(id.with("hover"), resp.hovered(), EASE);
        let press = ctx.animate_bool_with_time(id.with("press"), resp.is_pointer_button_down_on(), EASE * 0.6);
        if hover > 0.0 {
            let plate = icon.expand(5.0);
            let fill = lerp_color(ink.hover, ink.press, press).gamma_multiply(hover);
            ui.painter().rect_filled(plate, m.icon * 0.22 + 5.0, fill);
        }
        // Pressed, the icon sinks a little; new, it grows in from 80%.
        let k = (1.0 - 0.06 * press) * (0.8 + 0.2 * appear);
        let shown = Rect::from_center_size(icon.center(), icon.size() * k);
        let tint = Color32::WHITE.gamma_multiply(appear).linear_multiply(1.0 - 0.12 * press);
        paint_icon(ui, item, shown, tint, ink.plate, thumbs);
        paint_mark(ui, item, scene.front, egui::pos2(icon.center().x, glass.bottom() - m.pad / 2.0), &ink, appear);

        if resp.hovered() {
            events.hovered = Some((i, icon.center().x));
            let delta = ui.input(|i| i.raw_scroll_delta.y);
            if delta != 0.0 {
                events.scrolled = Some((i, delta));
            }
        }
        if resp.clicked() {
            events.clicked = Some(i);
        }
        if resp.middle_clicked() {
            events.middle = Some(i);
        }
        if resp.secondary_clicked() {
            events.menu = Some(i);
        }
    }
    // An icon without windows names itself; one with windows shows their previews instead.
    let named = events.hovered.filter(|&(i, _)| scene.items[i].windows.is_empty());
    events.label = paint_label(ui, scene, named.map(|(i, x)| (scene.items[i].name(), x)), &ink);
    events
}

/// 0 for an icon seen for the first time, easing to 1: new icons fade in.
fn appear(ctx: &egui::Context, id: Id) -> f32 {
    if !ctx.data(|d| d.get_temp::<()>(id).is_some()) {
        ctx.data_mut(|d| d.insert_temp(id, ()));
        ctx.animate_bool_with_time(id, false, MOVE);
    }
    ctx.animate_bool_with_time(id, true, MOVE)
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// The image for an item, drawn sharp at `px` pixels: shellman's own icons (the Launcher and
/// Files) in the accent, everything else from the shell.
fn texture(ctx: &egui::Context, item: &Item, px: f32, thumbs: &mut Thumbs) -> Option<(egui::TextureId, Vec2)> {
    if item.section == Section::Launcher {
        return Some(app_icon::kind_texture_px(ctx, Kind::Shellman, px));
    }
    let path = item.icon_source()?;
    match app_icon::kind_for_exe(&path) {
        Some(kind) => Some(app_icon::kind_texture_px(ctx, kind, px)),
        None => thumbs.get(&path, true).map(|t| (t.id(), t.size_vec2())),
    }
}

/// The item's icon filling `rect`. The shell's images come cropped to what they draw (see
/// `Thumbs::icons`): a real big icon fills the box; an old small one is drawn at a whole-number
/// scale on a faint plate, as the launcher does, rather than blown up and blurred.
fn paint_icon(ui: &egui::Ui, item: &Item, rect: Rect, tint: Color32, plate: Color32, thumbs: &mut Thumbs) {
    let ppp = ui.ctx().pixels_per_point();
    let plate = |ui: &egui::Ui| {
        let fill = plate.gamma_multiply(tint.a() as f32 / 255.0);
        ui.painter().rect_filled(rect, rect.width() * 0.22, fill);
    };
    let Some((id, px)) = texture(ui.ctx(), item, rect.width() * ppp, thumbs) else {
        plate(ui);
        let c = colors();
        let glyph_color = c.muted.gamma_multiply(tint.a() as f32 / 255.0);
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, glyph::APP, icon_font(rect.width() * 0.4), glyph_color);
        return;
    };
    let shown = if px.max_elem() < SMALL_PX {
        plate(ui);
        let k = (rect.width() * 0.62 * ppp / px.max_elem()).floor().max(1.0);
        px * k / ppp
    } else {
        px * (rect.width() / px.max_elem())
    };
    let uv = Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
    ui.painter().image(id, Rect::from_center_size(rect.center(), shown), uv, tint);
}

/// A running app's mark under its icon: a soft dot, and a wider, brighter pill for the app in
/// front. Pins that are not running have none.
fn paint_mark(ui: &egui::Ui, item: &Item, front: Option<isize>, at: Pos2, ink: &Ink, alpha: f32) {
    if item.windows.is_empty() {
        return;
    }
    let p = ui.painter();
    if front.is_some_and(|f| item.windows.contains(&f)) {
        let pill = Rect::from_center_size(at, egui::vec2(12.0, 4.0));
        p.rect_filled(pill, 2.0, ink.front.gamma_multiply(alpha));
    } else {
        p.circle_filled(at, 2.0, ink.dot.gamma_multiply(alpha));
    }
}

/// The name label above the hovered icon, `named` being its text and the icon's centre. It eases
/// in and out; while it fades out it keeps its last text and place. Returns where it was drawn.
fn paint_label(ui: &egui::Ui, scene: &Scene, named: Option<(String, f32)>, ink: &Ink) -> Option<Rect> {
    let ctx = ui.ctx();
    let id = Id::new(("dock label", scene.salt));
    if let Some(named) = &named {
        ctx.data_mut(|d| d.insert_temp(id, named.clone()));
    }
    let shown = ctx.animate_bool_with_time(id, named.is_some(), EASE);
    if shown <= 0.0 {
        return None;
    }
    let (text, center) = ctx.data(|d| d.get_temp::<(String, f32)>(id))?;
    let font = egui::FontId::proportional(13.0);
    let galley = ui.painter().layout_no_wrap(text, font, ink.text.gamma_multiply(shown));
    let window_w = ui.max_rect().width();
    let rect = metrics::label_rect(center, galley.size().x, scene.glass.top(), window_w);
    // Rises a couple of points as it appears.
    let rect = rect.translate(egui::vec2(0.0, 3.0 * (1.0 - shown)));
    let c = colors();
    let (fill, rim) = if c.light {
        (Color32::from_rgba_unmultiplied(250, 250, 252, 246), Color32::from_black_alpha(26))
    } else {
        (Color32::from_rgba_unmultiplied(40, 40, 44, 246), Color32::from_white_alpha(28))
    };
    let shadow = egui::epaint::Shadow {
        offset: [0, 1],
        blur: 6,
        spread: 0,
        color: Color32::from_black_alpha(if c.light { 30 } else { 70 }).gamma_multiply(shown),
    };
    let radius = egui::CornerRadius::same(8);
    ui.painter().add(shadow.as_shape(rect, radius));
    ui.painter().rect(
        rect,
        radius,
        fill.gamma_multiply(shown),
        egui::Stroke::new(1.0_f32, rim.gamma_multiply(shown)),
        egui::StrokeKind::Inside,
    );
    let at = rect.center() - galley.size() / 2.0;
    ui.painter().galley(at, galley, ink.text);
    Some(rect)
}
