//! Drives the shell menu the way Files does, without a window: walks on the worker, picks the
//! entry with the given verb, and reports the windows that open in this process. For checking
//! that commands which open their own windows still work.
//!
//! `cargo run -p shellman-core --example shell_menu_invoke -- VERB PATH`

use shellman_core::shell_menu::{Item, Loader, Target};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
};

fn find<'a>(items: &'a [Item], verb: &str, path: &mut Vec<&'a Item>) -> bool {
    for i in items {
        path.push(i);
        if i.verb.as_deref() == Some(verb) || find(&i.submenu, verb, path) {
            return true;
        }
        path.pop();
    }
    false
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (verb, file) = (args[0].as_str(), PathBuf::from(&args[1]));
    let target = Target { folder: file.parent().unwrap().into(), items: vec![file], extended: false };
    let mut loader = Loader::new(0, Arc::new(|| {}));
    loader.want(&target);
    let start = Instant::now();
    loop {
        loader.poll();
        let v = loader.view(&target).unwrap();
        if !v.loading {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    println!("walk done in {:?}", start.elapsed());
    let v = loader.view(&target).unwrap();
    let items: Vec<Item> = v.items.to_vec();
    let mut chain = Vec::new();
    if !find(&items, verb, &mut chain) {
        println!("no entry with verb {verb}");
        return;
    }
    println!("picking {:?}", chain.last().unwrap().title());
    let route = loader.pick(&target, &chain, true, (400, 400));
    println!("route {route:?}");
    for s in 0..10 {
        std::thread::sleep(Duration::from_secs(1));
        let w = windows_of_this_process();
        println!("{s}s: {w:?}");
        if !w.is_empty() && s >= 2 {
            break;
        }
    }
}

fn windows_of_this_process() -> Vec<String> {
    unsafe extern "system" fn visit(h: HWND, l: LPARAM) -> BOOL {
        let out = unsafe { &mut *(l.0 as *mut Vec<String>) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        if pid == unsafe { GetCurrentProcessId() } && unsafe { IsWindowVisible(h) }.as_bool() {
            let mut buf = [0u16; 200];
            let n = unsafe { GetWindowTextW(h, &mut buf) } as usize;
            out.push(String::from_utf16_lossy(&buf[..n]));
        }
        BOOL(1)
    }
    let mut out: Vec<String> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&raw mut out as isize));
    }
    out
}
