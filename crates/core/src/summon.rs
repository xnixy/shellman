//! Summoning the resident launcher and control centre from another shellman process: set the
//! named event the resident one waits on (see their `summon` modules), and start the exe only
//! when none is running.
//!
//! Starting `shellman-launcher.exe` again also summons it (the new process finds the resident
//! one, sets the event and exits), but that is a whole process start on every click: the loader,
//! the antivirus' look at an unsigned image and static initialisation, 10 to 20 ms when all is
//! warm and far more when not. Setting the event is a few microseconds.

use windows::core::HSTRING;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{OpenEventW, SetEvent, EVENT_MODIFY_STATE};
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};

fn instance() -> String {
    std::env::var("SHELLMAN_INSTANCE").unwrap_or_default()
}

/// The launcher's event: `Local\shellman-launcher-summon<instance>`.
fn launcher_event(instance: &str) -> String {
    format!("Local\\shellman-launcher-summon{instance}")
}

/// The control centre's event: `Local\shellman-control-summon[-<instance>]`.
fn control_event(i: &str) -> String {
    if i.is_empty() {
        "Local\\shellman-control-summon".into()
    } else {
        format!("Local\\shellman-control-summon-{i}")
    }
}

/// Set the named event; false when nobody waits on it.
fn signal(name: &str) -> bool {
    unsafe {
        // The caller just got the click or key, so it may hand the foreground on.
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        match OpenEventW(EVENT_MODIFY_STATE, false, &HSTRING::from(name)) {
            Ok(event) => {
                let ok = SetEvent(event).is_ok();
                let _ = CloseHandle(event);
                ok
            }
            Err(_) => false,
        }
    }
}

/// Show the launcher, or hide it when it is showing; starts it when it is not running.
pub fn launcher() -> bool {
    crate::timing::mark("summon: launcher asked");
    signal(&launcher_event(&instance())) || crate::sibling::start("launcher")
}

/// Show the control centre, or hide it when it is showing; starts it when it is not running.
pub fn control() -> bool {
    crate::timing::mark("summon: control centre asked");
    signal(&control_event(&instance())) || crate::sibling::start("control")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_the_residents() {
        // The launcher appends the instance without a dash, the control centre with one.
        assert_eq!(launcher_event(""), "Local\\shellman-launcher-summon");
        assert_eq!(control_event(""), "Local\\shellman-control-summon");
        assert_eq!(launcher_event("t"), "Local\\shellman-launcher-summont");
        assert_eq!(control_event("t"), "Local\\shellman-control-summon-t");
    }

    #[test]
    fn no_resident_means_no_signal() {
        assert!(!signal("Local\\shellman-summon-test-nobody-waits"));
    }
}
