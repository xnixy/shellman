//! Drag and drop in the window: dragging items out (a real OLE drag, see
//! `shellman_core::transfer::drag`), and the window as a drop target for Files itself and every
//! other app.
//!
//! The drop target is called by OLE between frames, on the window's thread, and must answer at
//! once what a drop at a point would do. So each frame leaves behind where things can be
//! dropped (`Zone`s: folder cards and rows, the folder's empty space, the rail's places, pins
//! and drives, the tabs, the path's steps), and the target looks the point up in them. The frame
//! lights up the zone under the drag, switches to a tab the drag rests on, and carries out the
//! drops the target took in (through `shell_op`, so progress, conflicts and undo work).

use super::actions::Action;
use super::tab::Loc;
use egui::{Pos2, Rect};
use shellman_core::transfer::target::{Answer, Handler};
use shellman_core::transfer::{self, Offer, Payload, COPY, LINK, MOVE, NONE};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a drag rests on a tab before switching to it.
const TAB_DWELL: Duration = Duration::from_millis(600);

/// Where a drop lands.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Dest {
    Dir(PathBuf),
    Bin,
    /// A tab: into what it shows, and resting on it switches to it.
    Tab(usize),
}

/// A place on screen (in points) that takes drops, and the radius it is drawn lit with.
#[derive(Clone, Debug)]
struct Zone {
    rect: Rect,
    dest: Dest,
    radius: f32,
}

thread_local! {
    /// The zones the frame being drawn has left so far.
    static ZONES: RefCell<Vec<Zone>> = const { RefCell::new(Vec::new()) };
}

/// Note that a drop on `rect` (clipped to what `ui` shows) lands in `dest`. Later zones are on
/// top of earlier ones.
pub(super) fn zone(ui: &egui::Ui, rect: Rect, dest: Dest, radius: f32) {
    let rect = rect.intersect(ui.clip_rect());
    if rect.is_positive() {
        ZONES.with(|z| z.borrow_mut().push(Zone { rect, dest, radius }));
    }
}

/// A folder's zone, for an item that is one.
pub(super) fn item_zone(ui: &egui::Ui, rect: Rect, e: &shellman_core::fsops::Entry, radius: f32) {
    if e.is_dir {
        zone(ui, rect, Dest::Dir(e.path.clone()), radius);
    }
}

/// A drag over the window, as the target last saw it.
#[derive(Clone, Debug)]
struct Hover {
    /// In points, in the window.
    pos: Pos2,
    offer: Offer,
    /// The tab under it, and since when.
    tab: Option<(usize, Instant)>,
    keys: u32,
    allowed: u32,
}

/// A drop the target took in, for the frame to carry out.
pub(super) struct Dropped {
    payload: Payload,
    dest: Option<Dest>,
    effect: u32,
}

/// What the frame and the drop target share.
#[derive(Default)]
pub(super) struct Shared {
    ppp: f32,
    zones: Vec<Zone>,
    /// What each tab drops into.
    tabs: Vec<Option<Dest>>,
    picker: bool,
    hover: Option<Hover>,
    dropped: Vec<Dropped>,
}

impl Shared {
    fn hit(&self, pos: Pos2) -> Option<&Zone> {
        self.zones.iter().rev().find(|z| z.rect.contains(pos))
    }

    /// A tab's zone stands for what the tab shows.
    fn resolve(&self, d: &Dest) -> Option<Dest> {
        match d {
            Dest::Tab(i) => self.tabs.get(*i).cloned().flatten(),
            other => Some(other.clone()),
        }
    }

    /// What a drop of `offer` at `pos` would do, and where.
    fn answer(&self, offer: &Offer, pos: Pos2, keys: u32, allowed: u32) -> (Option<Dest>, Answer) {
        if self.picker {
            // A picker takes a dropped file as the one to go to, not as a copy.
            let effect = if offer.files.is_empty() {
                NONE
            } else if allowed & LINK != 0 {
                LINK
            } else {
                allowed & (COPY | MOVE)
            };
            return (None, Answer { effect: lowest(effect), label: None });
        }
        let Some(dest) = self.hit(pos).and_then(|z| self.resolve(&z.dest)) else {
            return (None, Answer::default());
        };
        let effect = match (&dest, offer.files.is_empty()) {
            (Dest::Dir(d), false) => transfer::choose(&offer.files, d, keys, allowed),
            (Dest::Bin, false) => transfer::choose_bin(&offer.files, allowed),
            (Dest::Dir(_), true) if offer.image || offer.text => transfer::choose_content(allowed),
            _ => NONE,
        };
        let label = match &dest {
            Dest::Dir(d) => Some(super::tab::Loc::Dir(d.clone()).title()),
            Dest::Bin => Some("Recycle Bin".to_owned()),
            Dest::Tab(_) => None,
        };
        (Some(dest), Answer { effect, label })
    }
}

