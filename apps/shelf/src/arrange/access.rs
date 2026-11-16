//! The arrange bar for screen readers. eframe 0.31 hands only its root window to AccessKit, and
//! the bar is another viewport, so the bar gets an adapter of its own (`accesskit_windows`, on
//! its window) fed a small tree built from what the bar drew: the bar, its line of help, a
//! radio button per piece (named with where the piece is) and the three buttons. Screen readers
//! can click the buttons and choose a piece; the arrow keys then move it as with the keyboard.

use accesskit::{Action, ActionHandler, ActionRequest, ActivationHandler, Node, NodeId, Role, Toggled, Tree, TreeUpdate};
use std::sync::{Arc, Mutex};

const ROOT: NodeId = NodeId(1);
const HELP: NodeId = NodeId(2);
const BUTTONS: u64 = 10;
const CHIPS: u64 = 100;

/// One control as drawn this frame, in the window's points.
pub struct Control {
    pub name: String,
    pub rect: egui::Rect,
    /// A piece's chip (with whether it is chosen), or a button.
    pub chip: Option<bool>,
    pub focused: bool,
}

/// What a screen reader asked for, taken on the next frame: the button or chip by its place in
/// the list given to `update`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Asked {
    Button(usize),
    Chip(usize),
}

struct Activation(Arc<Mutex<Option<TreeUpdate>>>);

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.0.lock().ok()?.clone()
    }
}

struct Actions {
    asked: Arc<Mutex<Vec<Asked>>>,
    ctx: egui::Context,
}

impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        if !matches!(request.action, Action::Click | Action::Focus) {
            return;
        }
        let id = request.target.0;
        let asked = if (CHIPS..CHIPS + 1000).contains(&id) {
            Asked::Chip((id - CHIPS) as usize)
        } else if (BUTTONS..BUTTONS + 10).contains(&id) {
            if request.action == Action::Focus {
                return;
            }
            Asked::Button((id - BUTTONS) as usize)
        } else {
            return;
        };
        if let Ok(mut a) = self.asked.lock() {
            a.push(asked);
        }
        self.ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

pub struct Access {
    adapter: accesskit_windows::SubclassingAdapter,
    last: Arc<Mutex<Option<TreeUpdate>>>,
    asked: Arc<Mutex<Vec<Asked>>>,
}

impl Access {
    /// Attach to the bar's window; before it is first shown.
    pub fn attach(hwnd: isize, ctx: &egui::Context) -> Self {
        let last = Arc::new(Mutex::new(None));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let adapter = accesskit_windows::SubclassingAdapter::new(
            accesskit_windows::HWND(hwnd as *mut _),
            Activation(last.clone()),
            Actions { asked: asked.clone(), ctx: ctx.clone() },
        );
        Self { adapter, last, asked }
    }

    /// What screen readers asked for since the last frame.
    pub fn take(&self) -> Vec<Asked> {
        self.asked.lock().map(|mut a| std::mem::take(&mut *a)).unwrap_or_default()
    }

    /// The tree for this frame: `buttons` and `chips` as drawn, `scale` pixels per point.
    pub fn update(&mut self, title: &str, help: &str, island: egui::Rect, buttons: &[Control], chips: &[Control], scale: f32) {
        let tree = build(title, help, island, buttons, chips, scale);
        if let Ok(mut l) = self.last.lock() {
            *l = Some(tree.clone());
        }
        if let Some(events) = self.adapter.update_if_active(|| tree) {
            events.raise();
        }
    }
}

fn bounds(r: egui::Rect, scale: f32) -> accesskit::Rect {
    accesskit::Rect { x0: (r.left() * scale) as f64, y0: (r.top() * scale) as f64, x1: (r.right() * scale) as f64, y1: (r.bottom() * scale) as f64 }
}

/// The whole tree (every update sends all of it; it is a handful of nodes).
pub fn build(title: &str, help: &str, island: egui::Rect, buttons: &[Control], chips: &[Control], scale: f32) -> TreeUpdate {
    let mut nodes = Vec::new();
    let mut children = Vec::new();
    let mut focus = ROOT;
    let mut help_node = Node::new(Role::Label);
    help_node.set_value(help);
    nodes.push((HELP, help_node));
    for (i, c) in chips.iter().enumerate() {
        let id = NodeId(CHIPS + i as u64);
        let mut n = Node::new(Role::RadioButton);
        n.set_label(c.name.clone());
        n.set_bounds(bounds(c.rect, scale));
        n.set_toggled(if c.chip == Some(true) { Toggled::True } else { Toggled::False });
        n.add_action(Action::Click);
        n.add_action(Action::Focus);
        if c.focused {
            focus = id;
        }
        children.push(id);
        nodes.push((id, n));
    }
    for (i, c) in buttons.iter().enumerate() {
        let id = NodeId(BUTTONS + i as u64);
        let mut n = Node::new(Role::Button);
        n.set_label(c.name.clone());
        n.set_bounds(bounds(c.rect, scale));
        n.add_action(Action::Click);
        n.add_action(Action::Focus);
        if c.focused {
            focus = id;
        }
        children.push(id);
        nodes.push((id, n));
    }
    children.push(HELP);
    let mut root = Node::new(Role::Window);
    root.set_label(title);
    root.set_bounds(bounds(island, scale));
    root.set_children(children);
    nodes.insert(0, (ROOT, root));
    TreeUpdate { nodes, tree: Some(Tree::new(ROOT)), focus }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2, Rect};

    #[test]
    fn every_control_is_named_and_the_focus_follows() {
        let r = Rect::from_min_size(pos2(10.0, 10.0), vec2(80.0, 30.0));
        let b = |name: &str, focused| Control { name: name.into(), rect: r, chip: None, focused };
        let chips = vec![Control { name: "Shelf: left edge, centred".into(), rect: r, chip: Some(true), focused: true }];
        let t = build("Arrange the desktop", "Drag…", r, &[b("Reset", false), b("Cancel", false), b("Done", false)], &chips, 1.5);
        assert_eq!(t.focus, NodeId(CHIPS));
        assert_eq!(t.nodes.len(), 6);
        for (id, n) in &t.nodes {
            if *id != HELP {
                assert!(n.label().is_some_and(|l| !l.is_empty()), "{id:?} unnamed");
            }
        }
        let root = &t.nodes[0].1;
        assert_eq!(root.children().len(), 5);
        let chip = t.nodes.iter().find(|(id, _)| *id == NodeId(CHIPS)).unwrap();
        assert_eq!(chip.1.role(), Role::RadioButton);
        assert_eq!(chip.1.toggled(), Some(Toggled::True));
        assert_eq!(chip.1.bounds().unwrap().x0, 15.0);
        let none = build("t", "h", r, &[b("Done", false)], &[], 1.0);
        assert_eq!(none.focus, ROOT);
    }
}
