//! Screen readers for the windows eframe does not hand to AccessKit.
//!
//! eframe 0.31 gives only its root viewport an AccessKit adapter; every other viewport (the
//! bar's top bars and docks, its previews, notifications and calendar, the shelf's widgets, the
//! popup menu) drops egui's tree, so UI Automation sees an empty window. Such a window calls
//! [`publish`] at the end of its frame: it gets an adapter of its own (`accesskit_windows` on its
//! HWND, found by the viewport's title), fed a tree of what its painted items said about
//! themselves through [`describe`] (which `theme::speak` and the shared controls use).
//!
//! Names cost nothing until a screen reader asks: an item is only described once that window's
//! adapter has been asked for its first tree (`listening`), and a click from a screen reader is
//! played to the window as a mouse click on the item's middle.
//!
//! The root viewport keeps eframe's own adapter; never attach a second one to a window.

use accesskit::{Action, ActionHandler, ActionRequest, ActivationHandler, Node, NodeId, Role, Toggled, Tree, TreeUpdate};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const ROOT: NodeId = NodeId(0);

/// One painted item as a screen reader is told about it.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: u64,
    pub rect: egui::Rect,
    pub info: egui::WidgetInfo,
    pub focused: bool,
}

struct Bridge {
    hwnd: isize,
    adapter: accesskit_windows::SubclassingAdapter,
    active: Arc<AtomicBool>,
    last: Arc<Mutex<Option<TreeUpdate>>>,
    clicks: Arc<Mutex<Vec<NodeId>>>,
    /// The items of the last tree, for a click's place.
    items: Vec<Item>,
}

thread_local! {
    /// Every viewport's bridge (all eframe viewports live on the UI thread).
    static BRIDGES: RefCell<HashMap<egui::ViewportId, Bridge>> = RefCell::new(HashMap::new());
    /// What this frame's items said, by viewport.
    static ITEMS: RefCell<HashMap<egui::ViewportId, Vec<Item>>> = RefCell::new(HashMap::new());
}

/// Whether a screen reader has asked this (non-root) viewport for its tree.
fn listening(vp: egui::ViewportId) -> bool {
    vp != egui::ViewportId::ROOT && BRIDGES.with(|b| b.borrow().get(&vp).is_some_and(|b| b.active.load(Ordering::Relaxed)))
}

/// Say what `resp` is: egui's own `widget_info` (the root window's tree, and focus and click
/// announcements), and in a window with a bridge that a screen reader listens to, an item of its
/// tree. `info` may be costly (a format, a Windows call), it runs only when needed, but it runs
/// inside egui's lock for the root's tree: it must not touch the `Context` or a `Ui`.
pub fn describe(resp: &egui::Response, info: impl Fn() -> egui::WidgetInfo) {
    describe_focused(resp, resp.has_focus(), info);
}

/// [`describe`] for an item that holds the screen reader's focus by its own rule rather than
/// egui's (the popup menu's lit row, which its own keys move).
pub fn describe_focused(resp: &egui::Response, focused: bool, info: impl Fn() -> egui::WidgetInfo) {
    resp.widget_info(&info);
    let vp = resp.ctx.viewport_id();
    if listening(vp) {
        let item = Item { id: resp.id.value(), rect: resp.rect, info: info(), focused };
        ITEMS.with(|i| i.borrow_mut().entry(vp).or_default().push(item));
    }
}

struct Activation {
    active: Arc<AtomicBool>,
    last: Arc<Mutex<Option<TreeUpdate>>>,
    ctx: egui::Context,
    vp: egui::ViewportId,
}

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        // From now on the window describes its items; the next frame sends them.
        if !self.active.swap(true, Ordering::Relaxed) {
            self.ctx.request_repaint_of(self.vp);
        }
        let last = self.last.lock().ok()?.clone();
        Some(last.unwrap_or_else(|| build("", egui::Rect::NOTHING, &[], 1.0)))
    }
}

