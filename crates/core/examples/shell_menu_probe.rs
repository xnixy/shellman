//! Prints the shell menu Files would read for some paths, with timings: what Windows and the
//! installed apps put there, and what Files keeps. Nothing is shown or run.
//!
//! `cargo run -p shellman-core --example shell_menu_probe -- [--bg] [--shift] [--twice] PATH...`
//! Paths are items in one folder; with `--bg`, the one path is a folder whose background menu is
//! read.

use shellman_core::shell_menu::{probe, Item, Target};
use std::path::PathBuf;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut flag = |name: &str| {
        let had = args.iter().any(|a| a == name);
        args.retain(|a| a != name);
        had
    };
    let (bg, shift, twice) = (flag("--bg"), flag("--shift"), flag("--twice"));
    let paths: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let target = if bg {
        Target { folder: paths[0].clone(), items: vec![], extended: shift }
    } else {
        let folder = paths[0].parent().unwrap().to_path_buf();
        Target { folder, items: paths, extended: shift }
    };
    for round in 0..if twice { 2 } else { 1 } {
        let started = std::time::Instant::now();
        let Some(p) = probe(&target) else {
            println!("no menu");
            return;
        };
        println!(
            "round {round}: top {:?}, all {:?}, fills {:?}",
            p.top,
            started.elapsed(),
            p.fills.iter().map(|(k, d)| format!("{k}={}ms", d.as_millis())).collect::<Vec<_>>()
        );
        if round == 0 || !twice {
            println!("--- raw");
            print(&p.raw, 0);
            println!("--- shown");
            print(&p.shown, 0);
        }
    }
}

fn print(items: &[Item], depth: usize) {
    for i in items {
        let pad = "  ".repeat(depth + 1);
        if i.separator {
            println!("{pad}----");
            continue;
        }
        let mut line = format!("{pad}{:?}", i.title());
        if let Some(v) = &i.verb {
            line += &format!(" verb={v}");
        }
        if let Some(x) = &i.icon {
            line += &format!(" icon={}x{}", x.size[0], x.size[1]);
        }
        if i.owner_drawn {
            line += &format!(" OWNERDRAWN drawn={:?} help={:?}", i.drawn.as_ref().map(|x| x.size), i.help);
        }
        for (on, what) in [(!i.enabled, "disabled"), (i.checked, "checked"), (i.default, "default")] {
            if on {
                line += &format!(" {what}");
            }
        }
        if i.is_submenu() {
            line += &format!(" [{:?}]", i.cascade);
        }
        println!("{line}");
        print(&i.submenu, depth + 1);
    }
}
