//! Small pieces every page is built from: the heading, field labels, a segmented choice, and the
//! rows, fields and bars drawn inside a group.

use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};
use shellman_ui::theme::{self, colors, shape};
use std::cell::Cell;

/// Corner radius of a group's box, in points.
pub const GROUP_RADIUS: f32 = shellman_ui::theme::shape::ISLAND;
/// Space between one group and the next.
pub const GROUP_GAP: f32 = 22.0;
/// A group box's padding: the rows' text starts this far in, and hairlines between rows run
/// through it to the box's edges.
const PAD_X: f32 = 18.0;
const PAD_Y: f32 = 6.0;
/// Above and below a row's text.
const ROW_PAD: f32 = 14.0;

thread_local! {
    /// How many groups are open around the code running now.
    static DEPTH: Cell<u32> = const { Cell::new(0) };
    /// How many rows the open group has drawn, so the next knows to draw a hairline first.
    static ROWS: Cell<u32> = const { Cell::new(0) };
}

/// Start a new frame of the page.
pub fn begin_frame() {
    DEPTH.with(|d| d.set(0));
    ROWS.with(|r| r.set(0));
}

/// Draw one group: its title above, then Glasshouse's boxed list, a rounded box in the card fill
/// with a hairline, holding `add`. Returns the box's rectangle.
fn framed(ui: &mut Ui, title: &str, add: &mut dyn FnMut(&mut Ui)) -> Rect {
    if !title.is_empty() {
        group_title(ui, title);
    }
    let frame = shellman_ui::sheet::card_frame(egui::Margin::symmetric(PAD_X as i8, PAD_Y as i8));
    ROWS.with(|r| r.set(0));
    let out = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
    out.response.rect
}

/// One group of the page, as wide as the column: `title` (if not empty) in small capitals above
/// a box holding `add`. Inside another group it only lays out, so a page wrapped whole in one
/// group (see `Section::grouped`) can still use groups.
pub fn group(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    let mut add = Some(add);
    let mut once = |ui: &mut Ui| {
        if let Some(add) = add.take() {
            add(ui);
        }
    };
    let nested = DEPTH.with(Cell::get) > 0;
    DEPTH.with(|d| d.set(d.get() + 1));
    if nested {
        if !title.is_empty() {
            separator(ui);
            ui.add_space(10.0);
            ui.label(theme::group_heading(title));
            ui.add_space(4.0);
            ROWS.with(|r| r.set(0));
        }
        once(ui);
    } else {
        framed(ui, title, &mut once);
        ui.add_space(GROUP_GAP);
    }
    DEPTH.with(|d| d.set(d.get() - 1));
}

/// The page's title and a line under it saying what it is for. The title is in the window's
/// header bar, so on a page of groups only the line is kept, muted at the page's top. A page
/// that is one box as a whole (see `Section::grouped`) shows both inside it.
pub fn heading(ui: &mut Ui, title: &str, about: &str) {
    if DEPTH.with(Cell::get) == 0 {
        ui.add_space(2.0);
        ui.add(egui::Label::new(theme::muted(about).size(13.0)).wrap());
        ui.add_space(18.0);
        return;
    }
    ui.add_space(10.0);
    ui.label(RichText::new(title).size(20.0).strong().color(colors().text));
    ui.label(theme::muted(about).size(12.5));
    ui.add_space(12.0);
}

/// A group's title, above its box: Glasshouse's preferences-group heading, upper case and
/// spaced out, in line with the text in the box.
fn group_title(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(PAD_X - 4.0);
        ui.label(theme::group_heading(text));
    });
    ui.add_space(6.0);
}

/// The small label over a control.
pub fn label(ui: &mut Ui, text: &str) {
    ui.label(theme::muted(text).size(12.0));
    ui.add_space(2.0);
}

/// A hairline across the whole box, through its padding to its edges, between one row and the
/// next.
fn separator(ui: &mut Ui) {
    let c = colors();
    let inner = ui.max_rect();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    let edge = if c.high_contrast { c.text } else { c.hairline };
    ui.painter().hline((inner.left() - PAD_X)..=(inner.right() + PAD_X), rect.center().y, Stroke::new(1.0_f32, edge));
}