/// The lowest effect in a set (a picker's drop asks for one).
fn lowest(effects: u32) -> u32 {
    [LINK, COPY, MOVE].into_iter().find(|e| effects & e != 0).unwrap_or(NONE)
}

/// The window's drop target: OLE calls it between frames.
struct Target {
    shared: Arc<Mutex<Shared>>,
    ctx: egui::Context,
    hwnd: isize,
}

impl Target {
    /// A point on the screen, in pixels, as a point in the window.
    fn to_window(&self, pt: (i32, i32), ppp: f32) -> Pos2 {
        use windows::Win32::Foundation::{HWND, POINT};
        use windows::Win32::Graphics::Gdi::ClientToScreen;
        let mut origin = POINT::default();
        unsafe {
            let _ = ClientToScreen(HWND(self.hwnd as *mut _), &mut origin);
        }
        let ppp = if ppp > 0.0 { ppp } else { 1.0 };
        egui::pos2(
            (pt.0 - origin.x) as f32 / ppp,
            (pt.1 - origin.y) as f32 / ppp,
        )
    }

    fn update(&self, offer: Option<&Offer>, pt: (i32, i32), keys: u32, allowed: u32) -> Answer {
        let mut s = self.shared.lock().unwrap();
        let pos = self.to_window(pt, s.ppp);
        let offer = match (offer, &s.hover) {
            (Some(o), _) => o.clone(),
            (None, Some(h)) => h.offer.clone(),
            (None, None) => Offer::default(),
        };
        let (_, answer) = s.answer(&offer, pos, keys, allowed);
        let tab = match (s.hit(pos).map(|z| &z.dest), s.hover.as_ref().and_then(|h| h.tab)) {
            (Some(Dest::Tab(i)), Some((j, since))) if *i == j => Some((j, since)),
            (Some(Dest::Tab(i)), _) => Some((*i, Instant::now())),
            _ => None,
        };
        s.hover = Some(Hover { pos, offer, tab, keys, allowed });
        drop(s);
        self.ctx.request_repaint();
        answer
    }
}

impl Handler for Target {
    fn enter(&self, offer: &Offer, pt: (i32, i32), keys: u32, allowed: u32) -> Answer {
        self.update(Some(offer), pt, keys, allowed)
    }

    fn over(&self, pt: (i32, i32), keys: u32, allowed: u32) -> Answer {
        self.update(None, pt, keys, allowed)
    }

    fn leave(&self) {
        self.shared.lock().unwrap().hover = None;
        self.ctx.request_repaint();
    }

    fn dropped(&self, payload: Payload, pt: (i32, i32), keys: u32, allowed: u32) -> u32 {
        let mut s = self.shared.lock().unwrap();
        let pos = self.to_window(pt, s.ppp);
        let offer = match &payload {
            Payload::Files(f) => Offer { files: f.clone(), ..Default::default() },
            Payload::Image(_) => Offer { image: true, ..Default::default() },
            Payload::Text(_) => Offer { text: true, ..Default::default() },
            Payload::Nothing => Offer::default(),
        };
        let (dest, answer) = s.answer(&offer, pos, keys, allowed);
        s.hover = None;
        // A picker's drop has no destination: the file is where to go.
        if answer.effect != NONE && (dest.is_some() || s.picker) {
            s.dropped.push(Dropped {
                payload,
                dest,
                effect: answer.effect,
            });
        }
        drop(s);
        self.ctx.request_repaint();
        answer.effect
    }
}

/// Files' side of drag and drop.
pub(super) struct Dnd {
    shared: Arc<Mutex<Shared>>,
    /// A drag out is on (from its start until the window has the button back).
    pub(super) dragging_out: bool,
}

