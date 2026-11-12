//! `shellman_pickhook.dll`: an in-process COM server for Windows' file dialogs
//! (`CLSID_FileOpenDialog`, `CLSID_FileSaveDialog`) that shows the Files picker
//! (`shellman-files --pick`) instead, and the real dialog whenever the picker cannot stand in.
//!
//! Nothing registers it: see README.md for what registering would mean, and its risks.
//!
//! The parts: `factory` (the exports and the class factory), `dialog` (the two COM classes),
//! `inner` (what they do: pass everything to a real dialog, note what matters, and on `Show`
//! run the picker or the real dialog), `collected` (the app's settings as a picker request, and
//! what the picker cannot do; pure and tested), `launch` (running the picker while the app's
//! window stays alive), `real` (the real dialog, straight from comdlg32), `items` (paths and
//! shell items), `guard` (no panic ever reaches the app) and `log`.

pub mod collected;
mod dialog;
pub mod factory;
mod guard;
mod inner;
mod items;
pub mod launch;
pub mod log;
pub mod real;

pub use factory::{class_object, DllCanUnloadNow, DllGetClassObject};
pub use inner::OFF_VAR;
