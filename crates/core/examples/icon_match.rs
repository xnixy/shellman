//! How well an icon pack covers the apps installed here, and how long reading it takes:
//! `cargo run --release -p shellman-core --example icon_match -- <pack folder> [siblings folder]`
//! Reads the pack and the Apps folder; writes nothing.

use shellman_core::icon_pack::names::{candidates, AppRef};
use shellman_core::icon_pack::PackIndex;
use std::path::Path;
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(pack) = args.next() else {
        eprintln!("usage: icon_match <pack folder> [folder holding the other packs]");
        return;
    };
    let siblings = args.next();
    let t = Instant::now();
    let index = PackIndex::build(Path::new(&pack), siblings.as_deref().map(Path::new));
    println!("indexed {} names in {:?}", index.len(), t.elapsed());

    let apps = shellman_core::apps_index::enumerate();
    let (mut hit, mut miss) = (Vec::new(), Vec::new());
    let t = Instant::now();
    for app in &apps {
        // The launcher knows an app by its name and its Apps-folder id.
        let c = candidates(&AppRef { id: &app.id, exe: None, name: Some(&app.name) });
        match index.lookup(&c) {
            Some(p) => hit.push((app.name.clone(), format!("{}   <- {c:?} [{}]", p.file_name().unwrap().to_string_lossy(), app.id))),
            None => miss.push((app.name.clone(), app.id.clone())),
        }
    }
    println!("looked up {} apps in {:?}", apps.len(), t.elapsed());
    println!("{} of {} apps matched", hit.len(), apps.len());
    println!("-- matched");
    for (name, file) in &hit {
        println!("{name}  =>  {file}");
    }
    println!("-- missed");
    for (name, id) in &miss {
        println!("{name}  [{id}]");
    }
}
