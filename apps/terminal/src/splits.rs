//! A tab's panes: a tree of splits whose leaves are panes, the focused one, where each sits in
//! the tab's room, and the keyboard's ways between them. Pure, so the rules are tested without
//! shells or a window (as `tabs.rs` is for the tabs).
//!
//! Leaves are numbered in reading order (left before right, top before bottom); that order is
//! also the order panes are drawn in.

use egui::{pos2, Rect};

/// Which way a split lays its two halves out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    /// Side by side: the new pane goes to the right.
    Across,
    /// One above the other: the new pane goes below.
    Down,
}

/// A direction to move the focus or a divider in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// The smallest a pane may be, in points: a split that would leave one smaller is refused, and a
/// divider stops there.
pub const MIN_W: f32 = 120.0;
pub const MIN_H: f32 = 60.0;
/// Space between two panes (the window's gap between islands), which is also the divider.
pub const GAP: f32 = 12.0;

enum Node<T> {
    Leaf(T),
    Split { axis: Axis, ratio: f32, a: Box<Node<T>>, b: Box<Node<T>> },
}

impl<T> Node<T> {
    fn count(&self) -> usize {
        match self {
            Node::Leaf(_) => 1,
            Node::Split { a, b, .. } => a.count() + b.count(),
        }
    }

    fn leaves<'a>(&'a self, out: &mut Vec<&'a T>) {
        match self {
            Node::Leaf(t) => out.push(t),
            Node::Split { a, b, .. } => {
                a.leaves(out);
                b.leaves(out);
            }
        }
    }

    fn leaves_mut<'a>(&'a mut self, out: &mut Vec<&'a mut T>) {
        match self {
            Node::Leaf(t) => out.push(t),
            Node::Split { a, b, .. } => {
                a.leaves_mut(out);
                b.leaves_mut(out);
            }
        }
    }
}

/// Where a divider is: the gap between the halves of one split, the split's whole room (a drag
/// sets the ratio from it), and how to reach that split from the root (false = first half).
#[derive(Clone, Debug, PartialEq)]
pub struct Divider {
    pub gap: Rect,
    pub room: Rect,
    pub axis: Axis,
    pub path: Vec<bool>,
}

/// Each pane's rectangle (in leaf order) and the dividers between them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub panes: Vec<Rect>,
    pub dividers: Vec<Divider>,
}

pub struct Panes<T> {
    root: Option<Node<T>>,
    focus: usize,
}

impl<T> Panes<T> {
    pub fn new(first: T) -> Self {
        Self { root: Some(Node::Leaf(first)), focus: 0 }
    }

    fn root(&self) -> &Node<T> {
        self.root.as_ref().expect("a tab always has a pane")
    }

    pub fn len(&self) -> usize {
        self.root().count()
    }

    pub fn focus(&self) -> usize {
        self.focus
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        let mut v = Vec::new();
        self.root().leaves(&mut v);
        v.into_iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        let mut v = Vec::new();
        self.root.as_mut().expect("a tab always has a pane").leaves_mut(&mut v);
        v.into_iter()
    }

    pub fn get(&self, i: usize) -> Option<&T> {
        self.iter().nth(i)
    }

    pub fn focused(&self) -> &T {
        self.get(self.focus).expect("the focus is a pane")
    }

    /// Focus pane `i` (ignored when there is none).
    pub fn set_focus(&mut self, i: usize) {
        if i < self.len() {
            self.focus = i;
        }
    }

    /// The axis that suits splitting a pane of this size (Windows Terminal's "auto"): across a
    /// wide one, down a tall one.
    pub fn auto_axis(rect: Rect) -> Axis {
        if rect.width() >= rect.height() * 1.6 { Axis::Across } else { Axis::Down }
    }

    /// Whether the focused pane, at `rect`, has room to be split along `axis`.
    pub fn can_split(rect: Rect, axis: Axis) -> bool {
        match axis {
            Axis::Across => rect.width() >= MIN_W * 2.0 + GAP,
            Axis::Down => rect.height() >= MIN_H * 2.0 + GAP,
        }
    }

    /// Split the focused pane in two along `axis`; `new` takes the second half and the focus.
    pub fn split(&mut self, axis: Axis, new: T) {
        let target = self.focus;
        let root = self.root.take().expect("a tab always has a pane");
        let mut new = Some(new);
        self.root = Some(split_at(root, target, &mut 0, axis, &mut new));
        self.focus = target + 1;
    }

