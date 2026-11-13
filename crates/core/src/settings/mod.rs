//! The settings every shellman app shares, one type per file under `%APPDATA%\shellman`.
//!
//! Each type has `#[serde(default)]` on it, so a file written by an older version (or edited by
//! hand) still loads, and a new option is one new field with a default. Appearance lives in
//! `shellman-ui`, since it needs the theme; the rest is here so the settings app and the apps it
//! configures agree on one definition.

pub mod bar;
pub mod desktop;
pub mod dock;
pub mod launcher;
pub mod shell;
