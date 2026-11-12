//! Changing the font Windows uses everywhere: File Explorer, Settings, Start, the taskbar, title
//! bars, menus and most apps.
//!
//! This crate derives from Fontman (<https://github.com/xnixy/fontman>, MIT, by the same author):
//! its logic copied and adapted, without its window. For each Segoe UI and Segoe UI Variable face
//! it builds a renamed copy of the chosen font's closest face, has GDI and DirectWrite check it,
//! installs it under its own `fontman-` name and points the Segoe UI entries at it; window text is
//! changed per user. Fontman's folders and file formats are kept, so a PC Fontman changed is taken
//! over as it is (see `settings`).
//!
//! - `fonts`: reading font files, the faces being replaced, building and checking replacements.
//! - `plan`: from a family to the replacement files and registry values.
//! - `machine`: the machine-wide part, with its backup and restore file; `guard` is what the
//!   administrator step accepts; `elevated` the step itself.
//! - `metrics`: window text. `actions`: what the Settings page does. `check`: the sign-in check.
//!
//! `SHELLMAN_SYSFONT_DRY=1` makes a dry run: stand-ins for the registry, the fonts folder and
//! window text under a stand-in root, and no administrator step. Unit tests are always dry.

pub mod actions;
pub mod autostart;
pub mod check;
pub mod elevated;
pub mod fonts;
pub mod guard;
pub mod machine;
pub mod metrics;
pub mod plan;
pub mod reg;
mod service;
pub mod settings;
pub mod system;
