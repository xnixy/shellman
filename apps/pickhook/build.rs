//! Exports the DLL's two COM entry points as PRIVATE (by name for GetProcAddress, left out
//! of the import library), as the linker asks of DllGetClassObject and DllCanUnloadNow.
fn main() {
    // Read when the script runs, not baked in when it was compiled: a checkout that moved
    // would otherwise point the linker at the old folder.
    let dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let def = std::path::Path::new(&dir).join("exports.def");
    println!("cargo:rerun-if-changed=exports.def");
    println!("cargo:rustc-cdylib-link-arg=/DEF:{}", def.display());
}
