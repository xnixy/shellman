//! The work behind the window, on a thread of its own: unpack the payload and run setup's
//! `install` / `update`, or its `uninstall --revert`. Setup's report comes back line by line.

use crate::payload::{self, Payload};
use shellman_setup::cli::Opts;
use shellman_setup::ops::{self, Env};
use shellman_setup::{integrate, out};
use shellman_core::firefox::{self, actions, detect};
use shellman_core::registry::Real;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

pub enum Msg {
    Line(String),
    /// 0..1, or `None` while the length of what is left is unknown.
    Progress(Option<f32>),
    Done(Result<Outcome, String>),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Outcome {
    /// The shell changes at the next sign-in (made shellman's, or put back).
    pub sign_out: bool,
    /// shellman was the running shell: its files go at the next sign-in.
    pub deferred: bool,
}

/// What is there now, for the window's switches.
#[derive(Clone, Debug, Default)]
pub struct Current {
    pub installed: bool,
    pub stamp: Option<String>,
    pub shell: bool,
    pub terminal: bool,
    pub hook: bool,
    pub shortcuts: bool,
}

/// The install folder: where Apps & features says shellman is, else the default.
pub fn find_prefix(given: Option<PathBuf>) -> Option<PathBuf> {
    given.or_else(|| {
        let reg = shellman_setup::reg::open();
        integrate::app_registered(&*reg).map(PathBuf::from).filter(|p| p.is_dir())
    })
    .or_else(ops::default_prefix)
    .map(ops::trim_dir)
}

pub fn env_for(prefix: &Path) -> Result<Env, String> {
    Env::resolve(&Opts { prefix: Some(prefix.to_path_buf()), ..Default::default() })
}

pub fn current(prefix: &Path) -> Current {
    let Ok(env) = env_for(prefix) else { return Current::default() };
    let rec = ops::read_record(&env);
    Current {
        installed: rec.is_some(),
        stamp: rec.map(|r| r.stamp),
        shell: env.shell_is_shellman(),
        terminal: env.terminal_is_shellman(),
        hook: integrate::hook_registered(&*env.reg),
        shortcuts: shellman_setup::shortcuts::our_files().iter().any(|f| env.start_menu.join(f).is_file()),
    }
}

/// Whether Windows Terminal is there (shellman Terminal hands consoles over through it).
pub fn windows_terminal_installed() -> bool {
    std::env::var_os("LOCALAPPDATA").is_some_and(|l| Path::new(&l).join("Microsoft\\WindowsApps\\wt.exe").exists())
}

#[derive(Clone, Debug)]
pub struct Choice {
    pub prefix: PathBuf,
    pub shortcuts: bool,
    pub shell: bool,
    pub terminal: bool,
    pub hook: bool,
    /// Give Firefox's default profile the Islands look (CSS only; Settings adds the scripts).
    pub firefox: bool,
}

/// Whether Firefox is installed (the wizard offers to style it only then).
pub fn firefox_installed() -> bool {
    !detect::installs(&Real).is_empty()
}

/// The Firefox profiles that have the look (the uninstaller offers to take it out).
pub fn firefox_profiles_with_look() -> Vec<PathBuf> {
    if detect::installs(&Real).is_empty() {
        return Vec::new();
    }
    actions::profiles_with_look(&detect::Profiles::read())
}

/// Whether Firefox holds one of `profiles` (nothing can change in it then).
pub fn firefox_open(profiles: &[PathBuf]) -> bool {
    profiles.iter().any(|p| detect::in_use(p))
}

/// The look into the profile the first Firefox opens by default, from the install folder's copy.
/// Says what happened; never fails the install.
fn style_firefox(tx: &Sender<Msg>, prefix: &Path) {
    let installs = detect::installs(&Real);
    let profiles = detect::Profiles::read();
    let Some(profile) = installs.iter().find_map(|i| profiles.default_for(i)) else {
        say(tx, "Firefox: no default profile found; choose one in Settings \u{203a} Appearance \u{203a} Firefox.");
        return;
    };
    let Some(src) = firefox::Source::installed(prefix) else {
        say(tx, "Firefox: the look's files are missing from the install folder.");
        return;
    };
    match actions::apply(&profile, &src, None, &profiles, &installs, Some(prefix)) {
        Ok(msg) => say(tx, &format!("Firefox: {msg}")),
        Err(e) => say(tx, &format!("Firefox: {e} You can do it later in Settings \u{203a} Appearance \u{203a} Firefox.")),
    }
}

/// Take the look out of `profiles`, before shellman's files (with the administrator helper, for
/// the scripts' loader) go.
fn unstyle_firefox(say_line: &dyn Fn(&str), profiles: &[PathBuf], prefix: &Path) {
    let installs = detect::installs(&Real);
    let all = detect::Profiles::read();
    for p in profiles {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match actions::remove(p, &all, &installs, Some(prefix)) {
            Ok(_) => say_line(&format!("Firefox: the look is removed from {name}.")),
            Err(e) => say_line(&format!("Firefox ({name}): {e}")),
        }
    }
}

fn sink(tx: &Sender<Msg>, wake: impl Fn() + Send + Clone + 'static) {
    let tx = tx.clone();
    out::set_sink(move |l| {
        let _ = tx.send(Msg::Line(l.to_string()));
        wake();
    });
}

/// Unpack and install (or update, when shellman is installed there), then match the
/// integrations to `choice`: on what was asked for, off what was turned off.
pub fn install(payload: Payload, choice: Choice, was: Current, tx: Sender<Msg>, wake: impl Fn() + Send + Clone + 'static) {
    std::thread::spawn(move || {
        sink(&tx, wake.clone());
        let r = run_install(&payload, &choice, &was, &tx, &wake);
        let _ = tx.send(Msg::Done(r));
        wake();
    });
}

fn run_install(payload: &Payload, choice: &Choice, was: &Current, tx: &Sender<Msg>, wake: &(impl Fn() + Send)) -> Result<Outcome, String> {
    let staging = std::env::temp_dir().join(format!("shellman-install-{}", std::process::id()));
    say(tx, "Unpacking…");
    let files = payload::extract(payload, &staging, &mut |done, total| {
        let _ = tx.send(Msg::Progress(Some(0.5 * done as f32 / total.max(1) as f32)));
        wake();
    })?;
    say(tx, &format!("{} files unpacked.", files.len()));
    let _ = tx.send(Msg::Progress(None));
    let o = Opts {
        from: Some(staging.clone()),
        prefix: Some(choice.prefix.clone()),
        restart: true,
        no_shortcuts: !choice.shortcuts,
        shortcuts_on_update: choice.shortcuts,
        shell: choice.shell && !was.shell,
        default_terminal: choice.terminal && !was.terminal,
        picker_hook: choice.hook && !was.hook,
        ..Default::default()
    };
    let env = Env::resolve(&o)?;
    let r = if ops::read_record(&env).is_some() { ops::update(&o, &env) } else { ops::install(&o, &env) };
    let _ = std::fs::remove_dir_all(&staging);
    r?;
    let reg = &*env.reg;
    if was.hook && !choice.hook {
        integrate::unregister_hook(reg)?;
        say(tx, "Apps get Windows' own Open and Save dialogs again (from their next start).");
    }
    if was.terminal && !choice.terminal {
        integrate::revert_terminal(reg, &env.prefix)?;
        say(tx, "The default terminal that was there before is back.");
    }
    if was.shell && !choice.shell {
        integrate::revert_shell(reg, &env.prefix)?;
        say(tx, "Explorer is the shell again from the next sign-in.");
    }
    if choice.firefox {
        style_firefox(tx, &env.prefix);
    }
    let _ = tx.send(Msg::Progress(Some(1.0)));
    Ok(Outcome { sign_out: choice.shell != was.shell, deferred: false })
}

/// The command a RunOnce value runs at the next sign-in to finish an uninstall: this exe (the copy
/// in `%TEMP%` that runs the uninstall), quietly.
pub fn finish_command(prefix: &Path, purge: bool) -> Result<String, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(format!(
        "\"{}\" --uninstall --quiet --relaunched --prefix \"{}\"{}",
        me.display(),
        prefix.display(),
        if purge { " --purge-config" } else { "" }
    ))
}

