//! Blurman, from the Microsoft Store: the wizard's "Also install Blurman" (on by default, shown
//! when it is not installed yet) and `--blurman` with `--quiet`; the uninstall window can remove
//! it (off by default). It is an app of its own: removing shellman leaves it unless asked.
//!
//! Installed through Windows' Store install service (`AppInstallManager`,
//! `StartProductInstallAsync`), which needs no Store window: the Store's own UI cannot open under
//! shellman without the modern-app host. When that service fails, `winget install <id> --source
//! msstore` does the same through the command line (no window). Neither ever fails the shellman
//! install: what happened is a line in the log.
//!
//! Whether it is there: `PackageManager.FindPackagesByUserSecurityIdPackageFamilyName` for the
//! current user.

mod store_api;

use std::time::{Duration, Instant};
use store_api::{AppInstallManager, AppInstallState};
use windows::Management::Deployment::{PackageManager, PackageVolume};
use windows_core::HSTRING;

pub const PRODUCT_ID: &str = "9NQ3GQNBM4LK";
pub const FAMILY: &str = "xnixy.Blurman_nz2sgw67tcfww";
/// The longest the wizard waits for a Store install it started (the Store carries on after).
const LIMIT: Duration = Duration::from_secs(5 * 60);

/// The full names of the Blurman packages installed for this user.
fn packages() -> windows_core::Result<Vec<String>> {
    let pm = PackageManager::new()?;
    let found = pm.FindPackagesByUserSecurityIdPackageFamilyName(&HSTRING::new(), &HSTRING::from(FAMILY))?;
    let mut out = Vec::new();
    for p in found {
        out.push(p.Id()?.FullName()?.to_string_lossy());
    }
    Ok(out)
}

/// Whether Blurman is installed for this user (false when that cannot be told).
pub fn installed() -> bool {
    packages().is_ok_and(|p| !p.is_empty())
}

/// What one look at an install item says, as a line and whether it is over.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    Working(Option<String>),
    Done,
    Failed(String),
}

/// From an item's state, percent and error code. Pure.
fn step(state: AppInstallState, percent: f64, error: windows_core::HRESULT) -> Step {
    match state {
        AppInstallState::Completed => Step::Done,
        AppInstallState::Error => Step::Failed(format!("the Store install failed ({})", error.message().trim())),
        AppInstallState::Canceled => Step::Failed("the Store install was cancelled".into()),
        AppInstallState::PausedLowBattery | AppInstallState::PausedWiFiRecommended | AppInstallState::PausedWiFiRequired => {
            Step::Failed("the Store paused the install (battery or Wi-Fi); it carries on by itself".into())
        }
        AppInstallState::Downloading | AppInstallState::Installing if percent > 0.0 => Step::Working(Some(format!("{percent:.0}%"))),
        _ => Step::Working(None),
    }
}

/// How the Store install service ended.
enum Store {
    Done,
    /// It never took the job: winget may.
    NotStarted(String),
    /// It took the job and it did not finish here (failed, paused, or still going): reported,
    /// not tried again, since a second install would race the Store's own.
    Started(String),
}

/// The Store install service, to the end or to the wizard's limit.
fn via_store(say: &dyn Fn(&str)) -> Store {
    let e = |e: windows_core::Error| format!("the Store install service: {}", e.message().trim());
    let items = (|| -> Result<Vec<store_api::AppInstallItem>, String> {
        let mgr = AppInstallManager::new().map_err(e)?;
        let id = HSTRING::from(PRODUCT_ID);
        if !mgr.GetIsAppAllowedToInstallAsync(&id).and_then(|op| op.get()).map_err(e)? {
            return Err("the Store does not allow installing it here".into());
        }
        let none = HSTRING::new();
        let items = mgr
            .StartProductInstallAsync(&id, &none, &none, &HSTRING::from("shellman"), false, false, &none, None::<&PackageVolume>)
            .and_then(|op| op.get())
            .map_err(e)?;
        let items: Vec<_> = items.into_iter().collect();
        if items.is_empty() {
            return Err("the Store had nothing to install".into());
        }
        Ok(items)
    })();
    let items = match items {
        Ok(i) => i,
        Err(why) => return Store::NotStarted(why),
    };
    match watch(&items, say) {
        Ok(()) => Store::Done,
        Err(why) => Store::Started(why),
    }
}

