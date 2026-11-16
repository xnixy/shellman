//! Arrange mode: "Edit desktop…" lets the shelf and each widget be dragged to a new place on its
//! monitor, snapping to the work area's margins and middle and to each other (`place`), with a
//! small bar (`bar`) holding Reset, Cancel and Done and naming every piece for the keyboard and
//! screen readers.
//!
//! The pieces are the real windows, moved as they are dragged: no overlay (a window the size of
//! a monitor is not allowed, see PLAN "The right-click menu"). While arranging, every piece's
//! window is raised above the apps, fans stay shut, the drop zone and the widgets' buttons do
//! nothing, and each piece wears an accent ring. Positions are kept in a working copy here and
//! written to `desktop.json` (`arrange`, and `shelf.edge` when the shelf changed sides) on Done;
//! Cancel or Esc puts back what was there.
//!
//! The state lives on the window thread (thread-local), because the shelves (immediate
//! viewports, drawn in the shelf's frame) and the widgets (deferred viewports, drawn on their own)
//! both read it. Every borrow is short and never held across a call back into either.

mod access;
pub mod bar;
pub mod place;

use crate::displays;
use egui::{Pos2, Rect};
use shellman_core::monitors::Monitor;
use shellman_core::settings::desktop::{Arrangement, DesktopSettings, Edge, Pin, ShelfOn};
use std::cell::RefCell;
use std::collections::BTreeMap;
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetSystemMetrics, SM_SWAPBUTTON};

/// One thing that can be moved.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Item {
    Shelf,
    Today,
    Playing,
}

impl Item {
    pub fn label(self) -> &'static str {
        match self {
            Item::Shelf => "Shelf",
            Item::Today => "Today",
            Item::Playing => "Now playing",
        }
    }
}

/// A piece as last drawn: where it is on its monitor (monitor points) and the work area there.
#[derive(Clone, Debug)]
pub struct Piece {
    pub key: String,
    pub item: Item,
    pub display: String,
    pub rect: Rect,
    pub work: Rect,
}

struct Drag {
    key: String,
    item: Item,
    /// The pointer when the drag started (screen pixels), and the piece then.
    cursor: (i32, i32),
    from: Rect,
    scale: f32,
}

struct Session {
    working: BTreeMap<String, Arrangement>,
    edge: Edge,
    original: (BTreeMap<String, Arrangement>, Edge),
    drag: Option<Drag>,
    pieces: BTreeMap<(String, Item), Piece>,
    selected: Option<(String, Item)>,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Session) -> R) -> Option<R> {
    SESSION.with(|s| s.try_borrow_mut().ok()?.as_mut().map(f))
}

/// Whether arrange mode is on.
pub fn active() -> bool {
    SESSION.with(|s| s.try_borrow().map(|s| s.is_some()).unwrap_or(false))
}

/// Start arranging from the saved settings.
pub fn begin(settings: &DesktopSettings) {
    SESSION.with(|s| {
        if let Ok(mut s) = s.try_borrow_mut() {
            if s.is_none() {
                *s = Some(Session {
                    working: settings.arrange.clone(),
                    edge: settings.shelf.edge,
                    original: (settings.arrange.clone(), settings.shelf.edge),
                    drag: None,
                    pieces: BTreeMap::new(),
                    selected: None,
                });
            }
        }
    });
}

/// Stop arranging; what was arranged (the arrangements and the shelf's edge), for saving.
pub fn end() -> Option<(BTreeMap<String, Arrangement>, Edge)> {
    SESSION.with(|s| s.try_borrow_mut().ok()?.take().map(|s| (s.working, s.edge)))
}

/// Stop arranging and keep nothing: what was there before.
pub fn cancel() -> Option<(BTreeMap<String, Arrangement>, Edge)> {
    SESSION.with(|s| s.try_borrow_mut().ok()?.take().map(|s| s.original))
}

/// Everything back to the design's places (until Done).
pub fn reset() {
    with(|s| {
        s.working.clear();
        s.edge = Edge::default();
        s.drag = None;
    });
}

/// The arrangement in force for the monitor `key`: the working copy while arranging, else the
/// saved one (or the design's when there is none).
pub fn arrangement(settings: &DesktopSettings, key: &str) -> Arrangement {
    with(|s| s.working.get(key).copied())
        .unwrap_or_else(|| settings.arrange.get(key).copied())
        .unwrap_or_default()
}

