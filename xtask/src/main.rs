//! Maintainer commands, run as `cargo xtask <command>` (the alias is in `.cargo/config.toml`).
//!
//! - `release --version X.Y.Z [--publish]`: build, pack, sign, write release notes, and (only with
//!   `--publish`) create the GitHub Release. Without `--publish` it is a dry run that stops before
//!   anything leaves this PC. The signing key comes from the environment
//!   (`SHELLMAN_RELEASE_SIGNING_KEY`), put there by 1Password: see `release.rs`.
//! - `keygen`: make the release signing key pair and put the private half straight into a new
//!   1Password Environment; prints only the public half. Run once (or to replace the key).
//!
//! A Rust program rather than a script: the signing and the check before upload use the very code
//! the updater verifies with (`shellman-update-core`), and it runs the same on any shell.
//! `xtask.exe` is not a `shellman-*` file, so a pack never includes it.

mod keygen;
mod mcp;
mod release;

const HELP: &str = "cargo xtask <command>
  release --version X.Y.Z [--publish] [--test-key] [--allow-dirty] [--skip-build]
      build, pack and sign a release; with --publish also create the GitHub Release.
      Needs SHELLMAN_RELEASE_SIGNING_KEY: run it under
        op run --environment ojhbajp6vr6dp33bdkrjjgldci -- cargo xtask release ...
      --test-key   sign with a throwaway key (dry runs only; nothing can be published)
  keygen
      make the signing key pair; the private key goes only into 1Password";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("release") => release::run(&args[1..]),
        Some("keygen") => keygen::run(&args[1..]),
        _ => {
            eprintln!("{HELP}");
            2
        }
    };
    std::process::exit(code);
}
