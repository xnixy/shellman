//! One session per logon session. `SHELLMAN_INSTANCE` is added to the name, so a test copy runs
//! beside the real one.

use windows::core::HSTRING;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;

/// The mutex's name for an instance (empty for the real one).
pub fn name(instance: &str) -> String {
    if instance.is_empty() {
        "Local\\shellman-session".to_string()
    } else {
        format!("Local\\shellman-session-{instance}")
    }
}

/// Claim the session. `None` when another copy already holds it; keep the handle until exit.
pub fn claim() -> Option<HANDLE> {
    let instance = std::env::var("SHELLMAN_INSTANCE").unwrap_or_default();
    let mutex = unsafe { CreateMutexW(None, false, &HSTRING::from(name(&instance))) }.ok()?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return None;
    }
    Some(mutex)
}

#[cfg(test)]
mod tests {
    #[test]
    fn instance_gets_its_own_name() {
        assert_eq!(super::name(""), "Local\\shellman-session");
        assert_eq!(super::name("t1"), "Local\\shellman-session-t1");
    }
}
