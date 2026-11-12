//! The Startup page's worker. It sleeps until asked to read the items or to switch one, does the
//! slow part (the disk, the registry, and the administrator helper with its UAC prompt) off the UI
//! thread, and publishes the newest list. A watcher asks it to read again when the Run keys or
//! the Startup folders change.

use super::items::Item;
use super::rows::{build, Row};
use super::toggle::{apply, Step};
use super::watch::RunWatch;
use super::{describe, items, win};
use crate::elevate::{self, ops::Op, Outcome};
use crate::registry::Real;
use crate::system::live::{self, Mailbox, Wake};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// What the page shows.
#[derive(Default, Clone)]
pub struct Page {
    pub rows: Vec<Row>,
    /// Why the last switch did not work, when it did not (a refused UAC prompt is not one).
    pub notice: Option<String>,
}

enum Cmd {
    Refresh,
    Toggle(Box<Item>, bool),
    Stop,
}

pub struct StartupPage {
    tx: Sender<Cmd>,
    mailbox: Arc<Mailbox<Page>>,
    _watch: RunWatch,
}

/// The items as the page lists them, read now.
pub fn read_rows() -> Vec<Row> {
    let found = items::collect(&Real, &win::startup_files(), &win::expand, true);
    build(found, &describe::probe)
}

impl StartupPage {
    /// Start following; `wake` is called whenever there is a new page. The first reading follows
    /// at once.
    pub fn start(wake: Wake) -> Self {
        let (tx, rx) = channel();
        let mailbox = Arc::new(Mailbox::default());
        let out = mailbox.clone();
        live::spawn("startup-page", move || run(rx, &out, &wake));
        let _ = tx.send(Cmd::Refresh);
        let for_watch = tx.clone();
        let watch = RunWatch::start(move || {
            let _ = for_watch.send(Cmd::Refresh);
        });
        Self { tx, mailbox, _watch: watch }
    }

    pub fn take(&self) -> Option<Page> {
        self.mailbox.take()
    }

    pub fn refresh(&self) {
        let _ = self.tx.send(Cmd::Refresh);
    }

    /// Turn an item on or off.
    pub fn set_enabled(&self, item: &Item, enable: bool) {
        let _ = self.tx.send(Cmd::Toggle(Box::new(item.clone()), enable));
    }
}

impl Drop for StartupPage {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

/// The switch itself: directly, and through the elevated helper when Windows says no to the
/// machine-wide key. `Err` is a message for the user.
fn switch(item: &Item, enable: bool) -> Result<(), String> {
    match apply(&Real, item, enable, win::now_filetime())? {
        Step::Done => Ok(()),
        Step::NeedsAdmin => {
            let area = item.area().ok_or("This item has no switch.")?;
            let Some(op) = Op::startup_approve(enable, area, &item.name) else {
                return Err("This item's name is one the administrator helper does not accept.".into());
            };
            match elevate::run(&op) {
                Outcome::Done | Outcome::Cancelled => Ok(()),
                other => Err(other.message().unwrap_or_default()),
            }
        }
    }
}

fn run(rx: Receiver<Cmd>, out: &Mailbox<Page>, wake: &Wake) {
    live::init_mta();
    let mut notice = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Stop => return,
            Cmd::Refresh => {}
            Cmd::Toggle(item, enable) => notice = switch(&item, enable).err(),
        }
        // Several refreshes may have queued up (a switch changes keys the watcher hears): one read.
        while let Ok(more) = rx.try_recv() {
            match more {
                Cmd::Stop => return,
                Cmd::Toggle(item, enable) => notice = switch(&item, enable).err(),
                Cmd::Refresh => {}
            }
        }
        out.put(Page { rows: read_rows(), notice: notice.clone() });
        wake();
    }
}

#[cfg(test)]
mod tests {
    /// Reads this machine and prints the list; run by hand (`cargo test -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn lists_this_machines_items() {
        let rows = super::read_rows();
        for r in &rows {
            println!("{:<7} {:<28} missing={:<5} {}", r.item.enabled, r.title, r.missing, r.detail());
        }
        println!("{} rows", rows.len());
    }
}
