//! Small pieces every page is built from: the heading, field labels, and a segmented choice.

use crate::flow;
use egui::{Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Ui, UiBuilder, Vec2};
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
    /// The page's one-line description, waiting for the first island to carry it.
    static ABOUT: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Start collecting islands for a new frame.
pub fn begin_frame() {
    GROUPS.with(|g| g.borrow_mut().clear());
    DEPTH.with(|d| d.set(0));
    ABOUT.with(|a| a.borrow_mut().take());
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

/// One group's contents and its title, for [`pair`].
pub type Part<'a, T> = (&'a str, &'a mut dyn FnMut(&mut Ui, &mut T));

/// Draw one island's frame, its caption (the page's description, on the first island), its title
/// and `add`. Returns the rectangle it took.
fn framed(ui: &mut Ui, title: &str, add: &mut dyn FnMut(&mut Ui)) -> Rect {
    let frame = egui::Frame::new().inner_margin(egui::Margin::symmetric(24, 20));
    let out = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        if let Some(about) = ABOUT.with(|a| a.borrow_mut().take()) {
            ui.label(theme::muted(about).size(12.5));
            ui.add_space(14.0);
        }
        if !title.is_empty() {
            label(ui, title);
        }
        add(ui);
    });
    out.response.rect
}

/// One island of the page: `add` is drawn on a piece of glass of its own, as wide as the
/// column, with `title` (if not empty) as a small caption on top. The rectangle it used is
/// remembered, clipped to the visible part of the scroll area, and the window turns it into
/// glass after the frame. Inside another group it only lays out, so a page wrapped whole in one
/// island (see `Section::grouped`) can still use groups.
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
            label(ui, title);
        }
        once(ui);
    } else {
        let rect = framed(ui, title, &mut once);
        record(rect, ui);
        ui.add_space(GROUP_GAP);
    }
    DEPTH.with(|d| d.set(d.get() - 1));
}

/// Two small groups: side by side, as tall as the taller, when the column has room for both
/// (see `flow::two_up`), and one above the other when it has not. Both are given `state`, the
/// thing the page edits, one after the other, so neither has to own it.
pub fn pair<'a, T>(ui: &mut Ui, state: &mut T, left: Part<'a, T>, right: Part<'a, T>) {
    let full = ui.available_width();
    let nested = DEPTH.with(Cell::get) > 0;
    let Some(width) = flow::two_up(full, GROUP_GAP).filter(|_| !nested) else {
        group(ui, left.0, |ui| (left.1)(ui, state));
        group(ui, right.0, |ui| (right.1)(ui, state));
        return;
    };
    DEPTH.with(|d| d.set(d.get() + 1));
    let origin = ui.cursor().min;
    let mut rects = [Rect::NOTHING; 2];
    for (i, (title, add)) in [left, right].into_iter().enumerate() {
        let x = origin.x + i as f32 * (width + GROUP_GAP);
        let area = Rect::from_min_size(egui::pos2(x, origin.y), egui::vec2(width, 100_000.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(area));
        rects[i] = framed(&mut child, title, &mut |ui| add(ui, state));
    }
    let height = flow::row_height([rects[0].height(), rects[1].height()]);
    ui.allocate_rect(Rect::from_min_size(origin, egui::vec2(full, height)), Sense::hover());
    for r in rects {
        record(Rect::from_min_size(r.min, egui::vec2(width, height)), ui);
    }
    ui.add_space(GROUP_GAP);
    DEPTH.with(|d| d.set(d.get() - 1));
}

/// The page's title and a line under it saying what it is for. On a page of groups the title is
/// already in the window's title pill, so only the line is kept: the first island shows it as a
/// muted line at its top, which reads as a caption and not as a box of its own.
pub fn heading(ui: &mut Ui, title: &str, about: &str) {
    if DEPTH.with(Cell::get) == 0 {
        ABOUT.with(|a| *a.borrow_mut() = Some(about.to_owned()));
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
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, *text));
        theme::focus_ring(ui, &resp, seg, 8.0);
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
