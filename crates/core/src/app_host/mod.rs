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

/// The user's setting, under the environment's `off`.
fn enabled() -> bool {
    std::env::var(ENV).map_or(true, |v| v.trim() != "off")
        && crate::settings::shell::ShellSettings::load().modern_apps
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
}
