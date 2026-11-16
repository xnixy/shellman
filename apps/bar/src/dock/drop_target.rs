//! The dock windows as OLE drop targets (see `drop` for the rules).
//!
//! Each dock window's frame publishes where its icons are (`publish`), in screen pixels; the
//! drop target, called by OLE on the UI thread between frames, aims at those, answers the drag
//! image, brings an app forward after resting on it, and does the drop. Pinning changes the
//! dock's pins, so it is queued for the dock's next frame (`take_pins`). Opening and recycling
//! run on threads of their own: the shell's calls block. The dock windows never take the focus;
//! OLE does not activate a drop target.

use super::drop::{self as rules, Aim, Map, Plan, Spring};
use crate::wake::Wake;
use shellman_core::settings::dock::Pin;
use shellman_core::transfer::target::{self, Answer, Handler};
use shellman_core::transfer::{Offer, Payload, NONE};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// What is being dragged over which dock window, for its highlight.
#[derive(Clone, Debug, PartialEq)]
pub struct Hover {
    pub hwnd: isize,
    pub aim: Aim,
    /// The aimed icon's id, or None for a gap or nothing.
    pub key: Option<String>,
    /// Whether a drop there would do something.
    pub ok: bool,
}

#[derive(Default)]
struct State {
    maps: HashMap<isize, Map>,
    files: Vec<PathBuf>,
    pinnable: Option<Vec<Pin>>,
    hover: Option<Hover>,
    spring: Spring,
    pins: Vec<(Vec<Pin>, usize)>,
}

#[derive(Clone)]
pub struct Drops {
    state: Arc<Mutex<State>>,
    wake: Wake,
    registered: Arc<Mutex<Vec<isize>>>,
}

impl Drops {
    pub fn new(wake: Wake) -> Self {
        Self { state: Arc::default(), wake, registered: Arc::default() }
    }

    /// Make `hwnd` (a dock window) a drop target, once.
    pub fn register(&self, hwnd: isize) {
        let mut done = self.registered.lock().unwrap();
        if done.contains(&hwnd) {
            return;
        }
        done.push(hwnd);
        unsafe {
            // Counted, and the window library has usually done it already on this thread.
            let _ = windows::Win32::System::Ole::OleInitialize(None);
        }
        let handler = Target { hwnd, drops: self.clone() };
        let _ = target::register(hwnd, Box::new(handler));
    }

    /// Where `hwnd`'s icons are now.
    pub fn publish(&self, hwnd: isize, map: Map) {
        let mut s = self.state.lock().unwrap();
        if s.maps.get(&hwnd) != Some(&map) {
            s.maps.insert(hwnd, map);
        }
    }

    /// What is dragged over `hwnd`, for its highlight.
    pub fn hover_on(&self, hwnd: isize) -> Option<Hover> {
        self.state.lock().unwrap().hover.clone().filter(|h| h.hwnd == hwnd)
    }

    /// Pins dropped since the last call: what, and at which pinned slot.
    pub fn take_pins(&self) -> Vec<(Vec<Pin>, usize)> {
        std::mem::take(&mut self.state.lock().unwrap().pins)
    }
}

struct Target {
    hwnd: isize,
    drops: Drops,
}

fn is_dir(p: &Path) -> bool {
    p.is_dir()
}

/// The pins for what is dragged, if all of it can be pinned.
fn pins_for(files: &[PathBuf]) -> Option<Vec<Pin>> {
    if files.is_empty() {
        return None;
    }
    files
        .iter()
        .map(|f| {
            let link = f.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk"));
            let target = link.then(|| shellman_core::fsops::link_target(f)).flatten();
            rules::pin_for(f, f.is_dir(), target)
        })
        .collect()
}

