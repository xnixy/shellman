//! A right-click menu drawn by shellman: rows with an icon column, a label and a shortcut or
//! chevron, submenus that open on hover or with the arrow keys, and a scroll when it is taller
//! than the window. The rows are plain data, rebuilt each frame, so entries that arrive late (the
//! shell's) simply appear in the next frame.

mod keys;
mod row;

use egui::{Area, Context, Id, Key, Order, Pos2, Rect, Sense, Vec2};
use std::time::Duration;

pub use row::{Icon, ROW_H};

/// One line of a menu.
#[derive(Clone)]
pub struct Row<T> {
    pub icon: Icon,
    pub label: String,
    pub shortcut: String,
    pub enabled: bool,
    pub checked: bool,
    /// The letter that picks it from the keyboard.
    pub mnemonic: Option<char>,
    pub kind: Kind<T>,
}

#[derive(Clone)]
pub enum Kind<T> {
    Action(T),
    Submenu {
        rows: Vec<Row<T>>,
        /// Sent back as `Event::Opened` when the submenu opens: for entries filled on demand.
        on_open: Option<T>,
    },
    Separator,
    /// A quiet line that cannot be picked: "Loading", "2 items took too long".
    Note,
    /// A row someone else drew (an owner-drawn shell entry), shown as the picture it is, picked
    /// as `T`.
    Picture(egui::TextureId, Vec2, T),
}

impl<T> Row<T> {
    pub fn action(icon: Icon, label: impl Into<String>, shortcut: impl Into<String>, value: T) -> Self {
        Self {
            icon,
            label: label.into(),
            shortcut: shortcut.into(),
            enabled: true,
            checked: false,
            mnemonic: None,
            kind: Kind::Action(value),
        }
    }

    pub fn separator() -> Self {
        Self::plain(Kind::Separator, "")
    }

    pub fn note(text: impl Into<String>) -> Self {
        Self::plain(Kind::Note, text)
    }

    fn plain(kind: Kind<T>, label: impl Into<String>) -> Self {
        Self {
            icon: Icon::None,
            label: label.into(),
            shortcut: String::new(),
            enabled: false,
            checked: false,
            mnemonic: None,
            kind,
        }
    }

    /// Whether the keyboard can land on it.
    fn selectable(&self) -> bool {
        !matches!(self.kind, Kind::Separator | Kind::Note)
    }

    fn submenu(&self) -> Option<&[Row<T>]> {
        match &self.kind {
            Kind::Submenu { rows, .. } => Some(rows),
            _ => None,
        }
    }
}

/// What the person did.
pub enum Event<T> {
    Picked(T),
    /// A submenu with `on_open` opened.
    Opened(T),
}

/// Hovering another row this long closes an open submenu or opens another, so a diagonal move
/// toward a submenu does not close it on the way.
const SWITCH: Duration = Duration::from_millis(220);
/// Gap between a menu and its submenu.
const SUB_GAP: f32 = 2.0;

/// An open menu's state between frames.
#[derive(Default)]
pub struct Menu {
    open: bool,
    pos: Pos2,
    /// The submenu row open at each level, from the top.
    chain: Vec<usize>,
    /// The highlighted row of each level (one more than `chain`).
    hot: Vec<Option<usize>>,
    /// The row hovered at some level, and since when, for opening and closing submenus.
    dwell: Option<(usize, usize, f64)>,
    /// Each level's rectangle last frame, for clicks outside.
    rects: Vec<Rect>,
    opened_frame: u64,
    scroll_to_hot: bool,
    /// The height the last menu of this kind ended up with, to place this one once.
    height_hint: Option<f32>,
    /// Whether the position was settled (kept inside the window) on the first frame.
    placed: bool,
}

impl Menu {
    pub fn open_at(&mut self, ctx: &Context, pos: Pos2) {
        *self = Self {
            open: true,
            pos,
            hot: vec![None],
            opened_frame: ctx.cumulative_pass_nr(),
            ..Default::default()
        };
    }

