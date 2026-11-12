#![windows_subsystem = "windows"]
//! Shellman's administrator helper. Other apps start it through the UAC prompt (`runas`) to do
//! one thing that needs administrator rights, and wait for its exit code. It takes only the
//! fixed operations in `shellman_core::elevate::ops`, does nothing when it was not started
//! elevated, has no window, and ends when the operation is done.

mod handle;
mod token;

use shellman_core::elevate::exit;
use shellman_core::elevate::ops;

/// What the helper ends with for `args`: parsed first, then refused unless `elevated`, then run.
fn decide(args: &[String], elevated: bool, run: impl FnOnce(&ops::Op) -> bool) -> u32 {
    let Ok(op) = ops::parse(args) else { return exit::BAD_ARGS };
    if !elevated {
        return exit::NOT_ELEVATED;
    }
    if run(&op) {
        exit::DONE
    } else {
        exit::FAILED
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(decide(&args, token::is_elevated(), handle::run) as i32);
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "{0.0.0.00000000}.{8a4f1c2e-0b3d-4e5f-9a6b-7c8d9e0f1a2b}";

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn bad_arguments_do_nothing() {
        let never = |_: &ops::Op| -> bool { panic!("must not run") };
        assert_eq!(decide(&args(&["cmd.exe"]), true, never), exit::BAD_ARGS);
        assert_eq!(decide(&args(&[]), true, never), exit::BAD_ARGS);
        assert_eq!(decide(&args(&["sound-device", "enable", ID, "x"]), true, never), exit::BAD_ARGS);
    }

    #[test]
    fn without_elevation_nothing_runs() {
        let never = |_: &ops::Op| -> bool { panic!("must not run") };
        assert_eq!(decide(&args(&["sound-device", "enable", ID]), false, never), exit::NOT_ELEVATED);
    }

    #[test]
    fn elevated_runs_and_reports() {
        assert_eq!(decide(&args(&["sound-device", "disable", ID]), true, |_| true), exit::DONE);
        assert_eq!(decide(&args(&["sound-device", "disable", ID]), true, |_| false), exit::FAILED);
    }
}
