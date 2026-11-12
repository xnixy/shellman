//! Work that needs administrator rights, done by a small helper (`shellman-elevate.exe`) that is
//! started only when asked, through the standard Windows UAC prompt, and ends when it is done.
//! `ops` is the fixed list of what the helper will do and how it is spelled on its command line;
//! `launch` is the caller's side. Nothing here runs in the background.

mod launch;
pub mod ops;

pub use launch::run;

/// The helper's exit codes. Zero is success; the rest say why nothing was done.
pub mod exit {
    pub const DONE: u32 = 0;
    /// The command line was not one of the known operations.
    pub const BAD_ARGS: u32 = 2;
    /// The helper was started without administrator rights, so it did nothing.
    pub const NOT_ELEVATED: u32 = 3;
    /// The operation ran and Windows refused or failed it.
    pub const FAILED: u32 = 4;
}

/// How an elevated run ended, for the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// The user said no at the UAC prompt. Not an error.
    Cancelled,
    /// `shellman-elevate.exe` is not beside the running exe.
    HelperMissing,
    Failed(String),
}

impl Outcome {
    /// What to tell the user; None when there is nothing to say (done, or they declined).
    pub fn message(&self) -> Option<String> {
        match self {
            Outcome::Done | Outcome::Cancelled => None,
            Outcome::HelperMissing => Some("The administrator helper (shellman-elevate.exe) was not found.".into()),
            Outcome::Failed(why) => Some(why.clone()),
        }
    }
}

/// The outcome of a helper that ended with `code`.
pub fn outcome_of_exit(code: u32) -> Outcome {
    match code {
        exit::DONE => Outcome::Done,
        exit::BAD_ARGS => Outcome::Failed("The administrator helper did not accept the request.".into()),
        exit::NOT_ELEVATED => Outcome::Failed("The administrator helper did not get administrator rights.".into()),
        exit::FAILED => Outcome::Failed("Windows did not allow the change.".into()),
        other => Outcome::Failed(format!("The administrator helper ended with code {other}.")),
    }
}

/// ERROR_CANCELLED: what ShellExecuteEx reports when the UAC prompt is declined.
pub const ERROR_CANCELLED: u32 = 1223;
/// ERROR_FILE_NOT_FOUND and ERROR_PATH_NOT_FOUND.
pub const ERROR_FILE_NOT_FOUND: u32 = 2;
pub const ERROR_PATH_NOT_FOUND: u32 = 3;

/// The outcome of a launch that failed with the Win32 error `err`.
pub fn outcome_of_launch_error(err: u32) -> Outcome {
    match err {
        ERROR_CANCELLED => Outcome::Cancelled,
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Outcome::HelperMissing,
        other => Outcome::Failed(format!("The administrator helper could not start (error {other}).")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_map_to_outcomes() {
        assert_eq!(outcome_of_exit(0), Outcome::Done);
        for code in [2, 3, 4, 99] {
            assert!(matches!(outcome_of_exit(code), Outcome::Failed(_)), "{code}");
        }
    }

    #[test]
    fn declining_the_prompt_is_quiet() {
        assert_eq!(outcome_of_launch_error(1223), Outcome::Cancelled);
        assert_eq!(Outcome::Cancelled.message(), None);
        assert_eq!(Outcome::Done.message(), None);
        assert_eq!(outcome_of_launch_error(2), Outcome::HelperMissing);
        assert!(outcome_of_launch_error(5).message().is_some());
    }
}
