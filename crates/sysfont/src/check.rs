//! The check at sign-in (Fontman's `check`): window text comes back if a theme change reset it,
//! and the caller opens the Fonts page if a Windows update put Segoe UI back.

use crate::{machine, metrics};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Checked {
    /// Window text had been reset and was set again.
    pub window_text_again: bool,
    /// Something put Segoe UI back behind our back: the Fonts page should open with "Apply again".
    pub segoe_back: bool,
}

/// Run the check once.
pub fn at_signin() -> Checked {
    Checked {
        window_text_again: metrics::reapply_if_reset().unwrap_or(false),
        segoe_back: machine::status().drift,
    }
}