impl Dnd {
    /// Make the window a drop target. Nothing happens when there is no window.
    pub(super) fn new(ctx: &egui::Context, hwnd: isize, picker: bool) -> Self {
        let shared = Arc::new(Mutex::new(Shared {
            picker,
            ..Default::default()
        }));
        if hwnd != 0 {
            let target = Target {
                shared: shared.clone(),
                ctx: ctx.clone(),
                hwnd,
            };
            let _ = transfer::target::register(hwnd, Box::new(target));
        }
        Self {
            shared,
            dragging_out: false,
        }
    }
}

/// Whether the pointer has moved past the system's drag threshold since it went down.
pub(super) fn past_threshold(ctx: &egui::Context) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXDRAG, SM_CYDRAG};
    let (from, now, ppp) = ctx.input(|i| (i.pointer.press_origin(), i.pointer.latest_pos(), i.pixels_per_point));
    let (Some(from), Some(now)) = (from, now) else { return false };
    let (cx, cy) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
    let d = (now - from) * ppp;
    d.x.abs() > cx.max(1) as f32 || d.y.abs() > cy.max(1) as f32
}

impl super::FileMan {
    /// Before drawing: start this frame's zones, carry out drops, and switch to a tab a drag
    /// has rested on.
    pub(super) fn dnd_begin(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        ZONES.with(|z| z.borrow_mut().clear());
        if self.dnd.dragging_out && !ctx.input(|i| i.pointer.any_down()) {
            self.dnd.dragging_out = false;
        }
        let (dropped, tab) = {
            let mut s = self.dnd.shared.lock().unwrap();
            s.ppp = ctx.pixels_per_point();
            s.tabs = self
                .tabs
                .iter()
                .map(|t| match &t.loc {
                    Loc::Dir(p) => Some(Dest::Dir(p.clone())),
                    Loc::Bin => Some(Dest::Bin),
                    _ => None,
                })
                .collect();
            (std::mem::take(&mut s.dropped), s.hover.as_ref().and_then(|h| h.tab))
        };
        if let Some((i, since)) = tab {
            if i != self.active {
                let rested = since.elapsed();
                if rested >= TAB_DWELL {
                    actions.push(Action::SwitchTab(i));
                } else {
                    ctx.request_repaint_after(TAB_DWELL - rested);
                }
            }
        }
        for d in dropped {
            self.carry_out(d);
        }
    }

    /// After drawing: keep the zones for the drop target, and light up the one under a drag.
    pub(super) fn dnd_end(&mut self, ctx: &egui::Context) {
        let zones = ZONES.with(|z| std::mem::take(&mut *z.borrow_mut()));
        let mut s = self.dnd.shared.lock().unwrap();
        s.zones = zones;
        let Some(h) = &s.hover else { return };
        let Some(z) = s.hit(h.pos) else { return };
        let (dest, answer) = s.answer(&h.offer, h.pos, h.keys, h.allowed);
        if dest.is_none() || answer.effect == NONE && !matches!(z.dest, Dest::Tab(_)) {
            return;
        }
        let c = shellman_ui::theme::colors();
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop-zone")));
        let r = egui::CornerRadius::same(z.radius.clamp(0.0, 255.0) as u8);
        p.rect(
            z.rect.shrink(1.0),
            r,
            shellman_ui::glass::islands::shows_as(shellman_ui::theme::with_alpha(c.accent_fill, 46)),
            egui::Stroke::new(2.0_f32, c.accent_fill),
            egui::StrokeKind::Inside,
        );
    }

    /// Do what a drop asked.
    fn carry_out(&mut self, d: Dropped) {
        if self.pick.is_some() {
            // In a picker, a dropped file is where to go: its folder, with it selected.
            if let Payload::Files(paths) = d.payload {
                if let Some(p) = paths.into_iter().next() {
                    // Folder or file: looked at on a worker (see `probed`).
                    self.probe(super::Probed::Dropped(p));
                }
            }
            return;
        }
        match (d.payload, d.dest) {
            // Straight to the bin when all can go there; else it asks first, as a delete does.
            (Payload::Files(paths), Some(Dest::Bin)) => self.recycle(paths, false),
            (Payload::Files(paths), Some(Dest::Dir(dir))) => self.transfer_files(paths, &dir, d.effect, None),
            (Payload::Image(png), Some(Dest::Dir(dir))) => self.write_new(dir, "Dropped image.png", png),
            (Payload::Text(t), Some(Dest::Dir(dir))) => self.write_new(dir, "Dropped text.txt", t.into_bytes()),
            _ => {}
        }
    }

