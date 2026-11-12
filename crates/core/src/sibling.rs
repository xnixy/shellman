//! Starting another shellman app: its exe sits next to the running one.

/// Start `shellman-<name>.exe` from this program's folder. False when it is not there or would
/// not start. Apps that keep one window (the launcher, settings) bring the running one forward
/// instead of opening a second.
pub fn start(name: &str) -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(format!("shellman-{name}.exe"))))
        .filter(|path| path.exists())
        .is_some_and(|path| std::process::Command::new(path).spawn().is_ok())
}