/// Follow the Store's items until they are done, one fails, or the limit.
fn watch(items: &[store_api::AppInstallItem], say: &dyn Fn(&str)) -> Result<(), String> {
    let e = |e: windows_core::Error| format!("the Store install service: {}", e.message().trim());
    let start = Instant::now();
    let mut last = String::new();
    loop {
        let mut all_done = true;
        for item in items {
            let s = item.GetCurrentStatus().map_err(e)?;
            match step(s.InstallState().map_err(e)?, s.PercentComplete().map_err(e)?, s.ErrorCode().map_err(e)?) {
                Step::Done => {}
                Step::Failed(why) => return Err(why),
                Step::Working(line) => {
                    all_done = false;
                    if let Some(l) = line.filter(|l| *l != last) {
                        say(&format!("Blurman: {l}"));
                        last = l;
                    }
                }
            }
        }
        if all_done {
            return Ok(());
        }
        if start.elapsed() > LIMIT {
            return Err("the Store is still installing it and finishes by itself".into());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// The winget command line for the fallback.
pub fn winget_args() -> [&'static str; 7] {
    ["install", PRODUCT_ID, "--source", "msstore", "--accept-package-agreements", "--accept-source-agreements", "--disable-interactivity"]
}

fn via_winget() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("winget")
        .args(winget_args())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("winget could not start ({e})"))?;
    if out.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let last = text.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("").to_string();
    Err(format!("winget ended with {} {last}", out.status.code().unwrap_or(-1)))
}

/// Install Blurman, saying how it goes. Returns the line to end with; never an error, since a
/// failure here never fails shellman's install.
pub fn install(say: &dyn Fn(&str)) -> String {
    if installed() {
        return "Blurman is already installed.".into();
    }
    say("Blurman: installing from the Microsoft Store…");
    let _com = Com::init();
    let r = match via_store(say) {
        Store::Done => Ok(()),
        Store::Started(why) => return format!("Blurman: {why}. If it does not appear, get it from the Microsoft Store."),
        Store::NotStarted(why) => {
            say(&format!("Blurman: {why}; trying winget."));
            via_winget()
        }
    };
    match r {
        Ok(()) if installed() => "Blurman is installed.".into(),
        Ok(()) => "Blurman: the install reported success, but the app is not there yet; it may still be finishing.".into(),
        Err(why) => format!("Blurman was not installed: {why}. You can get it from the Microsoft Store later."),
    }
}

/// Remove Blurman for this user. Returns what happened.
pub fn remove() -> String {
    let _com = Com::init();
    let r = (|| -> windows_core::Result<usize> {
        let pm = PackageManager::new()?;
        let names = packages()?;
        for n in &names {
            pm.RemovePackageAsync(&HSTRING::from(n.as_str()))?.get()?;
        }
        Ok(names.len())
    })();
    match r {
        Ok(0) => "Blurman was not installed.".into(),
        Ok(_) => "Blurman is removed.".into(),
        Err(e) => format!("Blurman was not removed: {}", e.message().trim()),
    }
}

/// COM for this thread (the worker threads have none), undone on drop.
struct Com(bool);

impl Com {
    fn init() -> Com {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        Com(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_core::HRESULT;

    #[test]
    fn states_become_lines_and_ends() {
        assert_eq!(step(AppInstallState::Completed, 100.0, HRESULT(0)), Step::Done);
        assert_eq!(step(AppInstallState::Downloading, 42.4, HRESULT(0)), Step::Working(Some("42%".into())));
        assert_eq!(step(AppInstallState::Pending, 0.0, HRESULT(0)), Step::Working(None));
        assert!(matches!(step(AppInstallState::Error, 0.0, HRESULT(0x80070005u32 as i32)), Step::Failed(_)));
        assert!(matches!(step(AppInstallState::Canceled, 0.0, HRESULT(0)), Step::Failed(_)));
    }

    #[test]
    fn winget_runs_without_questions() {
        let a = winget_args();
        assert_eq!(a[..4], ["install", PRODUCT_ID, "--source", "msstore"]);
        assert!(a.contains(&"--disable-interactivity") && a.contains(&"--accept-package-agreements"));
    }

    /// On a PC with Blurman installed: it is found, and the Store service answers whether it may
    /// be installed, without installing anything. `cargo test -p shellman-installer -- --ignored`.
    #[test]
    #[ignore]
    fn the_store_service_answers_without_installing() {
        let _com = Com::init();
        assert!(installed(), "Blurman is not installed here");
        assert_eq!(install(&|_| panic!("must not try to install")), "Blurman is already installed.");
        let mgr = AppInstallManager::new().unwrap();
        let allowed = mgr.GetIsAppAllowedToInstallAsync(&HSTRING::from(PRODUCT_ID)).unwrap().get().unwrap();
        println!("allowed to install: {allowed}");
    }
}
