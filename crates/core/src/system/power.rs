//! Lock, sleep, sign out, restart and shut down: what the Start menu's power button does.
//!
//! Every call here ends or suspends the session, so callers ask the user first; this module
//! only carries the request out. `plan` is the pure part (which call, with which flags) and is
//! what the tests check; `perform` makes the call.

use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
    SE_SHUTDOWN_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::{
    ExitWindowsEx, LockWorkStation, EWX_LOGOFF, EWX_POWEROFF, EWX_REBOOT, EXIT_WINDOWS_FLAGS,
    SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_OTHER, SHTDN_REASON_MINOR_OTHER,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Lock,
    Sleep,
    SignOut,
    Restart,
    ShutDown,
}

/// The Windows call an action comes down to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    LockWorkStation,
    SetSuspendState,
    /// `ExitWindowsEx` with these flags; restart and shut down also need the shutdown privilege.
    ExitWindows { flags: u32, privilege: bool },
}

/// Which call carries out `action`. Apps are asked to close, never forced: one with unsaved
/// work can still hold the restart up and ask its user.
pub fn plan(action: Action) -> Call {
    match action {
        Action::Lock => Call::LockWorkStation,
        Action::Sleep => Call::SetSuspendState,
        Action::SignOut => Call::ExitWindows { flags: EWX_LOGOFF.0, privilege: false },
        Action::Restart => Call::ExitWindows { flags: EWX_REBOOT.0, privilege: true },
        Action::ShutDown => Call::ExitWindows { flags: EWX_POWEROFF.0, privilege: true },
    }
}

/// Carry out `action`. False when Windows refused. Blocks only briefly (Windows queues the
/// request), but callers should still make it from a worker or a button's click, never a loop.
pub fn perform(action: Action) -> bool {
    // SAFETY: plain FFI calls; the token handle is closed after use.
    unsafe {
        match plan(action) {
            Call::LockWorkStation => LockWorkStation().is_ok(),
            Call::SetSuspendState => SetSuspendState(false, false, false),
            Call::ExitWindows { flags, privilege } => {
                if privilege && !enable_shutdown_privilege() {
                    return false;
                }
                let reason = SHTDN_REASON_MAJOR_OTHER | SHTDN_REASON_MINOR_OTHER | SHTDN_REASON_FLAG_PLANNED;
                ExitWindowsEx(EXIT_WINDOWS_FLAGS(flags), reason).is_ok()
            }
        }
    }
}

/// Every user may shut the machine down, but the privilege starts disabled in each process.
unsafe fn enable_shutdown_privilege() -> bool {
    let mut token = HANDLE::default();
    if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token).is_err() {
        return false;
    }
    let mut luid = LUID::default();
    let ok = LookupPrivilegeValueW(None, SE_SHUTDOWN_NAME, &mut luid).is_ok() && {
        let wanted = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }],
        };
        AdjustTokenPrivileges(token, false, Some(&wanted), 0, None, None).is_ok()
    };
    let _ = CloseHandle(token);
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_action_has_its_call() {
        assert_eq!(plan(Action::Lock), Call::LockWorkStation);
        assert_eq!(plan(Action::Sleep), Call::SetSuspendState);
        assert_eq!(plan(Action::SignOut), Call::ExitWindows { flags: 0, privilege: false });
        assert_eq!(plan(Action::Restart), Call::ExitWindows { flags: 2, privilege: true });
        assert_eq!(plan(Action::ShutDown), Call::ExitWindows { flags: 8, privilege: true });
    }

    #[test]
    fn nothing_is_forced() {
        const EWX_FORCE: u32 = 4;
        const EWX_FORCEIFHUNG: u32 = 16;
        for a in [Action::SignOut, Action::Restart, Action::ShutDown] {
            if let Call::ExitWindows { flags, .. } = plan(a) {
                assert_eq!(flags & (EWX_FORCE | EWX_FORCEIFHUNG), 0);
            }
        }
    }

    #[test]
    fn the_privilege_can_be_enabled() {
        // Enabling it changes nothing on its own; only ExitWindowsEx would act on it.
        assert!(unsafe { enable_shutdown_privilege() });
    }
}
