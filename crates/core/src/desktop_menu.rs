//! The wallpaper's right-click menu, between the two processes that make the desktop: the
//! desktop process (plain Win32, the wallpaper windows) hears the click, and the shelf process
//! (`shellman-shelf`, egui and glass) shows the menu.
//!
//! The desktop finds the shelf's window by its title, lets the shelf's process take the
//! foreground (the desktop got the click, so it may hand that right on), and posts it a
//! registered message with the point. If no shelf runs, it starts one with `--menu x y`.
//! `SHELLMAN_INSTANCE` is part of the title, so a test copy talks to its own shelf.

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, FindWindowW, GetWindowThreadProcessId, PostMessageW, RegisterWindowMessageW,
};

/// The command-line flag that opens the menu at a point: `--menu <x> <y>` (physical pixels).
pub const MENU_FLAG: &str = "--menu";

/// The shelf window's title, which is how the desktop finds it.
pub fn shelf_title() -> String {
    match std::env::var("SHELLMAN_INSTANCE").ok().filter(|i| !i.is_empty()) {
        Some(instance) => format!("shellman shelf {instance}"),
        None => "shellman shelf".to_owned(),
    }
}

/// The message the shelf's window gets: `wparam` and `lparam` are the point's x and y, in
/// physical screen pixels (as `i32`).
pub fn message() -> u32 {
    static MESSAGE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("shellman-desktop-menu")) })
}

/// Pack a point for the message.
pub fn pack(x: i32, y: i32) -> (WPARAM, LPARAM) {
    (WPARAM(x as u32 as usize), LPARAM(y as isize))
}

/// The point a message carries.
pub fn unpack(wparam: WPARAM, lparam: LPARAM) -> (i32, i32) {
    (wparam.0 as u32 as i32, lparam.0 as i32)
}

/// `--menu x y` in a command line, if there.
pub fn parse_args(args: &[String]) -> Option<(i32, i32)> {
    let at = args.iter().position(|a| a == MENU_FLAG)?;
    let x = args.get(at + 1)?.parse().ok()?;
    let y = args.get(at + 2)?.parse().ok()?;
    Some((x, y))
}

/// Ask the shelf to show the desktop menu at `(x, y)`. Call from the process that got the click.
pub fn request(x: i32, y: i32) {
    unsafe {
        if let Ok(hwnd) = FindWindowW(None, &HSTRING::from(shelf_title())) {
            if post(hwnd, x, y) {
                return;
            }
        }
    }
    // No shelf (not started yet, or it ended): start one that opens the menu.
    crate::sibling::start_with("shelf", &[MENU_FLAG, &x.to_string(), &y.to_string()]);
}

unsafe fn post(hwnd: HWND, x: i32, y: i32) -> bool {
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid != 0 {
        let _ = AllowSetForegroundWindow(pid);
    }
    let (wp, lp) = pack(x, y);
    PostMessageW(Some(hwnd), message(), wp, lp).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_survive_the_message_even_left_of_the_primary() {
        for (x, y) in [(0, 0), (1920, 1080), (-3840, -12), (7679, 2159)] {
            let (wp, lp) = pack(x, y);
            assert_eq!(unpack(wp, lp), (x, y));
        }
    }

    #[test]
    fn the_menu_flag_takes_two_numbers() {
        let args = |s: &[&str]| s.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert_eq!(parse_args(&args(&["shelf.exe", "--menu", "-100", "250"])), Some((-100, 250)));
        assert_eq!(parse_args(&args(&["shelf.exe", "--menu", "x", "250"])), None);
        assert_eq!(parse_args(&args(&["shelf.exe", "--menu", "5"])), None);
        assert_eq!(parse_args(&args(&["shelf.exe"])), None);
    }
}
