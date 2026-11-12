//! Small pieces every page is built from: the heading, field labels, and a segmented choice.

use egui::{Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Ui, Vec2};
use shellman_ui::theme::{self, colors};
use std::cell::{Cell, RefCell};

/// Corner radius of a group's island, in points.
pub const GROUP_RADIUS: f32 = 22.0;
/// Space between one island and the next.
pub const GROUP_GAP: f32 = 12.0;

thread_local! {
    /// The islands the page declared this frame, clipped to what is on screen.
    static GROUPS: RefCell<Vec<Rect>> = const { RefCell::new(Vec::new()) };
    /// How many groups are open around the code running now.
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Start collecting islands for a new frame.
pub fn begin_frame() {
    GROUPS.with(|g| g.borrow_mut().clear());
    DEPTH.with(|d| d.set(0));
}

/// The rectangles of the groups shown since [`begin_frame`], in drawing order.
pub fn take_groups() -> Vec<Rect> {
    GROUPS.with(|g| std::mem::take(&mut *g.borrow_mut()))
}

fn record(rect: Rect, ui: &Ui) {
    let visible = rect.intersect(ui.clip_rect());
    if visible.width() > 1.0 && visible.height() > 1.0 {
        GROUPS.with(|g| g.borrow_mut().push(visible));
    }
}

/// One island of the page: `add` is drawn on a piece of glass of its own, as wide as the
/// column, with `title` (if not empty) as a small caption on top. The rectangle it used is
/// remembered, clipped to the visible part of the scroll area, and the window turns it into
/// glass after the frame. Inside another group it only lays out, so a page wrapped whole in one
/// island (see `Section::grouped`) can still use groups.
pub fn group(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    let nested = DEPTH.with(Cell::get) > 0;
    DEPTH.with(|d| d.set(d.get() + 1));
    if nested {
        if !title.is_empty() {
            label(ui, title);
        }
        add(ui);
    } else {
        let frame = egui::Frame::new().inner_margin(egui::Margin::symmetric(22, 18));
        let out = frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                label(ui, title);
            }
            add(ui);
        });
        record(out.response.rect, ui);
        ui.add_space(GROUP_GAP);
    }
    DEPTH.with(|d| d.set(d.get() - 1));
}

/// The page's title and a line under it saying what it is for. On a page of groups the title is
/// already in the window's title pill, so only the line is shown, on a slim island of its own.
pub fn heading(ui: &mut Ui, title: &str, about: &str) {
    if DEPTH.with(Cell::get) == 0 {
        let frame = egui::Frame::new().inner_margin(egui::Margin::symmetric(22, 12));
        let out = frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::muted(about).size(12.5));
        });
        record(out.response.rect, ui);
        ui.add_space(GROUP_GAP);
        return;
    }
    ui.add_space(6.0);
    ui.label(RichText::new(title).size(20.0).strong().color(colors().text));
    ui.label(theme::muted(about).size(12.5));
    ui.add_space(16.0);
}

/// The small label over a control.
pub fn label(ui: &mut Ui, text: &str) {
    ui.label(theme::muted(text).size(12.0));
    ui.add_space(2.0);
}

/// Space between one setting and the next.
pub fn gap(ui: &mut Ui) {
    ui.add_space(20.0);
}

/// A row of options of which one is chosen, drawn as a track with the choice lifted out in the
/// accent. True when the choice changed this frame.
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
    ui.painter().rect_filled(track, CornerRadius::same(10), c.control);
    let mut changed = false;
    let mut x = track.left() + 3.0;
    for ((value, text), width) in options.iter().zip(&widths) {
        let seg = egui::Rect::from_min_size(
            egui::pos2(x, track.top() + 3.0),
            Vec2::new(*width, height - 6.0),
        );
        x += width;
        let resp = ui.interact(seg, ui.id().with(("segment", *text)), Sense::click());
        let selected = *value == *current;
        if selected {
            ui.painter().rect_filled(seg, CornerRadius::same(8), c.accent_fill);
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(seg, CornerRadius::same(8), theme::with_alpha(c.text, 14));
        }
        let color: Color32 = if selected { c.on_accent } else { c.text };
        ui.painter()
            .text(seg.center(), Align2::CENTER_CENTER, text, font.clone(), color);
        if resp.clicked() && !selected {
            *current = *value;
            changed = true;
        }
    }
    changed
}
