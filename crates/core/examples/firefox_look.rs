//! Drive the Firefox look from a console, for checking it against a real Firefox with a
//! throwaway profile (never point it at a profile you use without a copy):
//!
//! ```text
//! cargo run -p shellman-core --example firefox_look -- status  <profile>
//! cargo run -p shellman-core --example firefox_look -- install <profile> [--scripts <firefox folder>]
//! cargo run -p shellman-core --example firefox_look -- remove  <profile>
//! ```
//!
//! `--scripts` writes the loader straight into the Firefox folder, so give it a writable copy
//! (Settings goes through the administrator helper for Program Files). The files come from the
//! repo's `integrations\firefox`.

use shellman_core::firefox::{loader, look, Source};
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(cmd), Some(profile)) = (args.first(), args.get(1)) else {
        eprintln!("usage: firefox_look status|install|remove <profile> [--scripts <firefox folder>]");
        std::process::exit(2);
    };
    let profile = PathBuf::from(profile);
    let scripts = args.iter().position(|a| a == "--scripts").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let src = Source::at(&Path::new(env!("CARGO_MANIFEST_DIR")).join("..\\..\\integrations\\firefox")).expect("the repo's integration");
    let r = match cmd.as_str() {
        "status" => Ok(format!("{:?}", look::status(&profile, Some(&src)))),
        "install" => (|| {
            if let Some(ff) = &scripts {
                loader::write(ff)?;
            }
            look::install(&profile, &src, scripts.as_deref()).map(|d| format!("{d:?}")).map_err(|e| e.message())
        })(),
        "remove" => look::remove(&profile).map_err(|e| e.message()).and_then(|d| {
            if let Some(ff) = &d.scripts_for {
                loader::remove(ff)?;
            }
            Ok(format!("{d:?}"))
        }),
        _ => Err(format!("unknown command {cmd}")),
    };
    match r {
        Ok(s) => println!("{s}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
