//! Nothing in this DLL may take the app down. Every entry point runs its body through `guard`,
//! which turns a panic into a failure code; the app then sees an ordinary COM error.

use std::panic::{catch_unwind, AssertUnwindSafe};
use windows::Win32::Foundation::E_UNEXPECTED;
use windows_core::{Error, Result};

/// Run `f`; a panic becomes `E_UNEXPECTED`.
pub fn guard<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(_) => {
            crate::log::line("a call panicked; returned E_UNEXPECTED");
            Err(Error::from(E_UNEXPECTED))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_becomes_an_error() {
        let r: Result<u32> = guard(|| panic!("boom"));
        assert_eq!(r.unwrap_err().code(), E_UNEXPECTED);
        assert_eq!(guard(|| Ok(7)).unwrap(), 7);
    }
}
