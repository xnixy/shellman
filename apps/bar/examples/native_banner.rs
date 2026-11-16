#![allow(dead_code)]
//! Tries `notify::native` alone: hooks Windows' own notification banner for 20 s, so a toast sent
//! meanwhile shows only shellman's copy. `cargo run -p shellman-bar --example native_banner`.

#[path = "../src/notify/native.rs"]
mod native;

fn main() {
    native::start();
    std::thread::sleep(std::time::Duration::from_millis(200));
    native::ensure();
    println!("hooked; send a notification within 20 s");
    std::thread::sleep(std::time::Duration::from_secs(20));
}
