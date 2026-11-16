#![windows_subsystem = "windows"]
//! shellman-update: look for a newer shellman release and install it.
//!
//! - `--auto`: what the session starts, on its schedule (`shellman_update_core::schedule`). Does
//!   nothing when Settings > About > "Update automatically" is off.
//! - `--now`: Settings' "Check now": the same, whatever that switch says.
//! - `--check`: only look, install nothing.
//! - `--prefix <dir>`: the install folder (default: this exe's own). Nothing happens unless setup's
//!   record (`shellman-install.txt`) is there, so a build folder or a test copy never updates.
//!
//! In order: the feed (GitHub's latest release; `SHELLMAN_UPDATE_FEED` points elsewhere for tests)
//! names a version; only a greater one goes on. Its `.sig` is fetched and checked against the
//! public key compiled in, and its signed version must be that greater one. The setup exe is
//! downloaded to `%TEMP%`, at most the signed size, and must match the signed name, size and
//! SHA-256, and its payload its own hash. Then, while no app is fullscreen and no files are being
//! copied, `<setup> --quiet --prefix <dir>`: setup replaces the files and asks the session to
//! restart the apps it supervises; the session itself switches at the next sign-in. Nothing signs
//! the user out. Every step goes to `session.log`; the result to `update.json` (`state`).

mod busy;
mod log;

use shellman_update_core::state::{Outcome, State};
use shellman_update_core::version::Version;
use shellman_update_core::{feed, payload, schedule, sig};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Auto,
    Now,
    Check,
}

struct Args {
    mode: Mode,
    prefix: Option<PathBuf>,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args { mode: Mode::Now, prefix: None };
    let mut it = args.iter();
    while let Some(x) = it.next() {
        match x.as_str() {
            "--auto" => a.mode = Mode::Auto,
            "--now" => a.mode = Mode::Now,
            "--check" => a.mode = Mode::Check,
            "--prefix" => a.prefix = Some(PathBuf::from(it.next().ok_or("--prefix needs a folder")?)),
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    Ok(a)
}

/// The record setup leaves in an install folder (`apps/setup/src/state.rs`).
const RECORD: &str = "shellman-install.txt";

fn main() {
    log::attach_console();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a = match parse(&args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("shellman-update: {e}\n  --auto | --now | --check   [--prefix <install folder>]");
            std::process::exit(2);
        }
    };
    let Some(_one) = log::claim() else {
        log::line("another update check is under way; this one stops");
        std::process::exit(0);
    };
    std::process::exit(run(&a));
}

fn run(a: &Args) -> i32 {
    let prefix = match a.prefix.clone().or_else(|| std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf))) {
        Some(p) => p,
        None => return 1,
    };
    if a.mode == Mode::Auto && !shellman_core::settings::shell::ShellSettings::load().auto_update {
        log::line("updating automatically is off; nothing checked");
        return 0;
    }
    let installed = Version::from_stamp(shellman_buildinfo::STAMP).unwrap_or(Version::new(0, 0, 0));
    let mut st = State::load();
    if !prefix.join(RECORD).is_file() {
        log::line(&format!("{} was not installed by setup (no {RECORD}); not updating", prefix.display()));
        finish(&mut st, Outcome::NotInstalled, "");
        return 0;
    }
    log::line(&format!("checking for a release newer than {installed} ({:?}, {})", a.mode, prefix.display()));
    working(&mut st, "checking");
    match update(a.mode, &prefix, installed, &mut st) {
        Ok((outcome, note)) => {
            log::line(&note);
            finish(&mut st, outcome, "");
            0
        }
        Err((outcome, why)) => {
            log::line(&format!("{}: {why}", if outcome == Outcome::Held { "held off" } else { "ERROR update" }));
            finish(&mut st, outcome, &why);
            1
        }
    }
}

fn working(st: &mut State, phase: &str) {
    st.outcome = Outcome::Working;
    st.phase = phase.to_string();
    st.save();
}

fn finish(st: &mut State, outcome: Outcome, detail: &str) {
    st.outcome = outcome;
    st.phase.clear();
    st.detail = detail.to_string();
    st.last_check = shellman_update_core::state::now();
    st.save();
}

