//! Prints every app window the dock would show and who it says it is: app id, program and the
//! relaunch command, name and icon it sets for the taskbar. For checking how an app (Steam,
//! browsers' web apps) presents itself: `cargo run -p shellman-core --example windows_probe`.

fn main() {
    let mut list = shellman_core::winlist::WindowList::default();
    for w in list.refresh() {
        println!("{:#x}  {:?}", w.hwnd, w.title);
        println!("    app_id   {}", w.app_id);
        println!("    exe      {:?}", w.exe);
        if let Some(r) = &w.relaunch {
            println!("    relaunch {:?}\n    name     {:?}\n    icon     {:?}", r.command, r.name, r.icon);
        }
    }
}