/// `firefox`: the profiles to take the look out of first.
pub fn uninstall(prefix: PathBuf, purge: bool, firefox: Vec<PathBuf>, tx: Sender<Msg>, wake: impl Fn() + Send + Clone + 'static) {
    std::thread::spawn(move || {
        sink(&tx, wake.clone());
        let _ = tx.send(Msg::Progress(None));
        let t = tx.clone();
        unstyle_firefox(&move |l: &str| say(&t, l), &firefox, &prefix);
        let r = run_uninstall(&prefix, purge);
        let _ = tx.send(Msg::Done(r));
        wake();
    });
}

pub fn run_uninstall(prefix: &Path, purge: bool) -> Result<Outcome, String> {
    let o = Opts {
        prefix: Some(prefix.to_path_buf()),
        revert: true,
        purge_config: purge,
        finish_command: Some(finish_command(prefix, purge)?),
        ..Default::default()
    };
    let env = Env::resolve(&o)?;
    let plan = ops::plan_uninstall(&env);
    ops::uninstall(&o, &env)?;
    Ok(Outcome { sign_out: plan.shell || plan.live_shell, deferred: plan.live_shell })
}

fn say(tx: &Sender<Msg>, text: &str) {
    let _ = tx.send(Msg::Line(text.to_string()));
}
