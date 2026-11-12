//! Exports the DLL's two COM entry points as PRIVATE (by name for GetProcAddress, left out
//! of the import library), as the linker asks of DllGetClassObject and DllCanUnloadNow.
fn main() {
    let def = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("exports.def");
    println!("cargo:rerun-if-changed=exports.def");
    println!("cargo:rustc-cdylib-link-arg=/DEF:{}", def.display());
}