impl Target {
    /// Aim at `pt`, spring-load, and say what a drop would do.
    fn look(&self, pt: (i32, i32), allowed: u32) -> Answer {
        let mut s = self.drops.state.lock().unwrap();
        let Some(map) = s.maps.get(&self.hwnd).cloned() else { return Answer::default() };
        let aim = rules::aim(&map, pt, s.pinnable.is_some());
        let (plan, key, front) = match &aim {
            Aim::Icon(i) => {
                let spot = &map.icons[*i];
                let front = match &spot.icon {
                    rules::Icon::App { front, .. } => *front,
                    _ => None,
                };
                (rules::plan_icon(&spot.icon, &s.files, is_dir), Some(spot.key.clone()), front)
            }
            Aim::Gap(slot) => (s.pinnable.clone().map_or(Plan::Nothing, |p| Plan::Pin(p, *slot)), None, None),
            Aim::Nothing => (Plan::Nothing, None, None),
        };
        let effect = rules::effect(&plan, &s.files, allowed);
        // Resting on an app with a window brings it forward, whatever a drop there would do.
        let spring_key = front.and(key.as_deref());
        if s.spring.over(Instant::now(), spring_key) {
            if let Some(w) = front {
                spring(w);
            }
        }
        let hover = Hover { hwnd: self.hwnd, aim, key, ok: effect != NONE || front.is_some() };
        if s.hover.as_ref() != Some(&hover) {
            s.hover = Some(hover);
            drop(s);
            self.drops.wake.docks();
        }
        Answer { effect, label: None }
    }

    fn clear(&self) {
        let mut s = self.drops.state.lock().unwrap();
        s.hover = None;
        s.files.clear();
        s.pinnable = None;
        s.spring = Spring::default();
        drop(s);
        self.drops.wake.docks();
    }
}

impl Handler for Target {
    fn enter(&self, offer: &Offer, pt: (i32, i32), _keys: u32, allowed: u32) -> Answer {
        {
            let mut s = self.drops.state.lock().unwrap();
            s.files = offer.files.clone();
            s.pinnable = pins_for(&offer.files);
            s.spring = Spring::default();
        }
        self.look(pt, allowed)
    }

    fn over(&self, pt: (i32, i32), _keys: u32, allowed: u32) -> Answer {
        self.look(pt, allowed)
    }

    fn leave(&self) {
        self.clear();
    }

    fn dropped(&self, payload: Payload, pt: (i32, i32), _keys: u32, allowed: u32) -> u32 {
        let files = match payload {
            Payload::Files(f) => f,
            _ => Vec::new(),
        };
        let plan = {
            let mut s = self.drops.state.lock().unwrap();
            s.files = files.clone();
            let map = s.maps.get(&self.hwnd).cloned().unwrap_or_default();
            match rules::aim(&map, pt, s.pinnable.is_some()) {
                Aim::Icon(i) => rules::plan_icon(&map.icons[i].icon, &files, is_dir),
                Aim::Gap(slot) => pins_for(&files).map_or(Plan::Nothing, |p| Plan::Pin(p, slot)),
                Aim::Nothing => Plan::Nothing,
            }
        };
        let effect = rules::effect(&plan, &files, allowed);
        if effect != NONE {
            self.carry_out(plan, files);
        }
        self.clear();
        effect
    }
}

impl Target {
    fn carry_out(&self, plan: Plan, files: Vec<PathBuf>) {
        match plan {
            Plan::Nothing => {}
            Plan::Recycle => {
                // Files' own "Move to Recycle Bin": the shell's undoable delete, never for good.
                std::thread::spawn(move || {
                    let _ = shellman_core::fsops::shell_op(0, shellman_core::fsops::Op::Recycle, &files, None);
                });
            }
            Plan::Reveal(folder) => {
                std::thread::spawn(move || {
                    shellman_core::sibling::start_with("files", &[&folder.to_string_lossy()]);
                });
            }
            Plan::Select(file) => {
                std::thread::spawn(move || {
                    shellman_core::sibling::start_with("files", &["--select", &file.to_string_lossy()]);
                });
            }
            Plan::OpenWith(with) => {
                std::thread::spawn(move || {
                    let _ = shellman_core::open_with::open(&with, &files);
                });
            }
            Plan::Pin(pins, slot) => {
                self.drops.state.lock().unwrap().pins.push((pins, slot));
                self.drops.wake.docks();
            }
        }
    }
}

/// Bring an app's window forward in the middle of a drag. The drag's source has the input, so
/// Windows may refuse the bar the foreground; one unused key press makes the bar the last
/// input's sender, and it asks again.
fn spring(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE};
    let h = HWND(hwnd as *mut _);
    unsafe {
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        if !SetForegroundWindow(h).as_bool() || GetForegroundWindow() != h {
            shellman_core::foreground::send_mask();
            let _ = SetForegroundWindow(h);
        }
    }
}
