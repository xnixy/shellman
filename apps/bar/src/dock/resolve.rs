//! Working out what dock icons are (`shellman_core::settings::dock_resolve`) on a worker thread:
//! it reads the Start menu, the Apps folder and the disk, which must never hold up a frame. A
//! result is kept for the session; the dock is woken when one arrives.

use shellman_core::settings::dock::Pin;
use shellman_core::settings::dock_resolve::resolve;
use shellman_core::settings::dock_world::Real;
use shellman_core::winlist::Relaunch;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{LazyLock, Mutex};

/// A pin as asked about: all it holds, and the relaunch command of a window of it, if one is open.
type Key = (String, Option<PathBuf>, Option<String>, String);

#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub pin: Pin,
}

struct State {
    done: HashMap<Key, Answer>,
    pending: HashSet<Key>,
    jobs: Sender<(Key, Pin, Option<Relaunch>)>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    let (tx, rx) = channel::<(Key, Pin, Option<Relaunch>)>();
    std::thread::Builder::new()
        .name("dock resolve".into())
        .spawn(move || {
            let world = Real::new();
            for (key, pin, relaunch) in rx {
                let resolved = resolve(&pin, relaunch.as_ref(), &world);
                let mut s = STATE.lock().unwrap();
                s.pending.remove(&key);
                s.done.insert(key, Answer { pin: resolved });
                drop(s);
                CHANGED.store(true, Ordering::Release);
                super::wake_docks_from_thread();
            }
        })
        .expect("dock resolve thread");
    Mutex::new(State { done: HashMap::new(), pending: HashSet::new(), jobs: tx })
});

/// An answer arrived since the last call.
static CHANGED: AtomicBool = AtomicBool::new(false);

fn key(pin: &Pin, relaunch: Option<&Relaunch>) -> Key {
    let rest = format!("{:?}{:?}{:?}{:?}", pin.name, pin.launch, pin.icon, pin.home);
    (pin.app_id.to_lowercase(), pin.exe.clone(), relaunch.map(|r| r.command.clone()), rest)
}

/// The pin worked out, if it is known; otherwise asks for it, once, and says None for now.
pub fn get(pin: &Pin, relaunch: Option<&Relaunch>) -> Option<Answer> {
    let key = key(pin, relaunch);
    let mut s = STATE.lock().unwrap();
    if let Some(a) = s.done.get(&key) {
        return Some(a.clone());
    }
    if s.pending.insert(key.clone()) {
        let _ = s.jobs.send((key, pin.clone(), relaunch.cloned()));
    }
    None
}

/// Whether answers arrived since the last call.
pub fn take_changed() -> bool {
    CHANGED.swap(false, Ordering::AcqRel)
}
