//! Picker mode: `shellman-files --pick <request file>` runs Files as the window other apps get
//! when they ask to open or save a file or pick a folder (see `shellman_pick` for what is said
//! between them, and `app::picker` for the window's side).
//!
//! The parts here are pure and tested on their own: file type patterns (`filter`), names and the
//! default extension (`name`), what the main button does (`decide`) and is called (`button`),
//! and where the picker opens (`start`). `host` holds the few calls about the asking app.

pub mod button;
pub mod decide;
pub mod filter;
pub mod host;
pub mod name;
pub mod start;

use shellman_pick::Request;
use std::path::PathBuf;
use std::sync::atomic::AtomicI32;
use std::sync::Arc;

/// One picker run: what was asked, the file it was asked in, and the exit code the process
/// leaves with (set by the window, read by `main` once the window has closed).
pub struct Session {
    pub req: Request,
    pub path: PathBuf,
    pub exit: Arc<AtomicI32>,
}
