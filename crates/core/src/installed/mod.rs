//! Installed programs, for Settings' Apps page: the classic ones from the Uninstall keys, the
//! Store (packaged) ones from the package manager, merged, searched, sorted, and uninstalled or
//! modified the way Windows does it.
//!
//! `app`, `classic`, `cmdline`, `format` and `list` are pure: the registry comes in through the
//! `registry` trait and the disk as a closure, so they are tested without the machine. `act` is
//! what may be done to an app, behind a `Runner` so tests (and a dry run) never start anything;
//! `win` and `packaged` are the Windows side; `watch` hears the Uninstall keys change; `worker`
//! ties them to a page.

pub mod act;
pub mod app;
pub mod classic;
pub mod cmdline;
pub mod format;
pub mod list;
pub mod packaged;
pub mod watch;
pub mod win;
pub mod worker;
