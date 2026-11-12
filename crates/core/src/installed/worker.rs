//! The Apps page's worker. It sleeps until asked to read the list or to run an action, reads the
//! registry and the package manager off the UI thread, and publishes the newest list. Each
//! uninstall or change runs on a thread of its own (the program may stay open for minutes), and
//! the list is read again when it ends; the Uninstall keys changing also asks for a read.

use super::act::{self, Action, Progress, Runner};
use super::app::App;
use super::{classic, list, packaged, watch};
use crate::registry::Real;
use crate::startup::watch::RunWatch;
use crate::startup::win::expand;
use crate::system::live::{self, Mailbox, Wake};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// What the page shows.
#[derive(Default, Clone)]
pub struct Page {
    pub apps: Arc<Vec<App>>,
    /// Apps with an uninstall or a change under way.
    pub busy: Vec<String>,
    /// How the last action ended, when it is worth saying (a failure, or the dry run's note).
    pub notice: Option<String>,
    /// The Store apps are still being read; the classic ones are already here.
    pub reading_packaged: bool,
}

/// What an app's action is, for the row to say.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Do {
    Uninstall,
    Modify,
}

enum Cmd {
    /// Read the list again; `packaged` also reads the Store apps (they do not change with the keys).
    Refresh { packaged: bool },
    Act(Box<App>, Do),
    Done { id: String, result: Result<String, String>, packaged: bool },
    Stop,
}

pub struct AppsPage {
    tx: Sender<Cmd>,
    mailbox: Arc<Mailbox<Page>>,
    progress: Arc<Mutex<HashMap<String, u32>>>,
    _watch: RunWatch,
}

fn exists(p: &std::path::Path) -> bool {
    p.exists()
}

/// The classic programs, read now.
pub fn read_classic() -> Vec<App> {
    classic::collect(&Real, &expand, &exists)
}

impl AppsPage {
    /// Start following; `wake` is called whenever there is a new page. The first reading follows
    /// at once.
    pub fn start(wake: Wake) -> Self {
        Self::start_with(wake, act::runner())
    }

    pub fn start_with(wake: Wake, runner: Arc<dyn Runner>) -> Self {
        let (tx, rx) = channel();
        let mailbox = Arc::new(Mailbox::default());
        let progress = Arc::new(Mutex::new(HashMap::new()));
        let out = mailbox.clone();
        let worker = Worker { tx: tx.clone(), wake, runner, shared: progress.clone(), page: Page::default(), read_store: false };
        live::spawn("apps-page", move || run(rx, worker, &out));
        let _ = tx.send(Cmd::Refresh { packaged: true });
        let for_watch = tx.clone();
        let watch = watch::start(move || {
            let _ = for_watch.send(Cmd::Refresh { packaged: false });
        });
        Self { tx, mailbox, progress, _watch: watch }
    }

    pub fn take(&self) -> Option<Page> {
        self.mailbox.take()
    }

    /// Uninstall or change an app.
    pub fn act(&self, app: &App, what: Do) {
        let _ = self.tx.send(Cmd::Act(Box::new(app.clone()), what));
    }

    /// How far a Store app's removal is, in percent, while it is under way.
    pub fn progress(&self, id: &str) -> Option<u32> {
        self.progress.lock().ok()?.get(id).copied()
    }
}

