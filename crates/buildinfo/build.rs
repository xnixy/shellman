//! The build stamp: `<crate version>+<git describe> <commit date>`, or just the crate version
//! when git (or a repository) is not there. `SHELLMAN_BUILD_STAMP` overrides it (release zips).
//! Kept cheap: two `git` calls, rerun only when HEAD moves.
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !s.is_empty()).then_some(s)
}

fn main() {
    println!("cargo:rerun-if-env-changed=SHELLMAN_BUILD_STAMP");
    println!("cargo:rerun-if-changed=build.rs");
    // HEAD of the checkout (a worktree's `.git` is a file, so ask git where it is).
    if let Some(head) = git(&["rev-parse", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={head}");
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let stamp = std::env::var("SHELLMAN_BUILD_STAMP").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| {
        match (git(&["describe", "--tags", "--always"]), git(&["log", "-1", "--format=%cs"])) {
            (Some(d), Some(date)) => format!("{version}+{d} {date}"),
            (Some(d), None) => format!("{version}+{d}"),
            _ => version.clone(),
        }
    });
    let dir = std::env::var("OUT_DIR").unwrap();
    std::fs::write(std::path::Path::new(&dir).join("stamp.txt"), stamp).unwrap();
}