/// One row of a group, Glasshouse's action row: `title` and a muted `detail` (if any) on the
/// left, `control` on the right, a hairline above it when a row came before.
pub fn row<R>(ui: &mut Ui, title: &str, detail: &str, control: impl FnOnce(&mut Ui) -> R) -> R {
    row_with(ui, |ui| {
        theme::trailing(ui, control, |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.add(egui::Label::new(RichText::new(title).size(13.5).color(colors().text)).wrap());
                if !detail.is_empty() {
                    ui.add(egui::Label::new(theme::muted(detail).size(12.5)).wrap());
                }
            });
        })
    })
}

/// A row of a group holding anything (a slider with its own name, a line of status): the
/// hairline above it when a row came before, and a row's room around `add`.
pub fn row_with<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    if ROWS.with(Cell::get) > 0 {
        separator(ui);
    }
    ui.add_space(ROW_PAD);
    let r = add(ui);
    ui.add_space(ROW_PAD);
    ROWS.with(|n| n.set(n.get() + 1));
    r
}

/// A row for a control too wide to sit beside its title (a slider, a picker, a row of chips):
/// `title` and a muted `detail` (if any) on top, `control` under them at the full width.
pub fn stacked<R>(ui: &mut Ui, title: &str, detail: &str, control: impl FnOnce(&mut Ui) -> R) -> R {
    row_with(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.add(egui::Label::new(RichText::new(title).size(13.5).color(colors().text)).wrap());
        if !detail.is_empty() {
            ui.add(egui::Label::new(theme::muted(detail).size(12.5)).wrap());
        }
        ui.add_space(5.0);
        control(ui)
    })
}

/// Above and below the content of a list item (see [`item`]), tighter than a setting's row.
const ITEM_PAD: f32 = 10.0;

/// How far one list item reaches down the page when its content is `content` tall: the hairline
/// above it, its padding and the content. A list drawn only where it is in view needs this.
pub fn item_stride(content: f32) -> f32 {
    1.0 + 2.0 * ITEM_PAD + content
}

/// One item of a list inside a group (an app, a device, a file): a row of the box with a
/// hairline above it when a row came before, `add` drawn in it, and Glasshouse's hover wash
/// laid under it (the selection wash when `selected`), inset from the box's edges so the washes
/// stay clear of its rounded corners. High contrast outlines the hovered item in the text colour
/// and the chosen one in highlight. The item is as wide as the box. Space between widgets is
/// zero inside it, so that every item is exactly [`item_stride`] tall.
pub fn item<R>(ui: &mut Ui, selected: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    let spacing = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.0;
    if ROWS.with(Cell::get) > 0 {
        separator(ui);
    }
    let left = ui.cursor().left();
    let (top, width) = (ui.cursor().top(), ui.available_width());
    let slot = ui.painter().add(egui::Shape::Noop);
    ui.add_space(ITEM_PAD);
    let r = add(ui);
    ui.add_space(ITEM_PAD);
    let rect = Rect::from_min_max(egui::pos2(left, top), egui::pos2(left + width, ui.cursor().top()));
    ui.spacing_mut().item_spacing.y = spacing;
    let wash = Rect::from_min_max(
        egui::pos2(rect.left() - (PAD_X - 6.0), rect.top() + 2.0),
        egui::pos2(rect.right() + (PAD_X - 6.0), rect.bottom() - 2.0),
    );
    let hovered = ui.rect_contains_pointer(rect);
    let c = colors();
    if c.high_contrast {
        theme::contrast_outline(ui, wash, shape::TILE, hovered, selected);
    } else if selected || hovered {
        let fill = if selected { c.row_selected } else { c.row_hover };
        ui.painter().set(slot, egui::Shape::rect_filled(wash, shape::corner(shape::TILE), fill));
    }
    ROWS.with(|n| n.set(n.get() + 1));
    r
}