    /// Close pane `i`; the other half of its split takes its room. Returns it, or None when it
    /// is the only pane (the tab closes instead). Closing the focused pane focuses the pane that
    /// took its place; closing another keeps the same pane focused.
    pub fn close(&mut self, i: usize) -> Option<T> {
        let n = self.len();
        if i >= n || n == 1 {
            return None;
        }
        let root = self.root.take().expect("a tab always has a pane");
        let mut out = None;
        let mut took = 0;
        self.root = Some(close_at(root, i, &mut 0, &mut out, &mut took));
        if i == self.focus {
            self.focus = took;
        } else if i < self.focus {
            self.focus -= 1;
        }
        out
    }

    /// Close every pane `keep` refuses. Returns whether any is left (the tab closes if not).
    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) -> bool {
        loop {
            let gone = self.iter().position(|t| !keep(t));
            match gone {
                None => return true,
                Some(_) if self.len() == 1 => return false,
                Some(i) => {
                    self.close(i);
                }
            }
        }
    }

    /// Where each pane goes in `room`, and the dividers.
    pub fn layout(&self, room: Rect) -> Layout {
        let mut out = Layout::default();
        place(self.root(), room, &mut Vec::new(), &mut out);
        out
    }

    /// Set the ratio of the split at `path` from a point in its room (a divider dragged),
    /// keeping both halves at least the smallest size.
    pub fn drag(&mut self, d: &Divider, at: egui::Pos2) {
        let (start, len, min, p) = match d.axis {
            Axis::Across => (d.room.left(), d.room.width(), MIN_W, at.x),
            Axis::Down => (d.room.top(), d.room.height(), MIN_H, at.y),
        };
        let usable = len - GAP;
        if usable <= min * 2.0 {
            return;
        }
        let ratio = ((p - start - GAP / 2.0) / usable).clamp(min / usable, 1.0 - min / usable);
        if let Some(Node::Split { ratio: r, .. }) = node_at(self.root.as_mut().unwrap(), &d.path) {
            *r = ratio;
        }
    }

    /// Move the divider nearest the focused pane on its `dir` side by `step` points (Alt+Shift+
    /// arrow). `layout` is this frame's.
    pub fn nudge(&mut self, layout: &Layout, dir: Dir, step: f32) {
        let Some(&me) = layout.panes.get(self.focus) else { return };
        let axis = if matches!(dir, Dir::Left | Dir::Right) { Axis::Across } else { Axis::Down };
        // The innermost divider along that axis that touches the pane on either side.
        let touching = layout.dividers.iter().filter(|d| d.axis == axis && d.room.contains_rect(me)).max_by_key(|d| d.path.len());
        let Some(d) = touching.cloned() else { return };
        let delta = match dir {
            Dir::Left | Dir::Up => -step,
            Dir::Right | Dir::Down => step,
        };
        let at = match axis {
            Axis::Across => pos2(d.gap.center().x + delta, d.gap.center().y),
            Axis::Down => pos2(d.gap.center().x, d.gap.center().y + delta),
        };
        self.drag(&d, at);
    }
}

/// The pane next to pane `from` in direction `dir` (Alt+arrow), given each pane's rectangle:
/// of those wholly on that side and overlapping it across, the nearest, then the one overlapping
/// most. None at the edge.
pub fn neighbour(panes: &[Rect], from: usize, dir: Dir) -> Option<usize> {
    let me = *panes.get(from)?;
    let eps = 0.5;
    let mut best: Option<(usize, f32, f32)> = None;
    for (i, r) in panes.iter().enumerate() {
        if i == from {
            continue;
        }
        let (beyond, dist, overlap) = match dir {
            Dir::Left => (r.right() <= me.left() + eps, me.left() - r.right(), span(r.y_range(), me.y_range())),
            Dir::Right => (r.left() >= me.right() - eps, r.left() - me.right(), span(r.y_range(), me.y_range())),
            Dir::Up => (r.bottom() <= me.top() + eps, me.top() - r.bottom(), span(r.x_range(), me.x_range())),
            Dir::Down => (r.top() >= me.bottom() - eps, r.top() - me.bottom(), span(r.x_range(), me.x_range())),
        };
        if !beyond || overlap <= 0.0 {
            continue;
        }
        let better = match best {
            None => true,
            Some((_, d, o)) => dist < d - eps || ((dist - d).abs() <= eps && overlap > o),
        };
        if better {
            best = Some((i, dist, overlap));
        }
    }
    best.map(|b| b.0)
}

fn span(a: egui::Rangef, b: egui::Rangef) -> f32 {
    a.max.min(b.max) - a.min.max(b.min)
}

