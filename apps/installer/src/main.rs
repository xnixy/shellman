#![windows_subsystem = "windows"]
//! shellman-installer: one exe that installs shellman, and (from Apps & features) removes it.
//!
//! - A packed installer (the stub with every file appended, see `payload`) opens the install
//!   wizard: folder, shortcuts, and the opt-in integrations; it installs, or updates what is
//!   there.
//! - `--uninstall` opens the uninstall window (`--quiet`: none). From the install folder it first
//!   runs itself again from a copy in `%TEMP%`, so it can remove its own exe.
//! - `--pack <folder> [--out <file>]` writes a packed installer from a build (`target\release`).
//! - `--quiet` installs with the defaults and no window; `--shell`, `--default-terminal`,
//!   `--picker-hook`, `--blurman` (Blurman from the Microsoft Store, see `blurman`) and
//!   `--prefix <dir>` work with it.
//!
//! The work itself is `shellman-setup`'s (its library), so the window and the command line do
//! exactly the same thing. See `docs/PLAN.md`, "Install and update".

mod app;
mod blurman;
mod payload;
mod winsys;
mod work;

use std::path::PathBuf;

struct Args {
    raw: Vec<String>,
    uninstall: bool,
    quiet: bool,
    relaunched: bool,
    purge: bool,
    prefix: Option<PathBuf>,
    pack: Option<PathBuf>,
    out: Option<PathBuf>,
    shell: bool,
    terminal: bool,
    hook: bool,
    blurman: bool,
}

fn parse() -> Result<Args, String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut a = Args { raw: raw.clone(), uninstall: false, quiet: false, relaunched: false, purge: false, prefix: None, pack: None, out: None, shell: false, terminal: false, hook: false, blurman: false };
    let mut it = raw.into_iter();
    while let Some(x) = it.next() {
        let mut value = |f: &str| it.next().map(PathBuf::from).ok_or(format!("{f} needs a value"));
        match x.as_str() {
            "--uninstall" => a.uninstall = true,
            "--quiet" => a.quiet = true,
            "--relaunched" => a.relaunched = true,
            "--purge-config" => a.purge = true,
            "--shell" => a.shell = true,
            "--default-terminal" => a.terminal = true,
            "--picker-hook" => a.hook = true,
            "--blurman" => a.blurman = true,
            "--prefix" => a.prefix = Some(value("--prefix")?),
            "--pack" => a.pack = Some(value("--pack")?),
            "--out" => a.out = Some(value("--out")?),
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    Ok(a)
}

const HELP: &str = "shellman-installer: install or remove shellman.
  (no arguments)        the install wizard (a packed installer), else the uninstall window
  --uninstall [--quiet] [--purge-config] [--prefix <dir>]
  --quiet [--prefix <dir>] [--shell] [--default-terminal] [--picker-hook] [--blurman]
                        install with no window (--blurman: also Blurman from the Store)
  --pack <folder> [--out <file>]
                        write a packed installer from a build folder (target\\release)";

fn main() {
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            winsys::attach_console();
            eprintln!("shellman-installer: {e}\n\n{HELP}");
            std::process::exit(2);
        }
    };
    std::process::exit(run(args));
}

fn run(a: Args) -> i32 {
    if let Some(from) = &a.pack {
        winsys::attach_console();
        return pack(from, a.out.clone());
    }
    let me = std::env::current_exe().unwrap_or_default();
    let packed = payload::find(&me);
    let Some(prefix) = work::find_prefix(a.prefix.clone()) else {
        winsys::attach_console();
        eprintln!("shellman-installer: cannot find %LOCALAPPDATA%; give --prefix");
        return 1;
    };
    let uninstall = a.uninstall || packed.is_none();
    if uninstall && a.relaunched {
        winsys::wait_for_stub(&prefix);
    }
    if uninstall && !a.relaunched {
        match winsys::relaunch_outside(&prefix, &a.raw) {
            Ok(true) => return 0,
            Ok(false) => {}
            Err(e) => {
                winsys::attach_console();
                eprintln!("shellman-installer: {e}");
                return 1;
            }
        }
    }
    if a.quiet {
        winsys::attach_console();
        return if uninstall { quiet_uninstall(&a, &prefix) } else { quiet_install(&a, packed.expect("packed"), &prefix) };
    }
    let mode = match packed {
        Some(p) if !a.uninstall => app::Mode::Install(p),
        _ => app::Mode::Uninstall,
    };
    let r = app::run(mode, prefix);
    if a.relaunched && !r.deferred {
        winsys::remove_self_later();
    }
    if r.ok { 0 } else { 1 }
}

fn pack(from: &std::path::Path, out: Option<PathBuf>) -> i32 {
    let me = std::env::current_exe().unwrap_or_default();
    let version = shellman_setup::integrate::display_version(shellman_buildinfo::STAMP).replace(['+', ' '], "-");
    let out = out.unwrap_or_else(|| from.parent().unwrap_or(from).join("dist").join(format!("shellman-{version}-setup.exe")));
    println!("packing {} into {}", from.display(), out.display());
    match payload::pack(&me, from, &out) {
        Ok((n, bytes)) => {
            println!("{n} files, {:.1} MB packed: {}", bytes as f64 / 1_048_576.0, out.display());
            0
        }
        Err(e) => {
            eprintln!("shellman-installer: {e}");
            1
        }
    }
}

fn quiet_install(a: &Args, p: payload::Payload, prefix: &std::path::Path) -> i32 {
    let was = work::current(prefix);
    let choice = work::Choice { prefix: prefix.to_path_buf(), shortcuts: true, shell: a.shell || was.shell, terminal: a.terminal || was.terminal, hook: a.hook || was.hook, firefox: false, blurman: a.blurman };
    let (tx, rx) = std::sync::mpsc::channel();
    work::install(p, choice, was, tx, || {});
    for m in rx {
        match m {
            work::Msg::Line(l) => println!("{l}"),
            work::Msg::Progress(_) => {}
            work::Msg::Done(Ok(_)) => return 0,
            work::Msg::Done(Err(e)) => {
                eprintln!("shellman-installer: {e}");
                return 1;
            }
        }
    }
    1
}

fn quiet_uninstall(a: &Args, prefix: &std::path::Path) -> i32 {
    match work::run_uninstall(prefix, a.purge) {
        Ok(r) => {
            if a.relaunched && !r.deferred {
                winsys::remove_self_later();
            }
            0
        }
        Err(e) => {
            eprintln!("shellman-installer: {e}");
            1
        }
    }
}
