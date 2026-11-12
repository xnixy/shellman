//! Painting the notifications window: each toast on its own island (app, time and ✕ on top,
//! then title, body, buttons and the reply box), and in the list a pill with do-not-disturb and
//! "Clear all". Returns the islands, for the window's glass, and what was clicked.

use super::layout::{self, Parts, BUTTON_H, PAD_X, RADIUS};
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, TextureId, Ui, Vec2};
use shellman_core::notify::Toast;
use shellman_ui::chips::{chip, chip_width, round_icon, soft_colors, Chip};
use shellman_ui::glass::{islands, Island};
use shellman_ui::theme::{colors, glyph, icon_font, mix, semibold_font, with_alpha};
use shellman_ui::widgets::draw_text;
use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

const ICON: f32 = 16.0;
const CLOSE: f32 = 24.0;
const BUTTON_GAP: f32 = 8.0;

/// Loads an app's icon: None until it has arrived.
pub type Icons<'a> = &'a mut dyn FnMut(&egui::Context, &Path) -> Option<(TextureId, Vec2)>;

/// One toast as placed this frame.
pub struct Card<'a> {
    pub toast: &'a Toast,
    /// Where the island is now (moved while it slides).
    pub rect: Rect,
    pub parts: Parts,
    /// 1 is fully shown; less while it slides.
    pub opacity: f32,
    /// The small line under the body: attribution, or "2 more" in the list.
    pub small: Option<String>,
}

pub struct View<'a> {
    pub cards: Vec<Card<'a>>,
    /// The list's do-not-disturb pill, when the list is open.
    pub controls: Option<Rect>,
    pub dnd: bool,
    /// Glass is composed under the window; if not, the islands are painted here.
    pub composed: bool,
}

/// What was clicked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    /// The toast itself.
    Open(u32),
    /// One of its buttons, by index.
    Button(u32, usize),
    /// The reply's send button (or Enter in the box).
    Send(u32),
    Dismiss(u32),
    ToggleDnd,
    ClearAll,
    /// The reply box was clicked into: the window needs the keyboard.
    Focus,
}

pub struct Drawn {
    pub islands: Vec<Island>,
    pub events: Vec<Event>,
    /// A reply box has the keyboard.
    pub typing: bool,
}

/// The body font, for measuring as well as drawing.
pub fn body_font() -> FontId {
    FontId::proportional(13.5)
}

/// Wrapped text heights from egui's fonts, for `layout`.
pub struct Fonts<'a>(pub &'a egui::Context);

impl layout::Measure for Fonts<'_> {
    fn height(&self, text: &str, width: f32) -> f32 {
        self.0.fonts(|f| f.layout(text.to_owned(), body_font(), Color32::WHITE, width)).size().y
    }
}

pub fn draw(ctx: &egui::Context, view: &View, replies: &mut HashMap<u32, String>, icons: Icons) -> Drawn {
    let mut out = Drawn { islands: Vec::new(), events: Vec::new(), typing: false };
    egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
        let under = ui.painter().add(egui::Shape::Noop);
        for card in &view.cards {
            out.islands.push(Island { opacity: card.opacity, ..Island::new(card.rect, RADIUS) });
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(card.rect));
            child.multiply_opacity(card.opacity);
            draw_card(&mut child, card, replies, icons, &mut out);
        }
        if let Some(rect) = view.controls {
            out.islands.push(Island::pill(rect));
            controls(ui, rect, view.dnd, &mut out.events);
        }
        let style = super::style();
        if !view.composed {
            ui.painter().set(under, egui::Shape::Vec(islands::solid_shapes(&out.islands, &style)));
        } else if !colors().light {
            // As the top bar does: enough tint under small text over the clearest glass.
            let fill = crate::topbar::scrim(style.tint);
            let shapes = out
                .islands
                .iter()
                .map(|i| egui::Shape::rect_filled(i.rect, i.radius.round() as u8, with_alpha(fill, (fill.a() as f32 * i.opacity) as u8)))
                .collect();
            ui.painter().set(under, egui::Shape::Vec(shapes));
        }
    });
    out
}

