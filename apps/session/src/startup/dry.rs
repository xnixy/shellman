//! `shellman-session --dry-run`: what the startup items would do, started or not. Reads the
//! registry and the Startup folders, writes nothing and starts nothing.

use crate::apps;
use crate::registry::Real;

pub fn dry_run() {
    let (list, test) = apps::load();
    println!("apps, in start order{}:", if test { " (test override)" } else { "" });
    for a in &list {
        println!("  {:<10} {} {}", a.name, a.exe.display(), a.args.join(" "));
    }
    super::win::init_com();
    let items = super::collect(&Real);
    println!("\nstartup items, in the order they would start ({}):", items.len());
    for i in &items {
        let state = if i.enabled { "ENABLED " } else { "disabled" };
        let once = if i.once { " [RunOnce: removed when started]" } else { "" };
        println!("  {state}  {:<22} {}{once}\n            why: {}", i.place, i.name, i.why);
        println!("            command: {}", i.command);
    }
    let on = items.iter().filter(|i| i.enabled).count();
    println!("\n{on} would start, {} would not. Nothing was started or changed.", items.len() - on);
    let session = super::win::session_id();
    let marker = if super::guard::windows_marker(&Real, session) {
        "present: Explorer or the modern-app host would not run these again"
    } else {
        "absent: a modern-app host started now would run them again (the session sets it when it runs them)"
    };
    println!("Windows' marker SessionInfo\\{session}\\RunStuffHasBeenRun: {marker}.");
}