type Step<T> = Result<T, (Outcome, String)>;

fn failed<E: std::fmt::Display>(e: E) -> (Outcome, String) {
    (Outcome::Failed, e.to_string())
}

/// The feed's address.
fn feed_url() -> String {
    std::env::var("SHELLMAN_UPDATE_FEED").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| shellman_update_core::FEED.to_string())
}

/// Where downloads may come from: HTTPS. A debug build also takes plain HTTP from this PC (a
/// local fake release for tests); a release build never does.
fn allowed(u: &shellman_core::net::Url) -> bool {
    u.https || (cfg!(debug_assertions) && matches!(u.host.as_str(), "127.0.0.1" | "localhost" | "[::1]"))
}

fn update(mode: Mode, prefix: &Path, installed: Version, st: &mut State) -> Step<(Outcome, String)> {
    use shellman_core::net;
    let url = feed_url();
    let u = net::parse_url(&url).ok_or_else(|| failed("the feed address is not valid"))?;
    if !allowed(&u) {
        return Err(failed("the feed must be HTTPS"));
    }
    let r = net::send("GET", &url, &[("Accept", "application/vnd.github+json")], None).map_err(failed)?;
    if r.status == 404 {
        st.version.clear();
        st.page = shellman_update_core::RELEASES_PAGE.to_string();
        return Ok((Outcome::UpToDate, "no release is published yet".into()));
    }
    if !r.ok() {
        return Err(failed(format!("the release feed answered {}", r.status)));
    }
    let rel = feed::parse(&r.text()).map_err(failed)?;
    st.version = rel.version.to_string();
    st.page = rel.page.clone();
    if !rel.version.replaces(&installed) {
        return Ok((Outcome::UpToDate, format!("up to date: the newest release is {}, this is {installed}", rel.version)));
    }
    if mode == Mode::Check {
        return Ok((Outcome::Available, format!("{} is available (not installed: --check)", rel.version)));
    }

    // The signature first: nothing big is downloaded for a release that is not signed.
    if !net::parse_url(&rel.sig_url).is_some_and(|u| allowed(&u)) {
        return Err(failed("the signature's address must be HTTPS"));
    }
    let s = net::send("GET", &rel.sig_url, &[("Accept", "application/octet-stream")], None).map_err(failed)?;
    let s = follow(s, &rel.sig_url)?;
    let statement = sig::verify(&s, &sig::trusted_keys()).map_err(failed)?;
    if Version::parse(&statement.version) != Some(rel.version) {
        return Err(failed(format!("the signature is for version {}, the release says {}", statement.version, rel.version)));
    }
    let key = if sig::verify(&s, &[sig::PUBLIC_KEY]).is_ok() { format!("release key {}", sig::fingerprint(&sig::PUBLIC_KEY)) } else { "the test key".to_string() };
    log::line(&format!("{}: signature valid ({key}), {} bytes, sha256 {}", rel.version, statement.size, &statement.sha256[..16]));

    working(st, "downloading");
    let dir = std::env::temp_dir().join(format!("shellman-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(failed)?;
    let result = download_and_install(&rel, &statement, &dir, prefix, st);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// A small download (the signature): follow GitHub's redirect to its storage host.
fn follow(mut r: shellman_core::net::Response, url: &str) -> Step<String> {
    use shellman_core::net;
    let mut here = url.to_string();
    for _ in 0..5 {
        match (&r.location, r.status) {
            (Some(loc), 300..=399) => {
                let next = if loc.starts_with('/') {
                    let u = net::parse_url(&here).ok_or_else(|| failed("bad redirect"))?;
                    format!("{}://{}:{}{loc}", if u.https { "https" } else { "http" }, u.host, u.port)
                } else {
                    loc.clone()
                };
                let nu = net::parse_url(&next).ok_or_else(|| failed("bad redirect"))?;
                if !allowed(&nu) {
                    return Err(failed(format!("refused a redirect to {}", nu.host)));
                }
                r = net::send("GET", &next, &[("Accept", "application/octet-stream")], None).map_err(failed)?;
                here = next;
            }
            (_, 200..=299) => {
                if r.body.len() > 4096 {
                    return Err(failed("the signature file is too large"));
                }
                return Ok(r.text());
            }
            (_, status) => return Err(failed(format!("the signature download answered {status}"))),
        }
    }
    Err(failed("too many redirects"))
}

fn download_and_install(rel: &feed::Release, statement: &sig::Statement, dir: &Path, prefix: &Path, st: &mut State) -> Step<(Outcome, String)> {
    let exe = dir.join(&rel.exe_name);
    let t = Instant::now();
    shellman_core::net::download(&rel.exe_url, &exe, statement.size, &allowed, &mut |_| {}).map_err(failed)?;
    let size = std::fs::metadata(&exe).map_err(failed)?.len();
    let sha = shellman_update_core::sha256_file(&exe).map_err(failed)?;
    statement.matches(&rel.exe_name, size, &sha).map_err(failed)?;
    payload::check(&exe).map_err(failed)?;
    log::line(&format!("{}: downloaded in {:.1} s; matches its signature and its payload hash", rel.exe_name, t.elapsed().as_secs_f32()));

    // Never in the middle of a game, a video or a file copy.
    let mut waited = Duration::ZERO;
    let mut said = false;
    loop {
        let why = schedule::hold(busy::now());
        let Some(why) = why else { break };
        if !said {
            log::line(&format!("{} is ready; waiting: {why}", rel.version));
            working(st, "waiting");
            said = true;
        }
        if !schedule::keep_waiting(waited) {
            return Err((Outcome::Held, why.to_string()));
        }
        std::thread::sleep(schedule::HOLD_POLL);
        waited += schedule::HOLD_POLL;
    }

    working(st, "installing");
    log::line(&format!("installing {} into {}", rel.version, prefix.display()));
    let out = run_quiet(&exe, prefix).map_err(failed)?;
    if !out.status.success() {
        let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).rev().take(3).collect();
        return Err(failed(format!("setup failed ({}): {}", out.status, tail.into_iter().rev().collect::<Vec<_>>().join(" / "))));
    }
    // Setup's own summary (its last lines name what changed and what restarts).
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    for l in text.lines().map(str::trim).filter(|l| l.contains("changed,") || l.contains("restart") || l.starts_with("replaced")) {
        log::line(&format!("setup: {l}"));
    }
    Ok((Outcome::Installed, format!("installed {}; the apps restart, the session switches at the next sign-in", rel.version)))
}

/// `<setup> --quiet --prefix <dir>`, with no window, waiting for it.
fn run_quiet(exe: &Path, prefix: &Path) -> std::io::Result<std::process::Output> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // A trailing `\` would end the quoted argument early (see setup's RunOnce notes).
    let p = prefix.display().to_string();
    let p = p.trim_end_matches('\\');
    std::process::Command::new(exe).arg("--quiet").arg("--prefix").arg(p).creation_flags(CREATE_NO_WINDOW).output()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn modes() {
        assert_eq!(parse(&a(&[])).unwrap().mode, Mode::Now);
        assert_eq!(parse(&a(&["--auto"])).unwrap().mode, Mode::Auto);
        let p = parse(&a(&["--check", "--prefix", "C:\\x"])).unwrap();
        assert_eq!((p.mode, p.prefix.unwrap()), (Mode::Check, PathBuf::from("C:\\x")));
        assert!(parse(&a(&["--prefix"])).is_err());
        assert!(parse(&a(&["--bogus"])).is_err());
    }

    #[test]
    fn downloads_are_https() {
        use shellman_core::net::parse_url;
        assert!(allowed(&parse_url("https://objects.githubusercontent.com/x").unwrap()));
        assert!(!allowed(&parse_url("http://example.com/x").unwrap()));
        // Plain HTTP from this PC only in a debug build (tests run as one).
        assert_eq!(allowed(&parse_url("http://127.0.0.1:8000/x").unwrap()), cfg!(debug_assertions));
    }
}
