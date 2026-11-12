//! The window's side of the shell menu: which walk is wanted, what has arrived, what to show
//! meanwhile, and when a worker has hung. Nothing here waits: the window asks, polls once a
//! frame, and shows whatever is there.

use super::model::{find_like, Cascade, Item};
use super::paint::Palette;
use super::slow::{self, SlowBook, TOO_SLOW};
use super::target::Target;
use super::tidy::{route, tidy, Route};
use super::worker::{Event, Request, Worker};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A finished walk this recent is shown as is when its menu opens; an older one is shown while a
/// fresh walk replaces it.
const FRESH: Duration = Duration::from_secs(4);
/// A worker that has shown no progress for this long is given up on, and a new one started.
const HUNG: Duration = Duration::from_secs(6);
/// Menus remembered per kind of target, to stand in while the real one loads.
const MAX_CACHED: usize = 64;
/// The book of slow handlers, in the config folder.
const BOOK_FILE: &str = "files-menu-slow.json";

/// The newest walk.
struct Current {
    gen: u64,
    target: Target,
    key: String,
    asked: Instant,
    /// The tree as read so far, and as Files shows it.
    raw: Option<Vec<Item>>,
    shown: Vec<Item>,
    done: bool,
    failed: bool,
    finished: Option<Instant>,
    /// The submenu being filled, and since when.
    filling: Option<(String, Instant)>,
    /// Submenus that took longer than the wait during this walk.
    too_slow: Vec<String>,
    /// When the top level arrived, for the trace.
    top_ms: Option<u128>,
    fills: Vec<(String, u128)>,
}

/// What the menu shows of the shell's entries now.
pub struct View<'a> {
    pub items: &'a [Item],
    /// False while a stand-in (an older walk of a menu like it) is shown.
    pub fresh: bool,
    /// The walk is still going.
    pub loading: bool,
    /// A short line about entries that took too long.
    pub note: Option<String>,
}

pub struct Loader {
    hwnd: isize,
    repaint: Arc<dyn Fn() + Send + Sync>,
    worker: Option<Worker>,
    /// When the worker last did something, while it has work outstanding.
    worker_busy: Option<Instant>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    next_gen: u64,
    current: Option<Current>,
    cache: HashMap<String, Vec<Item>>,
    book: SlowBook,
    book_path: Option<PathBuf>,
    palette: Palette,
    /// A pick made on a stand-in, run once the real entry arrives.
    pending_pick: Option<(u64, Vec<Item>, (i32, i32))>,
    /// Something the window should tell the person.
    notice: Option<String>,
    trace: Option<PathBuf>,
}

impl Loader {
    pub fn new(hwnd: isize, repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = channel();
        let book_path = crate::config::config_dir().map(|d| d.join(BOOK_FILE));
        let book = book_path.as_deref().map(SlowBook::load).unwrap_or_default();
        Self {
            hwnd,
            repaint,
            worker: None,
            worker_busy: None,
            tx,
            rx,
            next_gen: 1,
            current: None,
            cache: HashMap::new(),
            book,
            book_path,
            palette: Palette { text: 0xF0F0F0, background: 0x202020 },
            pending_pick: None,
            notice: None,
            trace: std::env::var_os("SHELLMAN_MENU_TRACE").map(PathBuf::from),
        }
    }