/// A row of options of which one is chosen, drawn as Glasshouse's pills in a pill: a track in
/// the field fill with the choice raised out of it as a chip (white with a soft shadow on light,
/// a stronger faint fill on dark). The accent stays for what is on. High contrast: the track
/// outlined, the choice in highlight under highlight text. True when the choice changed this
/// frame.
pub fn segmented<T: Copy + PartialEq>(ui: &mut Ui, current: &mut T, options: &[(T, &str)]) -> bool {
    let c = colors();
    let font = FontId::proportional(13.0);
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, text)| {
            ui.fonts(|f| f.layout_no_wrap((*text).to_owned(), font.clone(), c.text))
                .size()
                .x
                + 28.0
        })
        .collect();
    let height = 32.0;
    let (track, _) = ui.allocate_exact_size(
        Vec2::new(widths.iter().sum::<f32>() + 6.0, height),
        Sense::hover(),
    );
    if c.high_contrast {
        ui.painter().rect(track, shape::pill(track), c.bg, Stroke::new(1.0_f32, c.text), StrokeKind::Inside);
    } else {
        ui.painter().rect_filled(track, shape::pill(track), c.field);
    }
    let mut changed = false;
    let mut x = track.left() + 3.0;
    for ((value, text), width) in options.iter().zip(&widths) {
        let seg = egui::Rect::from_min_size(
            egui::pos2(x, track.top() + 3.0),
            Vec2::new(*width, height - 6.0),
        );
        x += width;
        let round = shape::pill(seg);
        let resp = ui.interact(seg, ui.id().with(("segment", *text)), Sense::click());
        let selected = *value == *current;
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, *text));
        theme::focus_ring(ui, &resp, seg, seg.height() / 2.0);
        let mut color: Color32 = c.text;
        if c.high_contrast {
            if selected {
                ui.painter().rect_filled(seg, round, c.accent_fill);
                color = c.on_accent;
            } else {
                theme::contrast_outline(ui, seg, seg.height() / 2.0, resp.hovered(), false);
            }
        } else if selected {
            if c.light {
                let shadow = egui::epaint::Shadow { offset: [0, 1], blur: 4, spread: 0, color: Color32::from_black_alpha(36) };
                ui.painter().add(shadow.as_shape(seg, round));
            }
            ui.painter().rect_filled(seg, round, c.control_active);
        } else if resp.hovered() {
            ui.painter().rect_filled(seg, round, c.row_hover);
        }
        let font = if selected { theme::semibold_font(13.0) } else { font.clone() };
        ui.painter()
            .text(seg.center(), Align2::CENTER_CENTER, text, font, color);
        if resp.clicked() && !selected {
            *current = *value;
            changed = true;
        }
    }
    changed
}

/// The box behind a text field the page lays out itself (a search box): the field fill rounded
/// as a field. High contrast outlines it in the text colour, as egui's own fields are.
pub fn field_fill(ui: &Ui, rect: Rect) {
    shellman_ui::sheet::field(ui, rect);
}

/// A thin bar: a pill track in the field fill with `fraction` (0 to 1) of it in `fill`. Anything
/// above nothing shows at least a round dot. High contrast outlines the track.
pub fn thin_bar(ui: &Ui, rect: Rect, fraction: f32, fill: Color32) {
    let c = colors();
    let round = shape::pill(rect);
    if c.high_contrast {
        ui.painter().rect(rect, round, c.bg, Stroke::new(1.0_f32, c.text), StrokeKind::Inside);
    } else {
        ui.painter().rect_filled(rect, round, c.field);
    }
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction > 0.0 {
        let w = (rect.width() * fraction).max(rect.height()).min(rect.width());
        let part = Rect::from_min_size(rect.min, Vec2::new(w, rect.height()));
        ui.painter().rect_filled(part, shape::pill(part), fill);
    }
}

/// A small pill button on a row or in a group: a quiet one in the field fill (it shows on a row
/// because it sits on the row's own fill), or Glasshouse's red text on a faint red wash for one
/// that removes something. Both outlined in high contrast. `height` is its height.
pub fn row_button(text: &str, danger: bool, height: f32) -> egui::Button<'static> {
    let b = if danger { theme::danger_button(text) } else { theme::secondary_button(text) };
    b.corner_radius(shape::corner(height / 2.0)).min_size(Vec2::new(0.0, height))
}
