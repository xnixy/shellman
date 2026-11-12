//! Startup items (programs that start at sign-in): which there are and whether each is on, shared
//! by the session, which runs them, and Settings, which edits the on/off state.
//!
//! `items` and `approved` read the Run keys, the Startup folders and the StartupApproved values
//! through the `registry` trait, so they are tested without the machine. `toggle` writes the
//! on/off value; `rows` names and sorts items for the Settings list; `describe`, `win` and
//! `watch` are the Windows side; `worker` ties them to a page.

pub mod approved;
pub mod describe;
pub mod items;
pub mod rows;
pub mod toggle;
pub mod watch;
pub mod win;
pub mod worker;
