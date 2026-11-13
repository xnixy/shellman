//! Gives the exe its icon, so the shell, Task Manager and the dock show it, and puts Microsoft's
//! ConPTY (`third_party/conpty`: `conpty.dll` and the `OpenConsole.exe` it starts) next to the
//! exe, where the terminal looks for it (`src/conpty.rs`).
fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/shellman.ico");
    winresource::WindowsResource::new().set_icon("../../assets/icons/shellman.ico").compile().unwrap();
    bundle_conpty();
}

fn bundle_conpty() {
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => "x64",
        // Only the x64 files are kept in the repository; other builds use Windows' ConPTY.
        _ => return,
    };
    let src = std::path::Path::new("../../third_party/conpty").join(arch);
    // OUT_DIR is target\<profile>\build\<crate>-<hash>\out: the exe lands in target\<profile>.
    let Some(out) = std::env::var_os("OUT_DIR") else { return };
    let Some(profile_dir) = std::path::Path::new(&out).ancestors().nth(3) else { return };
    for name in ["conpty.dll", "OpenConsole.exe"] {
        let from = src.join(name);
        println!("cargo:rerun-if-changed={}", from.display());
        let to = profile_dir.join(name);
        if std::fs::read(&to).ok() == std::fs::read(&from).ok() {
            continue;
        }
        if std::fs::copy(&from, &to).is_ok() {
            continue;
        }
        // A running terminal keeps these open: set the old one aside rather than fail the build.
        let aside = profile_dir.join(format!("{name}.old"));
        let _ = std::fs::remove_file(&aside);
        let _ = std::fs::rename(&to, &aside);
        if let Err(e) = std::fs::copy(&from, &to) {
            println!("cargo:warning=could not copy {name} next to the exe: {e}");
        }
    }
}