struct Actions {
    clicks: Arc<Mutex<Vec<NodeId>>>,
    ctx: egui::Context,
    vp: egui::ViewportId,
}

impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        if matches!(request.action, Action::Click) && request.target != ROOT {
            if let Ok(mut c) = self.clicks.lock() {
                c.push(request.target);
            }
            self.ctx.request_repaint_of(self.vp);
        }
    }
}

/// End a non-root viewport's frame: attach its bridge (once its window exists), hand a screen
/// reader the items described this frame, and play the clicks it asked for. Free while nobody
/// listens. Does nothing for the root viewport, which has eframe's adapter.
pub fn publish(ctx: &egui::Context) {
    let vp = ctx.viewport_id();
    if vp == egui::ViewportId::ROOT {
        return;
    }
    let items = ITEMS.with(|i| i.borrow_mut().remove(&vp)).unwrap_or_default();
    let attached = BRIDGES.with(|b| b.borrow().get(&vp).map(|b| b.hwnd));
    let alive = attached.is_some_and(|h| win::is_window(h));
    if !alive {
        // A window made again (or never seen): a bridge for the one there now. The old one is
        // forgotten, not dropped: dropping restores the window procedure, which panics if
        // Windows refuses (and the window is gone anyway).
        if let Some(old) = BRIDGES.with(|b| b.borrow_mut().remove(&vp)) {
            std::mem::forget(old);
        }
        let Some(title) = ctx.input(|i| i.viewport().title.clone()) else { return };
        let Some(hwnd) = win::own_window(&title) else { return };
        let active = Arc::new(AtomicBool::new(false));
        let last = Arc::new(Mutex::new(None));
        let clicks = Arc::new(Mutex::new(Vec::new()));
        let adapter = accesskit_windows::SubclassingAdapter::new(
            accesskit_windows::HWND(hwnd as *mut _),
            Activation { active: active.clone(), last: last.clone(), ctx: ctx.clone(), vp },
            Actions { clicks: clicks.clone(), ctx: ctx.clone(), vp },
        );
        BRIDGES.with(|b| b.borrow_mut().insert(vp, Bridge { hwnd, adapter, active, last, clicks, items: Vec::new() }));
        return;
    }
    let title = ctx.input(|i| i.viewport().title.clone()).unwrap_or_default();
    let screen = ctx.screen_rect();
    let ppp = ctx.pixels_per_point();
    BRIDGES.with(|b| {
        let mut b = b.borrow_mut();
        let Some(bridge) = b.get_mut(&vp) else { return };
        if !bridge.active.load(Ordering::Relaxed) {
            return;
        }
        // Clicks asked for last frame land on the items as they were then.
        let clicks = bridge.clicks.lock().map(|mut c| std::mem::take(&mut *c)).unwrap_or_default();
        for target in clicks {
            if let Some(item) = bridge.items.iter().find(|i| NodeId(i.id) == target) {
                win::click(bridge.hwnd, item.rect.center(), ppp);
            }
        }
        let tree = build(&title, screen, &items, ppp);
        if bridge.last.lock().is_ok_and(|l| l.as_ref() == Some(&tree)) {
            return;
        }
        if let Ok(mut l) = bridge.last.lock() {
            *l = Some(tree.clone());
        }
        bridge.items = items;
        if let Some(events) = bridge.adapter.update_if_active(|| tree) {
            events.raise();
        }
    });
}

fn bounds(r: egui::Rect, scale: f32) -> accesskit::Rect {
    accesskit::Rect { x0: (r.left() * scale) as f64, y0: (r.top() * scale) as f64, x1: (r.right() * scale) as f64, y1: (r.bottom() * scale) as f64 }
}

