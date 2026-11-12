//! The Recycle Bin without Explorer: what is in it, restoring and deleting items, emptying it,
//! and knowing when it changes.
//!
//! Deleting *into* the bin is `fsops::shell_op(Op::Recycle)`. Everything here talks to the bin
//! through the shell namespace, which shell32 serves in-process, so none of it needs
//! `explorer.exe` to be running. Each call blocks, so callers run it on a worker thread.

mod act;
mod com;
mod items;
mod notify;
mod state;

pub use act::{delete, restore};
pub use items::{list, BinItem, Handle};
pub use notify::watch_dirs;
pub use state::{empty, state, summary, BinState};

#[cfg(test)]
mod live_test;