fn draw_card(ui: &mut Ui, card: &Card, replies: &mut HashMap<u32, String>, icons: Icons, out: &mut Drawn) {
    let c = colors();
    let t = card.toast;
    let at = |r: Rect| r.translate(card.rect.min.to_vec2());
    // The whole island opens the toast; the controls on it are added later, so they win.
    let open = ui.interact(card.rect, egui::Id::new(("toast", t.id)), Sense::click());
    if open.clicked() {
        out.events.push(Event::Open(t.id));
    }

    // Header: icon, app, time, close.
    let header = at(card.parts.header);
    let mid = header.center().y;
    let source = t.app_icon_source();
    let icon = source.and_then(|p| icons(ui.ctx(), &p));
    let icon_rect = Rect::from_center_size(pos2(header.left() + ICON / 2.0, mid), Vec2::splat(ICON));
    match icon {
        Some((id, px)) => {
            egui::Image::from_texture((id, px)).paint_at(ui, icon_rect);
        }
        None => {
            ui.painter().text(icon_rect.center(), Align2::CENTER_CENTER, glyph::APP, icon_font(13.0), c.muted);
        }
    }
    let close = Rect::from_center_size(pos2(header.right() - CLOSE / 2.0 + 4.0, mid), Vec2::splat(CLOSE));
    let name_font = semibold_font(14.0);
    let name_w = text_width(ui, &t.app_name, name_font.clone()).min(header.width() - ICON - 120.0);
    let name = Rect::from_min_max(pos2(icon_rect.right() + 8.0, header.top()), pos2(icon_rect.right() + 8.0 + name_w, header.bottom()));
    draw_text(ui, name, &t.app_name, name_font, c.text, false);
    let secs = SystemTime::now().duration_since(t.time).map_or(0, |d| d.as_secs());
    let when = Rect::from_min_max(pos2(name.right() + 8.0, header.top()), pos2(close.left() - 4.0, header.bottom()));
    draw_text(ui, when, &layout::ago(secs), FontId::proportional(12.0), dim(), false);
    if round_icon(ui, close, egui::Id::new(("toast close", t.id)), glyph::CLOSE, true, false).clicked() {
        out.events.push(Event::Dismiss(t.id));
    }

    if let Some(r) = card.parts.title {
        draw_text(ui, at(r), &t.title, semibold_font(14.0), c.text, false);
    }
    if let Some(r) = card.parts.body {
        let r = at(r);
        let galley = ui.fonts(|f| f.layout(layout::body_text(t), body_font(), c.muted, r.width()));
        ui.painter().with_clip_rect(r.intersect(ui.clip_rect())).galley(r.min, galley, c.muted);
    }
    if let (Some(r), Some(text)) = (card.parts.small, &card.small) {
        draw_text(ui, at(r), text, FontId::proportional(12.0), dim(), false);
    }
    if let Some(r) = card.parts.buttons {
        let r = at(r);
        let mut x = r.left();
        for (i, action) in t.actions.iter().enumerate() {
            let w = chip_width(ui, &action.label, false, Chip::Soft).min(r.right() - x);
            if w < 40.0 {
                break;
            }
            let rect = Rect::from_min_size(pos2(x, r.top()), vec2(w, BUTTON_H));
            if chip(ui, rect, egui::Id::new(("toast button", t.id, i)), &action.label, None, Chip::Soft).clicked() {
                out.events.push(Event::Button(t.id, i));
            }
            x += w + BUTTON_GAP;
        }
    }
    if let (Some(r), Some(reply)) = (card.parts.reply, &t.reply) {
        let r = at(r);
        let label = if reply.send.label.is_empty() { "Send" } else { reply.send.label.as_str() };
        let send_w = chip_width(ui, label, false, Chip::Accent).max(64.0);
        let send = Rect::from_min_size(pos2(r.right() - send_w, r.top()), vec2(send_w, r.height()));
        let field = Rect::from_min_max(r.min, pos2(send.left() - BUTTON_GAP, r.bottom()));
        let (fill, _, line) = soft_colors(c.light);
        ui.painter().rect(field, (field.height() / 2.0) as u8, fill, line, egui::StrokeKind::Inside);
        let text = replies.entry(t.id).or_default();
        let edit = egui::TextEdit::singleline(text)
            .id(egui::Id::new(("toast reply", t.id)))
            .frame(false)
            .font(FontId::proportional(13.5))
            .hint_text(egui::RichText::new(if reply.placeholder.is_empty() { "Reply…" } else { &reply.placeholder }).color(dim()))
            .vertical_align(egui::Align::Center)
            .desired_width(field.width() - 2.0 * (PAD_X - 2.0));
        let inner = field.shrink2(vec2(PAD_X - 2.0, 2.0));
        let resp = ui.put(inner, edit);
        if resp.clicked() || resp.gained_focus() {
            out.events.push(Event::Focus);
        }
        out.typing |= resp.has_focus() || !text.is_empty();
        let entered = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let clicked = chip(ui, send, egui::Id::new(("toast send", t.id)), label, None, Chip::Accent).clicked();
        if (entered || clicked) && !text.trim().is_empty() {
            out.events.push(Event::Send(t.id));
        }
    }
}