/// The node for one item: egui's widget types as AccessKit roles, the way egui-winit maps them
/// for the root window (a "selected" button is pressed; a selectable label is a list item).
fn node(item: &Item, scale: f32) -> Node {
    use egui::WidgetType as W;
    let info = &item.info;
    let role = match info.typ {
        W::Label => Role::Label,
        W::Link => Role::Link,
        W::TextEdit => Role::TextInput,
        W::Button | W::ImageButton | W::CollapsingHeader => Role::Button,
        W::Checkbox => Role::CheckBox,
        W::RadioButton | W::RadioGroup => Role::RadioButton,
        W::SelectableLabel => Role::ListItem,
        W::ComboBox => Role::ComboBox,
        W::Slider => Role::Slider,
        W::DragValue => Role::SpinButton,
        W::ColorButton => Role::ColorWell,
        W::ProgressIndicator => Role::ProgressIndicator,
        W::Image => Role::Image,
        W::Window => Role::Window,
        W::Other => Role::Unknown,
    };
    let mut n = Node::new(role);
    if let Some(label) = info.label.as_ref().filter(|l| !l.is_empty()) {
        // A label's text is its value (UI Automation reads a text element's name from it).
        if role == Role::Label {
            n.set_value(label.clone());
        } else {
            n.set_label(label.clone());
        }
    }
    n.set_bounds(bounds(item.rect, scale));
    if let Some(text) = &info.current_text_value {
        n.set_value(text.clone());
    }
    if let Some(v) = info.value {
        n.set_numeric_value(v);
        n.set_min_numeric_value(0.0);
        n.set_max_numeric_value(100.0);
    }
    if let Some(selected) = info.selected {
        match role {
            Role::ListItem => n.set_selected(selected),
            Role::CheckBox | Role::RadioButton | Role::Button => n.set_toggled(if selected { Toggled::True } else { Toggled::False }),
            _ => {}
        }
    }
    if !info.enabled {
        n.set_disabled();
    } else if !matches!(role, Role::Label | Role::Unknown) {
        n.add_action(Action::Click);
        n.add_action(Action::Focus);
    }
    n
}

/// The whole tree for a window: one node per item under the window's (every update sends all
/// of it; a surface has tens of items at most). The focus is the focused item, else the window.
pub fn build(title: &str, screen: egui::Rect, items: &[Item], scale: f32) -> TreeUpdate {
    let mut nodes = Vec::with_capacity(items.len() + 1);
    let mut children = Vec::with_capacity(items.len());
    let mut focus = ROOT;
    for item in items {
        // Id 0 is the window's; an item that hashed to it (or came twice) is skipped.
        let id = NodeId(item.id);
        if id == ROOT || children.contains(&id) {
            continue;
        }
        if item.focused {
            focus = id;
        }
        children.push(id);
        nodes.push((id, node(item, scale)));
    }
    let mut root = Node::new(Role::Window);
    if !title.is_empty() {
        root.set_label(title);
    }
    if screen.is_finite() {
        root.set_bounds(bounds(screen, scale));
    }
    root.set_children(children);
    nodes.insert(0, (ROOT, root));
    TreeUpdate { nodes, tree: Some(Tree::new(ROOT)), focus }
}

