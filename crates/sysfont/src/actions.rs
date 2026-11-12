//! What the window does: read the fonts, put a choice in place, and put Windows back.
//!
//! The machine-wide step goes through an [`Elevator`] the caller gives (shellman's administrator
//! helper), except in a dry run, where it runs here against the stand-ins and the elevator is
//! never called.

use crate::elevated::{self, Step};
use crate::fonts::catalog::{self, Family, Origin};
use crate::fonts::segoe::{self, Slot};
use crate::machine;
use crate::metrics::{self, TextSizes};
use crate::plan::{self, FaceChoice, Layers, Request, Tune};
use crate::settings;
use std::collections::BTreeMap;

pub struct Catalog {
    pub families: Vec<Family>,
    pub slots: Vec<Slot>,
}

/// Every family that can be chosen, and every Segoe UI face that would be replaced.
pub fn load_catalog() -> Result<Catalog, String> {
    let slots = segoe::slots(&machine::originals())?;
    let families = catalog::families(&catalog::installed_files(), &settings::added_files());
    Ok(Catalog { families, slots })
}

/// The family called `name`, preferring an installed one over an added file of the same name.
pub fn find_family<'a>(families: &'a [Family], name: &str) -> Option<&'a Family> {
    let matching = || {
        families
            .iter()
            .filter(|f| f.name.eq_ignore_ascii_case(name))
    };
    matching()
        .find(|f| f.origin == Origin::Installed)
        .or_else(|| matching().next())
}

/// The face name window text should ask for. An installed family is asked for by its own name.
/// A font that is only added to Fontman is not installed, so window text can reach it only
/// through the replaced Segoe UI.
pub fn window_text_face(family: &Family, layers: &Layers) -> Option<String> {
    match family.origin {
        Origin::Installed => family.regular().map(|face| face.legacy_family.clone()),
        Origin::Added => layers.segoe.then(|| "Segoe UI".to_string()),
    }
}

/// Everything about one choice.
pub struct Choice<'a> {
    pub family: &'a Family,
    pub slots: &'a [Slot],
    pub overrides: &'a BTreeMap<String, FaceChoice>,
    pub tune: Tune,
    pub layers: Layers,
    pub text_sizes: TextSizes,
}

impl Choice<'_> {
    pub fn request(&self) -> Request<'_> {
        Request {
            family: self.family,
            slots: self.slots,
            overrides: self.overrides,
            tune: self.tune,
            layers: self.layers,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Building(usize, usize),
    /// Waiting for the administrator prompt and the machine-wide step.
    Permission,
    WindowText,
}

/// The result of putting a choice in place.
/// Runs the machine-wide step as administrator and returns its message. Never called in a dry run.
pub type Elevator<'a> = &'a dyn Fn(&Step) -> Result<String, String>;

/// Run `step`: here in a dry run, through `elevate` otherwise.
pub fn machine_step(step: Step, elevate: Elevator) -> Result<String, String> {
    if settings::dry_run() {
        elevated::run(&step)
    } else {
        elevate(&step)
    }
}

pub struct Applied {
    pub message: String,
    pub needs_restart: bool,
}

/// Make Windows match `choice`: every part switched on gets the font, every part switched off
/// goes back to Windows' own.
pub fn apply(choice: &Choice, progress: &dyn Fn(Progress), elevate: Elevator) -> Result<Applied, String> {
    let layers = choice.layers;
    let face = if layers.window_text {
        Some(window_text_face(choice.family, &layers).ok_or_else(|| {
            format!(
                "{} is not installed, so window text can use it only when Apps is on too. Install the font, or turn Apps on.",
                choice.family.name
            )
        })?)
    } else {
        None
    };
    let mut needs_restart = false;
    if layers.machine() {
        let generation = plan::new_generation();
        let staging = settings::staging_dir().join(&generation);
        let request = choice.request();
        let plan = plan::build(&request, &staging, &generation, |done, total| {
            progress(Progress::Building(done, total))
        });
        let plan = match plan {
            Ok(plan) => plan,
            Err(err) => {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(err);
            }
        };
        let plan_path = machine::plan_path();
        settings::write_json(&plan_path, &plan)?;
        progress(Progress::Permission);
        let result = machine_step(Step::Apply { generation }, elevate);
        let _ = std::fs::remove_dir_all(&staging);
        let _ = std::fs::remove_file(&plan_path);
        result?;
        needs_restart = true;
    } else if machine::status().current.is_some() {
        progress(Progress::Permission);
        machine_step(Step::Restore, elevate)?;
        needs_restart = true;
    }
    progress(Progress::WindowText);
    match &face {
        Some(face) => metrics::apply(face, choice.text_sizes)?,
        None if metrics::has_backup() => metrics::restore()?,
        None => {}
    }
    let name = &choice.family.name;
    let message = if needs_restart && layers.machine() {
        format!("{name} is ready. Restart Windows to see it everywhere.")
    } else if needs_restart {
        "Segoe UI is back for apps. Restart Windows to finish.".to_string()
    } else if face.is_some() {
        format!("Window text now uses {name}.")
    } else {
        "Nothing is switched on, so Windows keeps its own font.".to_string()
    };
    Ok(Applied {
        message,
        needs_restart,
    })
}

/// Put every part of Windows back to its own font.
pub fn restore_all(window_text: bool, machine_wide: bool, elevate: Elevator) -> Result<Applied, String> {
    if window_text {
        metrics::restore()?;
    }
    let mut needs_restart = false;
    if machine_wide && needs_machine_restore() {
        machine_step(Step::Restore, elevate)?;
        needs_restart = true;
    }
    Ok(Applied {
        message: if needs_restart {
            "Segoe UI is back. Restart Windows to see it everywhere.".into()
        } else {
            "Windows uses its own font again.".into()
        },
        needs_restart,
    })
}

/// Whether anything machine-wide is Fontman's: a font in place, or values left behind.
pub fn needs_machine_restore() -> bool {
    machine::status().current.is_some()
        || machine::font_values()
            .iter()
            .any(|(name, data)| {
                name.starts_with(plan::EXTRA_PREFIX)
                    || data.to_ascii_lowercase().starts_with(catalog::OWN_PREFIX)
            })
}

/// Put an earlier choice back, from the files saved when it was made.
pub fn reuse(generation: &str, elevate: Elevator) -> Result<Applied, String> {
    let message = machine_step(Step::Reuse { generation: generation.to_string() }, elevate)?;
    Ok(Applied {
        message,
        needs_restart: true,
    })
}