/// The shelf's edge in force.
pub fn edge(settings: &DesktopSettings) -> Edge {
    with(|s| s.edge).unwrap_or(settings.shelf.edge)
}

/// Change the working arrangement of monitor `key`.
pub fn edit(key: &str, f: impl FnOnce(&mut Arrangement)) {
    with(|s| {
        let a = s.working.entry(key.to_owned()).or_default();
        f(a);
        if *a == Arrangement::default() {
            s.working.remove(key);
        }
    });
}

pub fn set_edge(edge: Edge) {
    with(|s| s.edge = edge);
}

/// Say where a piece was drawn.
pub fn report(piece: Piece) {
    with(|s| {
        s.pieces.insert((piece.key.clone(), piece.item), piece);
    });
}

/// Forget the pieces of monitors that are gone (the monitors changed).
pub fn forget_pieces() {
    with(|s| {
        s.pieces.clear();
        s.drag = None;
    });
}

/// Every piece as last drawn, by monitor (the primary first) then item.
pub fn pieces() -> Vec<Piece> {
    let mut list: Vec<Piece> = with(|s| s.pieces.values().cloned().collect()).unwrap_or_default();
    list.sort_by(|a, b| (a.key != place::PRIMARY, &a.key, a.item).cmp(&(b.key != place::PRIMARY, &b.key, b.item)));
    list
}

/// The other pieces on monitor `key`, for snapping to.
pub fn others(key: &str, item: Item) -> Vec<Rect> {
    with(|s| s.pieces.values().filter(|p| p.key == key && p.item != item).map(|p| p.rect).collect()).unwrap_or_default()
}

pub fn select(key: &str, item: Item) {
    with(|s| s.selected = Some((key.to_owned(), item)));
}

/// Whether the piece wears the selected ring.
pub fn is_selected(key: &str, item: Item) -> bool {
    with(|s| s.selected.as_ref().is_some_and(|(k, i)| k == key && *i == item)).unwrap_or(false)
}

/// The pointer, in screen pixels.
pub fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

/// The pointer in monitor points of `m`.
pub fn cursor_on(m: &Monitor) -> Pos2 {
    let (x, y) = cursor();
    let s = if m.scale > 0.0 { m.scale } else { 1.0 };
    Pos2::new((x - m.rect.left) as f32 / s, (y - m.rect.top) as f32 / s)
}

/// Whether the primary mouse button is down now, wherever the pointer is (the piece's window
/// moves under it, so its own input is not to be trusted for this).
fn button_down() -> bool {
    let swapped = unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0;
    let vk = if swapped { VK_RBUTTON } else { VK_LBUTTON };
    unsafe { GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 }
}

/// A drag of the piece starts: it was at `from` (monitor points, `scale` pixels per point).
pub fn start_drag(key: &str, item: Item, from: Rect, scale: f32) {
    let at = cursor();
    with(|s| {
        s.selected = Some((key.to_owned(), item));
        s.drag = Some(Drag { key: key.to_owned(), item, cursor: at, from, scale });
    });
}

/// Where the piece being dragged would be now, unsnapped (monitor points); None when this piece
/// is not being dragged. The drag ends when the button is let go.
pub fn dragged(key: &str, item: Item) -> Option<Rect> {
    let down = button_down();
    with(|s| {
        let d = s.drag.as_ref().filter(|d| d.key == key && d.item == item)?;
        let (x, y) = cursor();
        let s_ = if d.scale > 0.0 { d.scale } else { 1.0 };
        let moved = d.from.translate(egui::vec2((x - d.cursor.0) as f32 / s_, (y - d.cursor.1) as f32 / s_));
        if !down {
            s.drag = None;
        }
        Some(moved)
    })
    .flatten()
}

/// Whether any drag is on (frames keep coming while it is).
pub fn dragging() -> bool {
    with(|s| s.drag.is_some()).unwrap_or(false)
}

/// The shelf's rectangle on monitor `m` (monitor points) with the arrangement `a`, if this
/// monitor has a shelf: the setting gives it one, and the monitor is fit for its window.
pub fn shelf_on(settings: &DesktopSettings, m: &Monitor, edge: Edge, pin: Option<Pin>) -> Option<Rect> {
    if !settings.shelf_shown() || !(settings.shelf.on == ShelfOn::All || m.primary) {
        return None;
    }
    let stacks = settings.stacks().len();
    displays::place(m, edge, stacks, pin)?;
    Some(place::shelf_rect(place::work(m), edge, stacks, pin))
}

