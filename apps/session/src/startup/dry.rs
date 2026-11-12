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
    println!("Packaged (Store) apps' startup tasks are not handled here.");
}
