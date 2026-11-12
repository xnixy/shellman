//! Read-only look at the notification store, as the bar sees it.
//!
//! `cargo run -p shellman-core --example notify` lists every toast with its app's activator;
//! `-- watch 20` also prints each re-read for 20 seconds, with how long after the change it came.

use shellman_core::notify::{activate, platform::Platform, watch::Watch, Source};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
    let source = Platform::new();
    let started = Instant::now();
    match source.read() {
        Err(e) => println!("read failed: {e}"),
        Ok(toasts) => {
            println!("{} toasts, read in {:?}", toasts.len(), started.elapsed());
            for t in &toasts {
                println!(
                    "#{} {} ({}) title={:?} body={:?} launch={:?} actions={} reply={} icon={:?} activator={:?}",
                    t.id,
                    t.app_name,
                    t.app_id,
                    t.title,
                    t.body,
                    t.launch,
                    t.actions.len(),
                    t.has_reply(),
                    t.app_icon,
                    activate::activator(&t.app_id),
                );
            }
        }
    }
    if args.get(1).map(String::as_str) == Some("watch") {
        let secs = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20);
        let t0 = Instant::now();
        let _watch = Watch::start(
            Arc::new(Platform::new()),
            Box::new(move |r| match r {
                Ok(list) => println!(
                    "[{:?}, clock {}] read: {} toasts, newest {:?}",
                    t0.elapsed(),
                    clock_ms(),
                    list.len(),
                    list.last().map(|t| &t.title)
                ),
                Err(e) => println!("[{:?}] read failed: {e}", t0.elapsed()),
            }),
        );
        std::thread::sleep(Duration::from_secs(secs));
    }
}

/// Milliseconds past the second, to line a read up with when a toast was sent.
fn clock_ms() -> String {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{}.{:03}", d.as_secs() % 60, d.subsec_millis())
}