    /// The height a menu of this kind is expected to have, from the last one. Call after
    /// `open_at`; the menu is then placed once, so entries arriving later do not move it.
    pub fn expect_height(&mut self, height: Option<f32>) {
        self.height_hint = height;
    }

    /// The first level's height as last drawn.
    pub fn height(&self) -> Option<f32> {
        self.rects.first().map(|r| r.height())
    }

    pub fn close(&mut self) {
        self.open = false;
        self.chain.clear();
        self.hot.clear();
        self.rects.clear();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Draws the open menu. Returns what was picked; the menu closes itself on a pick, a click
    /// elsewhere, or Escape.
    pub fn show<T: Clone>(&mut self, ctx: &Context, id: Id, rows: &[Row<T>]) -> Option<Event<T>> {
        if !self.open {
            return None;
        }
        let mut event = self.keyboard(ctx, rows);
        let mut level_rows: &[Row<T>] = rows;
        if !self.placed {
            self.placed = true;
            let screen = ctx.screen_rect();
            let max_h = (screen.height() - 16.0).max(ROW_H * 3.0);
            let own = (row::content_height(rows) + 2.0 * row::PAD).min(max_h);
            let h = self.height_hint.unwrap_or(0.0).max(own).min(max_h);
            if self.pos.y + h > screen.bottom() - 4.0 {
                self.pos.y = (screen.bottom() - 4.0 - h).max(screen.top() + 4.0);
            }
        }
        let mut pos = self.pos;
        let mut rects = Vec::new();
        let mut parent: Option<Rect> = None;
        let screen = ctx.screen_rect();
        for level in 0..=self.chain.len() {
            let width = row::menu_width(ctx, level_rows);
            if level == 0 && pos.x + row::MAX_W > screen.right() {
                // Left of the pointer when the widest menu would not fit right of it, judged on
                // the widest so the side does not change as entries arrive.
                pos.x = (pos.x - width).max(screen.left());
            }
            if let (Some(p), Some(open_rect)) = (parent, self.open_row_rect(ctx, id, level)) {
                // Beside the parent, to the right if it fits, else to the left.
                pos = if p.right() + SUB_GAP + width <= screen.right() {
                    egui::pos2(p.right() + SUB_GAP, open_rect.top() - row::PAD)
                } else {
                    egui::pos2(p.left() - SUB_GAP - width, open_rect.top() - row::PAD)
                };
            }
            let (rect, ev) = self.level(ctx, id, level, pos, width, level_rows);
            rects.push(rect);
            event = event.or(ev);
            parent = Some(rect);
            let Some(&open) = self.chain.get(level) else { break };
            match level_rows.get(open).and_then(Row::submenu) {
                Some(sub) => level_rows = sub,
                None => {
                    self.chain.truncate(level);
                    self.hot.truncate(level + 1);
                    break;
                }
            }
        }
        self.rects = rects;
        // A press anywhere outside the menu closes it. The press that opened it does not.
        let pressed = ctx.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten());
        if let Some(p) = pressed {
            if ctx.cumulative_pass_nr() > self.opened_frame && !self.rects.iter().any(|r| r.contains(p)) {
                self.close();
            }
        }
        if matches!(event, Some(Event::Picked(_))) {
            self.close();
        }
        event
    }

    /// Where the open submenu's row of `level - 1` was drawn, from egui's memory of it.
    fn open_row_rect(&self, ctx: &Context, id: Id, level: usize) -> Option<Rect> {
        let row = *self.chain.get(level.checked_sub(1)?)?;
        ctx.data(|d| d.get_temp::<Rect>(row_id(id, level - 1, row)))
    }