mod win {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindow, PostMessageW, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    };

    pub fn is_window(hwnd: isize) -> bool {
        // SAFETY: a plain query of a handle; a stale one answers false.
        unsafe { IsWindow(Some(HWND(hwnd as *mut _))).as_bool() }
    }

    /// The top-level window of this process with this title.
    pub fn own_window(title: &str) -> Option<isize> {
        struct Search<'a> {
            title: &'a str,
            pid: u32,
            found: Option<isize>,
        }
        unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
            // SAFETY: `lparam` is the `Search` below, alive for the whole enumeration.
            let search = unsafe { &mut *(lparam.0 as *mut Search) };
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pid == search.pid {
                let mut buf = [0u16; 256];
                let n = unsafe { GetWindowTextW(hwnd, &mut buf) }.max(0) as usize;
                if String::from_utf16_lossy(&buf[..n]) == search.title {
                    search.found = Some(hwnd.0 as isize);
                    return BOOL(0);
                }
            }
            BOOL(1)
        }
        let mut search = Search { title, pid: std::process::id(), found: None };
        // SAFETY: the callback only reads `search` through the pointer passed here.
        let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize)) };
        search.found
    }

    /// A left click at `at` (points in the client area): the pointer moves there, presses and
    /// lets go, as the window's own messages, so egui sees an ordinary click.
    pub fn click(hwnd: isize, at: egui::Pos2, ppp: f32) {
        let (x, y) = ((at.x * ppp).round() as i32, (at.y * ppp).round() as i32);
        let pos = LPARAM(((y as u32 & 0xffff) << 16 | (x as u32 & 0xffff)) as isize);
        let h = Some(HWND(hwnd as *mut _));
        // SAFETY: posting plain mouse messages to our own window.
        unsafe {
            let _ = PostMessageW(h, WM_MOUSEMOVE, WPARAM(0), pos);
            let _ = PostMessageW(h, WM_LBUTTONDOWN, WPARAM(0x0001), pos);
            let _ = PostMessageW(h, WM_LBUTTONUP, WPARAM(0), pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2, Rect, WidgetInfo, WidgetType};

    fn item(id: u64, info: WidgetInfo, focused: bool) -> Item {
        Item { id, rect: Rect::from_min_size(pos2(10.0, 4.0), vec2(30.0, 30.0)), info, focused }
    }

    #[test]
    fn items_become_named_nodes_under_the_window() {
        let items = [
            item(7, WidgetInfo::labeled(WidgetType::Button, true, "Start"), false),
            item(8, WidgetInfo::selected(WidgetType::SelectableLabel, true, true, "Notes"), true),
            item(9, WidgetInfo::selected(WidgetType::Checkbox, true, false, "Do not disturb"), false),
            item(7, WidgetInfo::labeled(WidgetType::Button, true, "twice"), false),
            item(0, WidgetInfo::labeled(WidgetType::Button, true, "the window's id"), false),
        ];
        let t = build("Top bar", Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 40.0)), &items, 1.5);
        assert_eq!(t.focus, NodeId(8));
        assert_eq!(t.nodes.len(), 4, "a repeated id and the window's id are skipped");
        let root = &t.nodes[0].1;
        assert_eq!(root.role(), Role::Window);
        assert_eq!(root.label(), Some("Top bar"));
        assert_eq!(root.children(), &[NodeId(7), NodeId(8), NodeId(9)]);
        let get = |id| &t.nodes.iter().find(|(n, _)| *n == NodeId(id)).unwrap().1;
        assert_eq!(get(7).role(), Role::Button);
        assert_eq!(get(7).label(), Some("Start"));
        assert_eq!(get(7).bounds().unwrap().x0, 15.0);
        assert_eq!(get(8).role(), Role::ListItem);
        assert_eq!(get(8).is_selected(), Some(true));
        assert_eq!(get(9).role(), Role::CheckBox);
        assert_eq!(get(9).toggled(), Some(Toggled::False));
    }

    #[test]
    fn a_label_reads_its_text_as_its_value() {
        let t = build("", Rect::NOTHING, &[item(5, WidgetInfo::labeled(WidgetType::Label, true, "No events"), false)], 1.0);
        assert_eq!(t.nodes[1].1.value(), Some("No events"));
    }

    #[test]
    fn a_disabled_item_cannot_be_clicked_and_a_label_has_no_actions() {
        let t = build("", Rect::NOTHING, &[item(3, WidgetInfo::labeled(WidgetType::Button, false, "Next"), false), item(4, WidgetInfo::labeled(WidgetType::Label, true, "No events"), false)], 1.0);
        assert!(t.nodes[1].1.is_disabled());
        assert!(!t.nodes[1].1.supports_action(Action::Click));
        assert!(!t.nodes[2].1.supports_action(Action::Click));
        assert_eq!(t.focus, ROOT);
    }
}
