//! Runs only the app-host supervisor, for trying it without restarting the desktop:
//! `SHELLMAN_APP_HOST=on SHELLMAN_CONFIG_DIR=<temp> cargo run -p shellman-core --example app_host -- 120`
//! runs it for 120 s (default 60). Its log is `app-host.log` in that folder. Ending this process
//! leaves the host running, as the desktop's exit does.

fn main() {
    let secs = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(60);
    if !shellman_core::app_host::start(false) {
        eprintln!("not started (see {})", shellman_core::app_host::ENV);
        return;
    }
    std::thread::sleep(std::time::Duration::from_secs(secs));
}