    /// One level: its frame and rows. Returns its rectangle and anything picked or opened.
    fn level<T: Clone>(
        &mut self,
        ctx: &Context,
        id: Id,
        level: usize,
        pos: Pos2,
        width: f32,
        rows: &[Row<T>],
    ) -> (Rect, Option<Event<T>>) {
        let mut event = None;
        let max_h = (ctx.screen_rect().height() - 16.0).max(ROW_H * 3.0);
        let area = Area::new(id.with(("menu", level)))
            .order(Order::Foreground)
            .fixed_pos(pos)
            .constrain(true)
            .show(ctx, |ui| {
                row::frame().show(ui, |ui| {
                    ui.set_width(width - 2.0 * row::PAD);
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    let content = row::content_height(rows);
                    let mut draw = |ui: &mut egui::Ui| {
                        for (i, r) in rows.iter().enumerate() {
                            if let Some(ev) = self.draw_row(ui, id, level, i, r) {
                                event = Some(ev);
                            }
                        }
                    };
                    if content > max_h - 2.0 * row::PAD {
                        // A fixed height: left to itself the scroll area would size from the
                        // menu's size last frame and grow a little each frame.
                        let view = max_h - 2.0 * row::PAD;
                        egui::ScrollArea::vertical()
                            .id_salt(("menu-scroll", level))
                            .min_scrolled_height(view)
                            .max_height(view)
                            .show(ui, draw);
                    } else {
                        draw(ui);
                    }
                });
            });
        (area.response.rect, event)
    }

    fn draw_row<T: Clone>(&mut self, ui: &mut egui::Ui, id: Id, level: usize, i: usize, r: &Row<T>) -> Option<Event<T>> {
        let h = row::height(r);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
        if !r.selectable() {
            row::paint(ui, rect, r, false, false);
            return None;
        }
        ui.ctx().data_mut(|d| d.insert_temp(row_id(id, level, i), rect));
        let resp = ui.interact(rect, row_id(id, level, i), Sense::click());
        let now = ui.input(|inp| inp.time);
        if resp.hovered() && ui.input(|inp| inp.pointer.delta() != Vec2::ZERO || inp.pointer.any_pressed()) {
            self.hot.truncate(level + 1);
            self.hot.resize(level + 1, None);
            self.hot[level] = Some(i);
            if self.dwell.map(|(l, row, _)| (l, row)) != Some((level, i)) {
                self.dwell = Some((level, i, now));
            }
        }
        let hot = self.hot.get(level).copied().flatten() == Some(i);
        let open = self.chain.get(level) == Some(&i);
        if hot && self.scroll_to_hot && level + 1 == self.hot.len() {
            ui.scroll_to_rect(rect, None);
            self.scroll_to_hot = false;
        }
        row::paint(ui, rect, r, hot || open, open);
        let mut event = None;
        // Hover: open this submenu, or close another, once the pointer has rested here.
        if let Some((l, row, since)) = self.dwell {
            if l == level && row == i && resp.hovered() {
                let rested = now - since >= SWITCH.as_secs_f64();
                let none_open = self.chain.len() == level;
                if let Kind::Submenu { on_open, .. } = &r.kind {
                    if !open && r.enabled && (none_open || rested) {
                        event = self.open_sub(level, i, on_open.clone(), false);
                    }
                } else if !none_open && rested {
                    self.chain.truncate(level);
                    self.hot.truncate(level + 1);
                }
                if !rested {
                    ui.ctx().request_repaint_after(SWITCH);
                }
            }
        }
        if resp.clicked() && r.enabled {
            event = match &r.kind {
                Kind::Action(v) | Kind::Picture(_, _, v) => Some(Event::Picked(v.clone())),
                Kind::Submenu { on_open, .. } if !open => self.open_sub(level, i, on_open.clone(), false),
                _ => event,
            };
        }
        event
    }

    fn open_sub<T>(&mut self, level: usize, i: usize, on_open: Option<T>, keyboard: bool) -> Option<Event<T>> {
        self.chain.truncate(level);
        self.chain.push(i);
        self.hot.truncate(level + 1);
        self.hot.push(None);
        if keyboard {
            self.scroll_to_hot = true;
        }
        on_open.map(Event::Opened)
    }

