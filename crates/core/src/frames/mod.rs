//! Window frames that match shellman's look: other apps' title bars and borders take the
//! theme's dark or light, its glass tone, its text colour and rounded corners, and the focused
//! window gets an accent border.
//!
//! The only thing done to another process's window is `DwmSetWindowAttribute` with a handful of
//! attributes (see `apply`). Nothing is injected, subclassed or moved. The host (the desktop
//! app) pumps messages, because the WinEvent hooks deliver on its thread; nothing runs between
//! events.
//!
//! Split by concern: `settings` (frames.json), `colors` (palette, COLORREF, which border),
//! `eligible` (which windows), `mode` (Windows' light/dark), `apply` (the DWM calls) and
//! `service` (hooks and bookkeeping). All but the last two are pure.

mod apply;
mod colors;
mod eligible;
mod mode;
mod service;
mod settings;

pub use colors::{border_for, colorref, Border, Palette, Rgb, COLOR_DEFAULT};
pub use eligible::{eligible, is_own_exe, WindowFacts};
pub use mode::{force_light, system_is_light};
pub use service::{reset_all_windows, start, stop, update};
pub use settings::{is_excluded, FramesSettings, NAME};
