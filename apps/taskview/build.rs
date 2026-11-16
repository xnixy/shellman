//! Gives the exe its icon, so the shell, Task Manager and the dock show it.
fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/shellman.ico");
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/icons/shellman.ico");
    shellman_buildinfo::apply(&mut res);
    res.compile().unwrap();
}