    /// Arrow keys move within the deepest open level, Right and Enter open a submenu, Left and
    /// Escape close one, Enter picks, and a row's letter picks it.
    fn keyboard<T: Clone>(&mut self, ctx: &Context, rows: &[Row<T>]) -> Option<Event<T>> {
        let level = self.chain.len();
        let mut current: &[Row<T>] = rows;
        for &i in &self.chain {
            current = current.get(i).and_then(Row::submenu).unwrap_or(&[]);
        }
        let selectable: Vec<bool> = current.iter().map(Row::selectable).collect();
        self.hot.resize(level + 1, None);
        let hot = self.hot[level];
        let pressed = |k: Key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, k));
        let mut moved = None;
        if pressed(Key::ArrowDown) {
            moved = keys::step(&selectable, hot, true);
        } else if pressed(Key::ArrowUp) {
            moved = keys::step(&selectable, hot, false);
        } else if pressed(Key::Home) {
            moved = keys::step(&selectable, None, true);
        } else if pressed(Key::End) {
            moved = keys::step(&selectable, None, false);
        }
        if moved.is_some() {
            self.hot[level] = moved;
            self.scroll_to_hot = true;
            return None;
        }
        if pressed(Key::Escape) || (level > 0 && pressed(Key::ArrowLeft)) {
            if level == 0 {
                self.close();
            } else {
                self.chain.pop();
                self.hot.pop();
            }
            return None;
        }
        let typed = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Text(t) if t.chars().count() == 1 => t.chars().next(),
                _ => None,
            })
        });
        let by_letter = typed.and_then(|ch| keys::mnemonic(current.iter().map(|r| (r.mnemonic, r.enabled)), ch));
        let enter = pressed(Key::Enter) || pressed(Key::Space);
        let right = pressed(Key::ArrowRight);
        let target = by_letter.or(if enter || right { hot } else { None })?;
        let row = current.get(target)?;
        if !row.enabled {
            return None;
        }
        self.hot[level] = Some(target);
        match &row.kind {
            Kind::Submenu { rows: sub, on_open } => {
                let ev = self.open_sub(level, target, on_open.clone(), true);
                let first = keys::step(&sub.iter().map(Row::selectable).collect::<Vec<_>>(), None, true);
                if let Some(h) = self.hot.last_mut() {
                    *h = first;
                }
                ev
            }
            Kind::Action(v) | Kind::Picture(_, _, v) if !right => Some(Event::Picked(v.clone())),
            _ => None,
        }
    }
}

/// One entry of an ordinary egui popup or context menu, drawn as a row of shellman's own menu:
/// flat, lit only while hovered, with an icon column, the label, and a shortcut on the right; a
/// tick in the icon column when `checked` and there is no icon. Use it for every entry of a
/// popup so the popup reads like the right-click menu. True when clicked.
///
/// ```ignore
/// resp.context_menu(|ui| {
///     if menu::flat_item(ui, Icon::Glyph(glyph::ADD), "Open in new tab", "", false, true) {
///         ui.close_menu();
///     }
/// });
/// ```
pub fn flat_item(ui: &mut egui::Ui, icon: Icon, label: &str, shortcut: &str, checked: bool, enabled: bool) -> bool {
    let row = Row::<()> {
        icon,
        label: label.to_owned(),
        shortcut: shortcut.to_owned(),
        enabled,
        checked,
        mnemonic: None,
        kind: Kind::Action(()),
    };
    let width = ui.available_width().max(row::menu_width(ui.ctx(), std::slice::from_ref(&row)) - 2.0 * row::PAD);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, ROW_H), if enabled { Sense::click() } else { Sense::hover() });
    row::paint(ui, rect, &row, resp.hovered(), false);
    enabled && resp.clicked()
}

/// A thin line between groups of [`flat_item`]s.
pub fn flat_separator(ui: &mut egui::Ui) {
    let row = Row::<()>::separator();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row::height(&row)), Sense::hover());
    row::paint(ui, rect, &row, false, false);
}

/// Make an egui popup's own spacing match the flat rows: no gaps between them. Call first in
/// the popup.
pub fn flat_popup(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing = Vec2::ZERO;
}

fn row_id(id: Id, level: usize, i: usize) -> Id {
    id.with(("menu-row", level, i))
}
