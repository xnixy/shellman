//! Gives the exe its icon, so Task Manager shows it.
fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/shellman.ico");
    winresource::WindowsResource::new().set_icon("../../assets/icons/shellman.ico").compile().unwrap();
}
