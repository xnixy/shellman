//! Embeds the built-in icon pack (`assets/icon-packs/islands`): writes a table of its icons, each
//! with its large and small SVG, for `icon_pack::builtin`.

use std::fmt::Write;
use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icon-packs/islands");
    let big = root.join("scalable/apps");
    let small = root.join("24x24/apps");
    println!("cargo:rerun-if-changed={}", big.display());
    println!("cargo:rerun-if-changed={}", small.display());
    let mut names: Vec<String> = std::fs::read_dir(&big)
        .expect("assets/icon-packs/islands/scalable/apps")
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|f| f.strip_suffix(".svg").map(str::to_string))
        .collect();
    names.sort();
    let mut out = String::from("/// The built-in pack's icons: (name, large SVG, small SVG), by name.\npub static FILES: &[(&str, &[u8], &[u8])] = &[\n");
    for name in &names {
        let b = big.join(format!("{name}.svg"));
        let s = small.join(format!("{name}.svg"));
        assert!(s.is_file(), "{name} has no small variant");
        println!("cargo:rerun-if-changed={}", b.display());
        println!("cargo:rerun-if-changed={}", s.display());
        let _ = writeln!(out, "    ({name:?}, include_bytes!({:?}), include_bytes!({:?})),", b, s);
    }
    out.push_str("];\n");
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("islands.rs");
    std::fs::write(dest, out).unwrap();
}