    /// Move, copy or link `paths` into `dir`, as `effect` says. `clear_clip` is the clipboard's
    /// sequence number to clear after a cut's move.
    pub(super) fn transfer_files(&mut self, paths: Vec<PathBuf>, dir: &Path, effect: u32, clear_clip: Option<u32>) {
        if transfer::refused(&paths, dir) {
            self.toast("A folder can't go inside itself");
            return;
        }
        let here = paths
            .iter()
            .all(|p| p.parent().is_some_and(|q| transfer::path_key(q) == transfer::path_key(dir)));
        match effect {
            MOVE if here => {}
            MOVE => self.run_plan(super::OpPlan {
                op: shellman_core::fsops::Op::Move,
                what: "Move",
                from: paths,
                to: Some(dir.to_path_buf()),
                clear_clip,
            }),
            LINK => self.make_links(paths, dir),
            _ => {
                let op = if here {
                    shellman_core::fsops::Op::Duplicate
                } else {
                    shellman_core::fsops::Op::Copy
                };
                self.run_op(op, "Copy", paths, Some(dir.to_path_buf()));
            }
        }
    }

    /// Shortcuts to `paths`, in `dir`; the last one made is selected.
    pub(super) fn make_links(&mut self, paths: Vec<PathBuf>, dir: &Path) {
        let mut last = None;
        for p in paths {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let link = shellman_core::fsops::unique_child(dir, &format!("{name} - Shortcut.lnk"));
            match shellman_core::fsops::make_link(&p, &link) {
                Ok(()) => last = Some(link),
                Err(e) => self.toast(format!("Couldn't make a shortcut: {e}")),
            }
        }
        if let Some(l) = last {
            self.select_new(l, false);
        }
    }

