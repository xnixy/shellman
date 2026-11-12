//! Adds an icon pack from a folder, as Settings' "Add pack from folder..." does, and lists the
//! packs: `SHELLMAN_CONFIG_DIR=<temp> cargo run --release -p shellman-core --example icon_import -- <folder>`

use std::sync::atomic::AtomicUsize;
use std::time::Instant;

fn main() {
    let Some(folder) = std::env::args().nth(1) else {
        eprintln!("usage: icon_import <folder>");
        return;
    };
    let count = AtomicUsize::new(0);
    let t = Instant::now();
    match shellman_core::icon_pack::import::add_from_folder(std::path::Path::new(&folder), &count) {
        Ok(names) => println!("added {names:?}, {} files, {:?}", count.load(std::sync::atomic::Ordering::Relaxed), t.elapsed()),
        Err(e) => println!("failed: {e}"),
    }
    println!("packs: {:?}", shellman_core::icon_pack::import::list());
}
