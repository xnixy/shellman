//! Gives the exe its icon, so the shell, Task Manager and the dock show it.
fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/settings.ico");
    winresource::WindowsResource::new().set_icon("../../assets/icons/settings.ico").compile().unwrap();
}