/// "Do not disturb" with its switch, and "Clear all", in a pill.
fn controls(ui: &mut Ui, rect: Rect, dnd: bool, events: &mut Vec<Event>) {
    let c = colors();
    let mid = rect.center().y;
    ui.painter().text(pos2(rect.left() + PAD_X, mid), Align2::LEFT_CENTER, "Do not disturb", FontId::proportional(14.0), c.muted);
    let clear_w = chip_width(ui, "Clear all", false, Chip::Soft);
    let clear = Rect::from_min_size(pos2(rect.right() - 8.0 - clear_w, mid - 14.0), vec2(clear_w, 28.0));
    if chip(ui, clear, egui::Id::new("notifications clear"), "Clear all", None, Chip::Soft).clicked() {
        events.push(Event::ClearAll);
    }
    // The switch sits between, as in the mockup (38 by 22).
    let track = Rect::from_center_size(pos2(clear.left() - 24.0 - 19.0, mid), vec2(38.0, 22.0));
    let resp = ui.interact(track.expand(4.0), egui::Id::new("notifications dnd"), Sense::click());
    let t = ui.ctx().animate_bool(resp.id, dnd);
    // Off: the mockup's faint white track (#ffffff22), with a hairline so it reads on any glass.
    let (_, off, line) = soft_colors(c.light);
    let line = egui::Stroke::new(line.width, with_alpha(line.color, (line.color.a() as f32 * (1.0 - t)) as u8));
    ui.painter().rect(track, 11, mix(off, c.accent_fill, t), line, egui::StrokeKind::Inside);
    let knob = pos2(track.left() + 11.0 + 16.0 * t, mid);
    ui.painter().circle_filled(knob, 8.0, Color32::WHITE);
    if resp.clicked() {
        events.push(Event::ToggleDnd);
    }
}

fn dim() -> Color32 {
    let c = colors();
    mix(c.muted, c.bg, 0.3)
}

fn text_width(ui: &Ui, text: &str, font: FontId) -> f32 {
    ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, Color32::WHITE)).size().x
}

/// Where a toast's app icon comes from: the app's entry in the Apps folder, as the dock shows it.
trait IconSource {
    fn app_icon_source(&self) -> Option<std::path::PathBuf>;
}

impl IconSource for Toast {
    fn app_icon_source(&self) -> Option<std::path::PathBuf> {
        (!self.app_id.is_empty() && !self.app_id.contains('\\'))
            .then(|| std::path::PathBuf::from(format!("shell:AppsFolder\\{}", self.app_id)))
    }
}