    /// Write a new file in `dir` (a dropped or pasted picture or text) on a worker; it is
    /// selected and put into rename when it shows up.
    pub(super) fn write_new(&self, dir: PathBuf, name: &'static str, bytes: Vec<u8>) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let r = super::clip::write_new(&dir, name, &bytes);
            let _ = tx.send(super::Msg::Created(r));
            ctx.request_repaint();
        });
    }

    /// Select `path` once its folder (the one shown) has listed it, and rename it if asked.
    pub(super) fn select_new(&mut self, path: PathBuf, rename: bool) {
        if path.parent() == self.tabs[self.active].loc.dir() {
            self.tabs[self.active].select_after_load = Some((path, rename));
            self.load(self.active);
        }
    }

    /// Start dragging item `r` out: the selection when it is in it, else just it (selected).
    pub(super) fn drag_out(&mut self, r: usize) {
        if self.dnd.dragging_out || self.pick.is_some() {
            return;
        }
        let tab = &mut self.tabs[self.active];
        if matches!(tab.loc, Loc::Bin | Loc::Pc) || r >= tab.view.len() {
            return;
        }
        if !tab.selected.contains(&tab.entry(r).path) {
            tab.select_only(r);
        }
        let entries = tab.selection();
        if entries.is_empty() {
            return;
        }
        self.dnd.dragging_out = true;
        let ppp = self.ctx.pixels_per_point();
        let image = super::drag_image::render(&self.ctx, &entries, ppp);
        let paths: Vec<PathBuf> = entries.into_iter().map(|e| e.path).collect();
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        transfer::drag::start(self.hwnd, paths, image, COPY | MOVE | LINK, move |ended| {
            let _ = tx.send(super::Msg::DragEnded(ended));
            ctx.request_repaint();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared() -> Shared {
        let mut s = Shared {
            ppp: 1.0,
            tabs: vec![Some(Dest::Dir(PathBuf::from(r"C:\T\one"))), Some(Dest::Bin), None],
            ..Default::default()
        };
        let z = |x: f32, dest| Zone {
            rect: Rect::from_min_size(egui::pos2(x, 0.0), egui::vec2(100.0, 100.0)),
            dest,
            radius: 0.0,
        };
        // The folder's space, then a folder card on top of it.
        s.zones = vec![
            Zone {
                rect: Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1000.0, 1000.0)),
                dest: Dest::Dir(PathBuf::from(r"C:\T\here")),
                radius: 0.0,
            },
            z(0.0, Dest::Dir(PathBuf::from(r"C:\T\here\sub"))),
            z(200.0, Dest::Tab(0)),
            z(400.0, Dest::Tab(1)),
            z(600.0, Dest::Tab(2)),
        ];
        s
    }

    fn files(p: &[&str]) -> Offer {
        Offer {
            files: p.iter().map(PathBuf::from).collect(),
            ..Default::default()
        }
    }

    const ALL: u32 = COPY | MOVE | LINK;

    #[test]
    fn the_zone_on_top_takes_the_drop() {
        let s = shared();
        let o = files(&[r"C:\T\x.txt"]);
        let (d, a) = s.answer(&o, egui::pos2(50.0, 50.0), 0, ALL);
        assert_eq!(d, Some(Dest::Dir(PathBuf::from(r"C:\T\here\sub"))));
        assert_eq!(a.effect, MOVE);
        assert_eq!(a.label.as_deref(), Some("sub"));
        // On empty space: the folder shown.
        let (d, _) = s.answer(&o, egui::pos2(150.0, 500.0), 0, ALL);
        assert_eq!(d, Some(Dest::Dir(PathBuf::from(r"C:\T\here"))));
        // Outside every zone: nothing.
        let (d, a) = s.answer(&o, egui::pos2(5000.0, 5.0), 0, ALL);
        assert_eq!((d, a.effect), (None, NONE));
    }

    #[test]
    fn tabs_stand_for_what_they_show() {
        let s = shared();
        let o = files(&[r"C:\T\x.txt"]);
        let (d, a) = s.answer(&o, egui::pos2(250.0, 50.0), MK_CTRL, ALL);
        assert_eq!(d, Some(Dest::Dir(PathBuf::from(r"C:\T\one"))));
        assert_eq!(a.effect, COPY);
        let (d, a) = s.answer(&o, egui::pos2(450.0, 50.0), 0, ALL);
        assert_eq!((d, a.effect), (Some(Dest::Bin), MOVE));
        assert_eq!(a.label.as_deref(), Some("Recycle Bin"));
        // A tab with nowhere to drop (This PC, a tag place).
        assert_eq!(s.answer(&o, egui::pos2(650.0, 50.0), 0, ALL).1.effect, NONE);
    }
    const MK_CTRL: u32 = transfer::MK_CONTROL;

    #[test]
    fn content_and_refusals() {
        let s = shared();
        let pic = Offer { image: true, ..Default::default() };
        assert_eq!(s.answer(&pic, egui::pos2(50.0, 50.0), 0, ALL).1.effect, COPY);
        assert_eq!(s.answer(&pic, egui::pos2(450.0, 50.0), 0, ALL).1.effect, NONE, "no pictures in the bin");
        // A folder dropped on itself.
        let me = files(&[r"C:\T\here\sub"]);
        assert_eq!(s.answer(&me, egui::pos2(50.0, 50.0), 0, ALL).1.effect, NONE);
        // Moving into the folder it is already in.
        let local = files(&[r"C:\T\here\a.txt"]);
        assert_eq!(s.answer(&local, egui::pos2(150.0, 500.0), 0, ALL).1.effect, NONE);
    }

    #[test]
    fn a_picker_takes_files_to_go_to() {
        let mut s = shared();
        s.picker = true;
        let o = files(&[r"C:\T\x.txt"]);
        assert_eq!(s.answer(&o, egui::pos2(50.0, 50.0), 0, ALL).1.effect, LINK);
        assert_eq!(s.answer(&o, egui::pos2(50.0, 50.0), 0, COPY | MOVE).1.effect, COPY);
        let text = Offer { text: true, ..Default::default() };
        assert_eq!(s.answer(&text, egui::pos2(50.0, 50.0), 0, ALL).1.effect, NONE);
    }
}
