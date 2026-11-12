//! The machine-wide step: what runs as administrator. The window never runs elevated; it asks
//! shellman's administrator helper (`shellman-elevate`) to run one [`Step`], and the helper calls
//! [`run`] here. In a dry run the window calls [`run`] itself, against the stand-ins.
//!
//! A step names no paths: the plan is read from its fixed place (`machine::plan_path`) and the
//! outcome written to another (`outcome_path`), both computed here.

use crate::machine;
use crate::settings;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Install the faces built for this generation and point the Segoe UI entries at them.
    Apply { generation: String },
    /// Point the entries back at Windows' own files.
    Restore,
    /// Put a choice from the history back.
    Reuse { generation: String },
}

/// Do `step` in this process.
pub fn run(step: &Step) -> Result<String, String> {
    match step {
        Step::Apply { generation } => machine::apply_staged(generation),
        Step::Restore => machine::restore(),
        Step::Reuse { generation } => machine::reuse(generation),
    }
}

/// Where the helper leaves its message, for the window to read.
pub fn outcome_path() -> PathBuf {
    settings::local_dir().join("outcome.json")
}

/// Run `step` and leave its message in [`outcome_path`]. What the helper does.
pub fn run_and_report(step: &Step) -> bool {
    let result = run(step);
    machine::write_outcome(&outcome_path(), &result);
    result.is_ok()
}

/// The message the helper left, taken (the file is removed).
pub fn take_outcome() -> Option<machine::Outcome> {
    let path = outcome_path();
    let outcome = settings::read_json(&path);
    let _ = std::fs::remove_file(&path);
    outcome
}