/// Move the piece by `delta` points (the keyboard): kept inside, no snapping, except that Now
/// playing put right under Today is tied to it again.
pub fn nudge(p: &Piece, delta: egui::Vec2) {
    match p.item {
        Item::Shelf => {
            let top = place::clamp(p.rect.translate(egui::vec2(0.0, delta.y)), p.work).top();
            let pin = place::pin_for(top, p.work.top(), p.work.bottom(), p.rect.height());
            edit(&p.key, |a| a.shelf = Some(pin));
        }
        Item::Today | Item::Playing => {
            let r = place::clamp(p.rect.translate(delta), p.work);
            store_widget(&p.key, p.item, r, p.work);
        }
    }
}

/// Keep a widget's new place: its spot, or for Now playing right under Today, tied to Today.
pub fn store_widget(key: &str, item: Item, r: Rect, work: Rect) {
    let today = with(|s| s.pieces.get(&(key.to_owned(), Item::Today)).map(|p| p.rect)).flatten();
    let spot = place::spot_for(r, work);
    edit(key, |a| match item {
        Item::Today => a.today = Some(spot),
        _ => a.playing = if today.is_some_and(|t| place::is_under(r, t)) { None } else { Some(spot) },
    });
}

/// The piece's ring while arranging: an accent line inside its island, heavier when selected.
pub fn ring(painter: &egui::Painter, r: Rect, radius: f32, selected: bool) {
    let c = shellman_ui::theme::colors();
    let w: f32 = if selected { 3.0 } else { 2.0 };
    painter.rect_stroke(r, radius, egui::Stroke::new(w, c.accent_fill), egui::StrokeKind::Inside);
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    #[test]
    fn a_session_edits_a_copy_and_cancel_puts_it_back() {
        let mut s = DesktopSettings::default();
        s.arrange.insert(place::PRIMARY.into(), Arrangement { shelf: Some(Pin::default()), ..Default::default() });
        begin(&s);
        assert!(active());
        edit("X", |a| a.today = Some(place::default_today()));
        set_edge(Edge::Right);
        assert_eq!(edge(&s), Edge::Right);
        assert!(arrangement(&s, "X").today.is_some());
        // An edit back to the design's place leaves no entry.
        edit("X", |a| a.today = None);
        let (back, e) = cancel().unwrap();
        assert_eq!((back, e), (s.arrange.clone(), Edge::Left));
        assert!(!active());
        assert_eq!(edge(&s), Edge::Left);
        begin(&s);
        reset();
        let (done, e) = end().unwrap();
        assert!(done.is_empty() && e == Edge::Left);
    }

    #[test]
    fn now_playing_dropped_under_today_is_tied_to_it() {
        let s = DesktopSettings::default();
        begin(&s);
        let work = Rect::from_min_max(pos2(0.0, 40.0), pos2(2560.0, 1372.0));
        let t = Rect::from_min_size(pos2(2000.0, 56.0), vec2(372.0, 250.0));
        report(Piece { key: "K".into(), item: Item::Today, display: "d".into(), rect: t, work });
        store_widget("K", Item::Playing, Rect::from_min_size(pos2(500.0, 500.0), vec2(372.0, 78.0)), work);
        assert!(arrangement(&s, "K").playing.is_some());
        store_widget("K", Item::Playing, Rect::from_min_size(pos2(t.left(), t.bottom() + place::GAP), vec2(372.0, 78.0)), work);
        assert_eq!(arrangement(&s, "K").playing, None);
        let p = Piece { key: "K".into(), item: Item::Shelf, display: "d".into(), rect: Rect::from_min_size(pos2(20.0, 400.0), vec2(104.0, 400.0)), work };
        nudge(&p, vec2(30.0, -10.0));
        let pin = arrangement(&s, "K").shelf.unwrap();
        assert_eq!(place::start(pin, work.top(), work.bottom(), 400.0), 390.0);
        assert_eq!(others("K", Item::Playing), vec![t]);
        let _ = end();
    }
}
