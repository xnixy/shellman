//! The text font every shellman app wears: which fonts are installed, which one the look asks
//! for, and putting it into egui.
//!
//! Nothing is bundled or downloaded: only font files already installed on the machine are read.
//!
//! - `catalog`: pure parsing of the registry's font list into families and weights.
//! - `source`: the thin reader of the registry and the fonts folder.
//! - `choice`: the saved choice (Default or a family name).
//! - `tweak`: per-family vertical centring, as data.
//! - `install`: working out what to load, and loading it only when it changed.

mod catalog;
mod choice;
mod install;
mod source;
mod tweak;

pub use catalog::{Face, Family, Weight};
pub use choice::{FamilyName, FontChoice, SF_DEFAULT};
pub use install::{apply, default_label, install_saved};
pub use source::installed;