    /// The colours owner-drawn entries are asked to use.
    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }

    /// Have the menu for `target` ready. Cheap to call every frame: it starts a walk only when
    /// the target changed or the last walk is stale.
    pub fn want(&mut self, target: &Target) {
        if let Some(c) = &self.current {
            let fresh = c.finished.is_none_or(|t| t.elapsed() < FRESH);
            if c.target == *target && fresh && !c.failed && self.worker.is_some() {
                return;
            }
        }
        self.start(target.clone());
    }

    /// Walk again even if the last walk for this target is recent: the menu is opening now.
    pub fn refresh(&mut self, target: &Target) {
        match &self.current {
            Some(c) if c.target == *target && !c.done && !c.failed && self.worker.is_some() => {}
            Some(c) if c.target == *target && c.finished.is_some_and(|t| t.elapsed() < FRESH) => {}
            _ => self.start(target.clone()),
        }
    }

    fn start(&mut self, target: Target) {
        // A worker stuck in an older walk is left to it; a new one takes this.
        let stuck = self.worker_busy.is_some_and(|t| t.elapsed() > TOO_SLOW);
        if stuck {
            self.abandon_worker();
        }
        if self.worker.is_none() {
            self.worker = Worker::spawn(self.tx.clone(), self.repaint.clone());
        }
        let gen = self.next_gen;
        self.next_gen += 1;
        let kinds: Vec<String> = target
            .items
            .iter()
            .map(|p| slow::kind_of(p, p.is_dir()))
            .collect();
        let key = slow::type_key(&kinds, target.extended);
        let req = Request::Load {
            gen,
            target: target.clone(),
            hwnd: self.hwnd,
            plan: self.book.clone(),
            palette: self.palette,
        };
        let sent = self.worker.as_ref().is_some_and(|w| w.send(req));
        self.current = Some(Current {
            gen,
            target,
            key,
            asked: Instant::now(),
            raw: None,
            shown: Vec::new(),
            done: !sent,
            failed: !sent,
            finished: None,
            filling: None,
            too_slow: Vec::new(),
            top_ms: None,
            fills: Vec::new(),
        });
        self.worker_busy = sent.then(Instant::now);
        if !sent {
            self.worker = None;
        }
    }

    fn abandon_worker(&mut self) {
        // Dropping the sender ends the thread once it is free again; until then it is left alone.
        self.worker = None;
        self.worker_busy = None;
    }

    /// Take in what the worker sent, and give up on it if it hung.
    pub fn poll(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            self.take(ev);
        }
        let Some(c) = self.current.as_mut() else { return };
        if c.done {
            return;
        }
        // A submenu past the wait: note it now, so next time it is not waited for.
        if let Some((key, since)) = &c.filling {
            if since.elapsed() > TOO_SLOW && !c.too_slow.contains(key) {
                c.too_slow.push(key.clone());
                self.book.record(key, since.elapsed());
                self.save_book();
            }
        }
        let c = self.current.as_mut().unwrap();
        if self.worker_busy.is_some_and(|t| t.elapsed() > HUNG) {
            c.done = true;
            c.failed = c.raw.is_none();
            c.finished = Some(Instant::now());
            self.abandon_worker();
        } else if !c.done {
            (self.repaint)();
        }
    }

    fn take(&mut self, ev: Event) {
        let gen = match &ev {
            Event::Tree { gen, .. }
            | Event::Filling { gen, .. }
            | Event::Filled { gen, .. }
            | Event::Failed { gen } => *gen,
        };
        let Some(c) = self.current.as_mut().filter(|c| c.gen == gen) else {
            return;
        };
        self.worker_busy = Some(Instant::now());
        match ev {
            Event::Tree { items, done, .. } => {
                if c.top_ms.is_none() {
                    c.top_ms = Some(c.asked.elapsed().as_millis());
                }
                c.shown = tidy(items.clone());
                c.raw = Some(items);
                if self.cache.len() >= MAX_CACHED {
                    self.cache.clear();
                }
                self.cache.insert(c.key.clone(), c.shown.clone());
                if done {
                    c.done = true;
                    c.filling = None;
                    c.finished = Some(Instant::now());
                    self.worker_busy = None;
                    self.write_trace();
                }
                self.run_pending_pick();
            }
            Event::Filling { key, .. } => c.filling = Some((key, Instant::now())),
            Event::Filled { key, took, .. } => {
                c.filling = None;
                c.fills.push((key.clone(), took.as_millis()));
                if self.book.record(&key, took) {
                    self.save_book();
                }
            }
            Event::Failed { .. } => {
                c.done = true;
                c.failed = true;
                c.finished = Some(Instant::now());
                self.worker_busy = None;
                self.write_trace();
            }
        }
    }

    /// The shell's entries for `target` as they stand, if a walk for it was asked for.
    pub fn view(&self, target: &Target) -> Option<View<'_>> {
        let c = self.current.as_ref().filter(|c| c.target == *target)?;
        let stand_in = self.cache.get(&c.key).filter(|_| c.raw.is_none());
        let (items, fresh): (&[Item], bool) = match (&c.raw, stand_in) {
            (Some(_), _) => (&c.shown, true),
            (None, Some(old)) => (old, false),
            (None, None) => (&[], false),
        };
        let mut note = None;
        let slow = c.too_slow.len() + deferred(items);
        if slow > 0 {
            note = Some(format!("{slow} item{} took too long", if slow == 1 { "" } else { "s" }));
        } else if c.raw.is_none() && c.asked.elapsed() > TOO_SLOW {
            note = Some(if c.done {
                "Windows entries took too long".to_string()
            } else {
                "Windows entries are taking long".to_string()
            });
        }
        Some(View { items, fresh, loading: !c.done, note })
    }

    /// The person picked `chain` (the entry and the submenus above it, as the view showed them)
    /// at `point`. Files' own handling comes back as a route; the shell's commands run on the
    /// worker, now or as soon as the real entry arrives.
    pub fn pick(&mut self, target: &Target, chain: &[&Item], fresh: bool, point: (i32, i32)) -> Route {
        let Some(leaf) = chain.last() else { return Route::Invoke };
        let r = route(leaf.verb.as_deref(), target.folders_only());
        if r != Route::Invoke {
            return r;
        }
        let Some(c) = self.current.as_ref().filter(|c| c.target == *target) else {
            return r;
        };
        let gen = c.gen;
        if fresh {
            self.invoke(gen, leaf.id, point);
        } else {
            self.pending_pick = Some((gen, chain.iter().map(|i| (*i).clone()).collect(), point));
            self.run_pending_pick();
        }
        r
    }

    fn run_pending_pick(&mut self) {
        let Some((gen, chain, point)) = self.pending_pick.take() else { return };
        let Some(c) = self.current.as_ref().filter(|c| c.gen == gen) else { return };
        if c.raw.is_none() {
            if !c.done {
                self.pending_pick = Some((gen, chain, point));
            }
            return;
        }
        let refs: Vec<&Item> = chain.iter().collect();
        match find_like(&c.shown, &refs).and_then(|p| super::model::at(&c.shown, &p).cloned()) {
            Some(item) if !item.is_submenu() => self.invoke(gen, item.id, point),
            // Its submenu is still being filled: wait for it.
            _ if !c.done => self.pending_pick = Some((gen, chain, point)),
            _ => self.notice = Some("That entry is no longer there".into()),
        }
    }

    fn invoke(&mut self, gen: u64, id: u32, point: (i32, i32)) {
        self.trace_line(&format!("invoke gen={gen} id={id} at={point:?}\n"));
        if let Some(w) = self.worker.take() {
            w.send(Request::Invoke { gen, id, hwnd: self.hwnd, point });
        }
        // The worker keeps the menu until the command is done, then ends; the next menu gets a
        // fresh one.
        self.worker_busy = None;
        self.current = None;
    }

    /// The person opened a submenu that was left unfilled because its handler was slow.
    pub fn expand(&mut self, target: &Target, item: &Item) {
        if item.cascade != Cascade::Deferred {
            return;
        }
        let Some(c) = self.current.as_mut().filter(|c| c.target == *target && c.raw.is_some()) else {
            return;
        };
        let Some(w) = &self.worker else { return };
        if w.send(Request::Expand { gen: c.gen, path: item.path.clone(), plan: self.book.clone() }) {
            mark_pending(&mut c.shown, &item.path);
            if let Some(raw) = c.raw.as_mut() {
                mark_pending(raw, &item.path);
            }
            c.done = false;
            self.worker_busy = Some(Instant::now());
        }
    }

    /// A message for the person, once.
    pub fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }

    fn save_book(&self) {
        if let Some(p) = &self.book_path {
            self.book.save(p);
        }
    }

    /// With SHELLMAN_MENU_TRACE set to a file, one line per walk: what for, when the top level
    /// and the rest arrived, and each submenu's time.
    fn write_trace(&self) {
        let (Some(_), Some(c)) = (&self.trace, &self.current) else { return };
        let fills: Vec<String> = c.fills.iter().map(|(k, ms)| format!("{k}={ms}ms")).collect();
        let line = format!(
            "{key} items={n} top={top:?}ms done={done}ms failed={f} fills=[{fills}]\n",
            key = c.key,
            n = c.target.items.len(),
            top = c.top_ms,
            done = c.asked.elapsed().as_millis(),
            f = c.failed,
            fills = fills.join(", "),
        );
        self.trace_line(&line);
    }

    fn trace_line(&self, line: &str) {
        let Some(path) = &self.trace else { return };
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

/// Submenus in the tree left for when they are opened.
fn deferred(items: &[Item]) -> usize {
    items
        .iter()
        .map(|i| usize::from(i.cascade == Cascade::Deferred) + deferred(&i.submenu))
        .sum()
}

/// Marks the submenu whose shell path is `path` as being filled.
fn mark_pending(items: &mut [Item], path: &[usize]) {
    for i in items.iter_mut() {
        if i.path == path {
            i.cascade = Cascade::Pending;
            return;
        }
        mark_pending(&mut i.submenu, path);
    }
}