impl Drop for AppsPage {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

/// The action an app's button stands for.
fn action_of(app: &App, what: Do) -> Option<Action> {
    match what {
        Do::Uninstall => app.uninstall(),
        Do::Modify => app.modify().map(Action::Launch),
    }
}

/// Run an action on its own thread; when it ends, the worker is told.
fn spawn_action(app: App, what: Do, tx: Sender<Cmd>, progress: Progress, runner: Arc<dyn Runner>) {
    let Some(action) = action_of(&app, what) else { return };
    let packaged = matches!(action, Action::RemovePackage(_));
    live::spawn("apps-action", move || {
        live::init_mta();
        let result = act::perform(&*runner, &action, progress);
        let _ = tx.send(Cmd::Done { id: app.id, result, packaged });
    });
}

/// What the worker thread keeps between commands.
struct Worker {
    tx: Sender<Cmd>,
    wake: Wake,
    runner: Arc<dyn Runner>,
    shared: Arc<Mutex<HashMap<String, u32>>>,
    page: Page,
    /// Whether the Store apps are to be read again on this round.
    read_store: bool,
}

impl Worker {
    /// Take in one command; false when it says to stop.
    fn handle(&mut self, cmd: Cmd) -> bool {
        match cmd {
            Cmd::Stop => return false,
            Cmd::Refresh { packaged } => self.read_store |= packaged,
            Cmd::Act(app, what) => {
                if action_of(&app, what).is_some() {
                    self.page.busy.push(app.id.clone());
                    let (id, sink, wake) = (app.id.clone(), self.shared.clone(), self.wake.clone());
                    let progress: Progress = Arc::new(move |p| {
                        if let Ok(mut m) = sink.lock() {
                            m.insert(id.clone(), p);
                        }
                        wake();
                    });
                    spawn_action(*app, what, self.tx.clone(), progress, self.runner.clone());
                }
            }
            Cmd::Done { id, result, packaged } => {
                self.page.busy.retain(|b| *b != id);
                if let Ok(mut m) = self.shared.lock() {
                    m.remove(&id);
                }
                self.page.notice = match result {
                    Ok(note) if !note.is_empty() => Some(note),
                    Ok(_) => None,
                    Err(why) => Some(why),
                };
                self.read_store |= packaged;
            }
        }
        true
    }

    fn publish(&mut self, apps: Vec<App>, reading_packaged: bool) {
        self.page.apps = Arc::new(apps);
        self.page.reading_packaged = reading_packaged;
        // The mailbox keeps only the newest page, so a slow UI never queues them up.
    }
}

fn run(rx: Receiver<Cmd>, mut w: Worker, out: &Mailbox<Page>) {
    live::init_mta();
    let mut store_apps: Option<Vec<App>> = None;
    while let Ok(cmd) = rx.recv() {
        if !w.handle(cmd) {
            return;
        }
        // Several refreshes may have queued up (an installer touches many keys): one read.
        while let Ok(more) = rx.try_recv() {
            if !w.handle(more) {
                return;
            }
        }
        let classic_apps = read_classic();
        if store_apps.is_none() {
            // The classic programs are quick: show them while the Store apps are still read.
            w.publish(list::merge(vec![classic_apps.clone()]), true);
            out.put(w.page.clone());
            (w.wake)();
        }
        if w.read_store || store_apps.is_none() {
            store_apps = Some(packaged::list(&exists));
            w.read_store = false;
        }
        w.publish(list::merge(vec![classic_apps, store_apps.clone().unwrap_or_default()]), false);
        out.put(w.page.clone());
        (w.wake)();
    }
}

#[cfg(test)]
mod tests {
    /// Reads this machine and prints the list; run by hand (`cargo test -- --ignored --nocapture`).
    /// Only reads: nothing is started or removed.
    #[test]
    #[ignore]
    fn lists_this_machines_apps() {
        let started = std::time::Instant::now();
        crate::system::live::init_mta();
        let classic = super::read_classic();
        println!("{} classic in {:?}", classic.len(), started.elapsed());
        let store = super::packaged::list(&super::exists);
        println!("{} packaged, {:?} in all", store.len(), started.elapsed());
        let all = super::list::merge(vec![classic, store]);
        for a in &all {
            println!("{:<40} | {} | icon={}", a.name, a.detail(), a.icon.is_some());
        }
        println!("{} apps; with icon {}", all.len(), all.iter().filter(|a| a.icon.is_some()).count());
    }
}
