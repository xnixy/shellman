//! shellman-setup: install, update, inspect and remove shellman from the command line. A console
//! tool with no UI; it works from a plain folder of exes (a release zip) as well as from a
//! build's `target\release`. The work is the library's (`ops`); see `docs/PLAN.md`, "Install and
//! update". It never makes shellman the Windows shell without `--shell`, and never restarts
//! anything without `--restart`.

use shellman_setup::cli::{self, Cmd, Opts};
use shellman_setup::ops::{self, Env};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match cli::parse(&args) {
        Err(e) => {
            eprintln!("shellman-setup: {e}\n\n{}", cli::HELP);
            2
        }
        Ok((Cmd::Help, _)) => {
            println!("{}", cli::HELP);
            0
        }
        Ok((Cmd::Version, _)) => {
            println!("shellman-setup {}", shellman_buildinfo::STAMP);
            0
        }
        Ok((cmd, mut o)) => match Env::resolve(&o).and_then(|env| {
            if cmd == Cmd::Uninstall && o.revert && !o.dry_run && o.finish_command.is_none() && ops::plan_uninstall(&env).live_shell {
                o.finish_command = Some(finish_command(&env, &o)?);
            }
            ops::run(cmd, &o, &env)
        }) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("shellman-setup: {e}");
                1
            }
        },
    };
    std::process::exit(code);
}

/// For an uninstall while shellman is the running shell: a copy of this tool outside the install
/// folder, and the command that finishes the uninstall with it at the next sign-in.
fn finish_command(env: &Env, o: &Opts) -> Result<String, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = std::env::temp_dir().join(format!("shellman-uninstall-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let copy = dir.join("shellman-setup.exe");
    std::fs::copy(&me, &copy).map_err(|e| format!("cannot copy {} to {}: {e}", me.display(), copy.display()))?;
    Ok(format!(
        "\"{}\" uninstall --revert --prefix \"{}\" --start-menu \"{}\"{}",
        copy.display(),
        env.prefix.display(),
        env.start_menu.display(),
        if o.purge_config { " --purge-config" } else { "" }
    ))
}