fn split_at<T>(node: Node<T>, target: usize, seen: &mut usize, axis: Axis, new: &mut Option<T>) -> Node<T> {
    match node {
        Node::Leaf(t) => {
            let here = *seen;
            *seen += 1;
            if here == target {
                if let Some(n) = new.take() {
                    return Node::Split { axis, ratio: 0.5, a: Box::new(Node::Leaf(t)), b: Box::new(Node::Leaf(n)) };
                }
            }
            Node::Leaf(t)
        }
        Node::Split { axis: ax, ratio, a, b } => {
            let a = split_at(*a, target, seen, axis, new);
            let b = split_at(*b, target, seen, axis, new);
            Node::Split { axis: ax, ratio, a: Box::new(a), b: Box::new(b) }
        }
    }
}

/// Remove leaf `target`; `took` is set to the index (after removal) of the pane that takes its
/// room: the first of what followed it, or the last of what came before.
fn close_at<T>(node: Node<T>, target: usize, seen: &mut usize, out: &mut Option<T>, took: &mut usize) -> Node<T> {
    match node {
        Node::Leaf(t) => {
            *seen += 1;
            Node::Leaf(t)
        }
        Node::Split { axis, ratio, a, b } => {
            let start = *seen;
            let (na, nb) = (a.count(), b.count());
            if na == 1 && start == target {
                // The first half is the one closing: the second takes the room.
                if let Node::Leaf(t) = *a {
                    *out = Some(t);
                }
                *took = start;
                *seen += 1 + nb;
                return *b;
            }
            if nb == 1 && start + na == target {
                if let Node::Leaf(t) = *b {
                    *out = Some(t);
                }
                *took = start + na - 1;
                *seen += na + 1;
                return *a;
            }
            let a = close_at(*a, target, seen, out, took);
            let b = close_at(*b, target, seen, out, took);
            Node::Split { axis, ratio, a: Box::new(a), b: Box::new(b) }
        }
    }
}

fn place<T>(node: &Node<T>, room: Rect, path: &mut Vec<bool>, out: &mut Layout) {
    match node {
        Node::Leaf(_) => out.panes.push(room),
        Node::Split { axis, ratio, a, b } => {
            let (ra, gap, rb) = halves(room, *axis, *ratio);
            out.dividers.push(Divider { gap, room, axis: *axis, path: path.clone() });
            path.push(false);
            place(a, ra, path, out);
            path.pop();
            path.push(true);
            place(b, rb, path, out);
            path.pop();
        }
    }
}

/// A room cut in two along `axis` at `ratio`, the gap between, on whole points.
fn halves(room: Rect, axis: Axis, ratio: f32) -> (Rect, Rect, Rect) {
    match axis {
        Axis::Across => {
            let usable = (room.width() - GAP).max(0.0);
            let x = (room.left() + usable * ratio).round();
            (
                Rect::from_min_max(room.min, pos2(x, room.bottom())),
                Rect::from_min_max(pos2(x, room.top()), pos2(x + GAP, room.bottom())),
                Rect::from_min_max(pos2(x + GAP, room.top()), room.max),
            )
        }
        Axis::Down => {
            let usable = (room.height() - GAP).max(0.0);
            let y = (room.top() + usable * ratio).round();
            (
                Rect::from_min_max(room.min, pos2(room.right(), y)),
                Rect::from_min_max(pos2(room.left(), y), pos2(room.right(), y + GAP)),
                Rect::from_min_max(pos2(room.left(), y + GAP), room.max),
            )
        }
    }
}

