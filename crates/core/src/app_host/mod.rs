//! Keeping modern apps working without Explorer.
//!
//! Settings, Calculator, the Store and every `ms-settings:` link are immersive (CoreWindow) apps,
//! and those only open while something hosts the immersive shell. Explorer does; without it,
//! activation times out after ~45 s. Windows ships that host on its own as
//! `System32\ShellAppRuntime.exe` (the app half of Shell Launcher's custom shell). Started with no
//! arguments while no process owns the shell window, it becomes the shell window's owner within
//! a few seconds, creates no taskbar, and modern apps open in a fraction of a second.
//!
//! The supervisor here starts it, watches it, and recovers when it dies: Winlogon then starts
//! Explorer as the shell, and that one Explorer is stopped before the host is started again. It
//! gives up after a few failures in a short time. shellman itself never claims the shell window.
//! On shellman's exit the host is left running: it does no harm, and ending it any way but the
//! clean one would bring Explorer back.
//!
//! The host is undocumented as a program of its own, so it may be missing or behave differently
//! on other builds: then nothing is started and modern apps simply stay unavailable, as before.

mod decide;
mod log;
mod supervise;
mod win;

pub use win::explorer_is_shell;
/// For the session's safety net, which must free the shell window before it starts Explorer.
pub use win::{quit_shell, shell_window_pid};

/// `off` never runs the host; `on` runs it even in a test copy (for tests, with
/// `SHELLMAN_CONFIG_DIR` pointing somewhere else). Unset: the `modern_apps` setting decides.
pub const ENV: &str = "SHELLMAN_APP_HOST";

/// Start supervising the host on a thread of its own, if this process is the real shell's
/// (`no_shell` is its test mode). True when the supervisor runs.
pub fn start(no_shell: bool) -> bool {
    let env = std::env::var(ENV).ok();
    let instance = std::env::var("SHELLMAN_INSTANCE").is_ok_and(|s| !s.is_empty());
    if !allowed(env.as_deref(), instance, no_shell) {
        return false;
    }
    std::thread::Builder::new().name("app host".into()).spawn(supervise::run).is_ok()
}

/// Whether the host may run at all in this process; the user's setting is read later, and live.
fn allowed(env: Option<&str>, instance: bool, no_shell: bool) -> bool {
    match env.map(str::trim) {
        Some("off") => false,
        Some("on") => true,
        _ => !instance && !no_shell,
    }
}

/// The user's setting, or a request for this sign-in ([`want_now`]), under the environment's
/// `off`.
fn enabled() -> bool {
    std::env::var(ENV).map_or(true, |v| v.trim() != "off")
        && (crate::settings::shell::ShellSettings::load().modern_apps || wanted_now())
}

/// The file a request for the host is written to, in the settings folder (which the supervisor
/// watches). It holds the sign-in it was made in, so it means nothing after the next sign-in.
const WANTED: &str = "app-host-wanted";

/// This sign-in's id: the logon session's LUID, the same for every process of it and new at
/// every sign-in.
fn logon_id() -> Option<u64> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenStatistics, TOKEN_QUERY, TOKEN_STATISTICS};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
        let mut stats = TOKEN_STATISTICS::default();
        let mut len = 0u32;
        let got = GetTokenInformation(token, TokenStatistics, Some(&mut stats as *mut _ as *mut _), size_of::<TOKEN_STATISTICS>() as u32, &mut len);
        let _ = CloseHandle(token);
        got.ok()?;
        let id = stats.AuthenticationId;
        Some((u64::from(id.HighPart as u32) << 32) | u64::from(id.LowPart))
    }
}

/// Whether someone asked for the host in this sign-in.
fn wanted_now() -> bool {
    let Some(dir) = crate::config::config_dir() else { return false };
    let Ok(text) = std::fs::read_to_string(dir.join(WANTED)) else { return false };
    logon_id().is_some_and(|id| text.trim() == id.to_string())
}

/// Ask for the host for the rest of this sign-in, when Windows' Settings (or another modern app)
/// is wanted now and the "Keep Settings, Calculator and the Store working" setting is off: so the
/// host costs nothing until it is needed. The desktop's supervisor sees the request and starts it.
/// Then waits, up to `timeout`, for modern apps to be able to open. True when they can.
pub fn want_now(timeout: std::time::Duration) -> bool {
    if modern_apps_available() {
        return true;
    }
    let (Some(dir), Some(id)) = (crate::config::config_dir(), logon_id()) else { return false };
    let _ = std::fs::create_dir_all(&dir);
    if std::fs::write(dir.join(WANTED), id.to_string()).is_err() {
        return false;
    }
    let until = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < until {
        if modern_apps_available() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    modern_apps_available()
}

/// Whether modern apps can be made to open: they can now, or the host is on this system and
/// [`want_now`] starts it on demand. What decides whether a "Windows Settings" button is offered.
pub fn modern_apps_reachable() -> bool {
    modern_apps_available() || win::host_path().is_some()
}

/// Whether modern apps (and `ms-settings:` links) can open right now: Explorer or the host is
/// hosting the immersive shell. A couple of window lookups, cheap enough to ask at every showing.
pub fn modern_apps_available() -> bool {
    win::immersive_shell() || win::explorer_is_shell()
}

#[cfg(test)]
mod tests {
    use super::allowed;

    #[test]
    fn only_the_real_shell_runs_the_host_unless_told() {
        assert!(allowed(None, false, false));
        assert!(!allowed(None, true, false), "a test copy");
        assert!(!allowed(None, false, true), "--no-shell");
        assert!(allowed(Some("on"), true, true));
        assert!(!allowed(Some("off"), false, false));
        assert!(allowed(Some(""), false, false));
    }

    #[test]
    fn a_sign_in_has_an_id_that_stays_the_same() {
        let a = super::logon_id();
        assert!(a.is_some_and(|id| id != 0));
        assert_eq!(a, super::logon_id());
    }
}