fn node_at<'a, T>(node: &'a mut Node<T>, path: &[bool]) -> Option<&'a mut Node<T>> {
    match path.split_first() {
        None => Some(node),
        Some((&second, rest)) => match node {
            Node::Split { a, b, .. } => node_at(if second { b } else { a }, rest),
            Node::Leaf(_) => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(812.0, 412.0))
    }

    fn names(p: &Panes<&'static str>) -> Vec<&'static str> {
        p.iter().copied().collect()
    }

    #[test]
    fn a_split_puts_the_new_pane_after_and_focuses_it() {
        let mut p = Panes::new("a");
        p.split(Axis::Across, "b");
        assert_eq!(names(&p), ["a", "b"]);
        assert_eq!(*p.focused(), "b");
        p.set_focus(0);
        p.split(Axis::Down, "c");
        assert_eq!(names(&p), ["a", "c", "b"]);
        assert_eq!(p.focus(), 1);
    }

    #[test]
    fn halves_share_the_room_with_a_gap_and_line_up() {
        let mut p = Panes::new(1);
        p.split(Axis::Across, 2);
        let l = p.layout(room());
        assert_eq!(l.panes.len(), 2);
        assert_eq!(l.panes[0].width(), 400.0);
        assert_eq!(l.panes[1].left() - l.panes[0].right(), GAP);
        assert_eq!(l.panes[1].right(), room().right());
        assert_eq!((l.panes[0].top(), l.panes[1].bottom()), (0.0, 412.0));
        assert_eq!(l.dividers.len(), 1);
        assert_eq!(l.dividers[0].gap.width(), GAP);
    }

    #[test]
    fn closing_gives_the_room_to_the_other_half() {
        let mut p = Panes::new("a");
        p.split(Axis::Across, "b");
        p.split(Axis::Down, "c");
        // a | (b / c), c focused.
        assert_eq!(p.close(2), Some("c"));
        assert_eq!(names(&p), ["a", "b"]);
        assert_eq!(*p.focused(), "b", "the pane that took its place");
        let l = p.layout(room());
        assert_eq!(l.panes[1].height(), 412.0);
        // Closing the first half of a split focuses the second.
        p.set_focus(0);
        assert_eq!(p.close(0), Some("a"));
        assert_eq!(*p.focused(), "b");
        assert_eq!(p.layout(room()).panes[0], room());
        assert_eq!(p.close(0), None, "the last pane stays");
    }

    #[test]
    fn closing_another_pane_keeps_the_focus() {
        let mut p = Panes::new("a");
        p.split(Axis::Across, "b");
        p.split(Axis::Across, "c");
        assert_eq!(*p.focused(), "c");
        p.close(0);
        assert_eq!(*p.focused(), "c");
        assert_eq!(names(&p), ["b", "c"]);
    }

    #[test]
    fn retain_closes_exited_panes() {
        let mut p = Panes::new(1);
        p.split(Axis::Across, 2);
        p.split(Axis::Down, 3);
        assert!(p.retain(|&n| n != 2));
        assert_eq!(p.iter().copied().collect::<Vec<_>>(), [1, 3]);
        assert!(!p.retain(|_| false));
    }

    #[test]
    fn the_focus_moves_to_the_pane_beside() {
        // a | (b / c)
        let mut p = Panes::new("a");
        p.split(Axis::Across, "b");
        p.split(Axis::Down, "c");
        let l = p.layout(room());
        assert_eq!(neighbour(&l.panes, 0, Dir::Right), Some(1), "the upper of two, which overlaps as much, comes first");
        assert_eq!(neighbour(&l.panes, 2, Dir::Left), Some(0));
        assert_eq!(neighbour(&l.panes, 1, Dir::Down), Some(2));
        assert_eq!(neighbour(&l.panes, 2, Dir::Up), Some(1));
        assert_eq!(neighbour(&l.panes, 0, Dir::Left), None);
        assert_eq!(neighbour(&l.panes, 0, Dir::Up), None);
    }

    #[test]
    fn small_panes_are_not_split_and_dividers_stop_short() {
        let narrow = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(200.0, 400.0));
        assert!(!Panes::<()>::can_split(narrow, Axis::Across));
        assert!(Panes::<()>::can_split(narrow, Axis::Down));
        assert_eq!(Panes::<()>::auto_axis(room()), Axis::Across);
        assert_eq!(Panes::<()>::auto_axis(narrow), Axis::Down);
        let mut p = Panes::new(1);
        p.split(Axis::Across, 2);
        let d = p.layout(room()).dividers[0].clone();
        p.drag(&d, pos2(-500.0, 10.0));
        assert_eq!(p.layout(room()).panes[0].width(), MIN_W);
        p.drag(&d, pos2(5000.0, 10.0));
        assert_eq!(p.layout(room()).panes[1].width(), MIN_W);
        p.drag(&d, pos2(306.0, 10.0));
        assert_eq!(p.layout(room()).panes[0].width(), 300.0);
    }

    #[test]
    fn nudging_moves_the_divider_beside_the_focus() {
        let mut p = Panes::new(1);
        p.split(Axis::Across, 2);
        let l = p.layout(room());
        p.nudge(&l, Dir::Left, 40.0);
        assert_eq!(p.layout(room()).panes[0].width(), 360.0);
        let l = p.layout(room());
        p.nudge(&l, Dir::Up, 40.0);
        assert_eq!(p.layout(room()), l, "no divider that way");
    }
}
